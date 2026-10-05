//! 資產（Asset）領域模組：欄位驗證、屆齡判斷與資料庫存取。
//!
//! 詞彙依 `GLOSSARY.md`；規則見 `.scratch/asset-ip-management/spec.md` §2.1、§3。

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use chrono::{Datelike, Local, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, QueryBuilder, Sqlite, SqliteConnection, SqlitePool};

use crate::api::ApiError;
use crate::assignments;

/// `assets` 資料表完整欄位清單。
const COLUMNS: &str = "id, property_no, description, location, device_serial, brand, model, \
                       purchase_date, lifespan_years, note, tags, created_at, updated_at";

/// `assets` 資料表列；`tags` 為 JSON 陣列字串（DB 格式）。
#[derive(Debug, FromRow)]
struct AssetRow {
    id: i64,
    property_no: Option<String>,
    description: String,
    location: String,
    device_serial: Option<String>,
    brand: Option<String>,
    model: Option<String>,
    purchase_date: Option<String>,
    lifespan_years: Option<i64>,
    note: Option<String>,
    tags: String,
    created_at: String,
    updated_at: String,
}

/// API 回傳的資產（含後端計算的屆齡旗標）。
#[derive(Debug, Serialize)]
pub struct Asset {
    pub id: i64,
    pub property_no: Option<String>,
    pub description: String,
    pub location: String,
    pub device_serial: Option<String>,
    pub brand: Option<String>,
    pub model: Option<String>,
    pub purchase_date: Option<String>,
    pub lifespan_years: Option<i64>,
    pub note: Option<String>,
    /// 標籤：多值自由文字（正規化後、不分大小寫去重）。
    pub tags: Vec<String>,
    /// 屆齡：`purchase_date + lifespan_years < 今天`（僅提示，不影響操作）。
    pub expired: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// 新增資產的輸入；所有欄位先收為 `Option`，缺漏必填欄位由 `validate` 回報。
#[derive(Debug, Default, Deserialize)]
pub struct AssetInput {
    pub property_no: Option<String>,
    pub description: Option<String>,
    pub location: Option<String>,
    pub device_serial: Option<String>,
    pub brand: Option<String>,
    pub model: Option<String>,
    pub purchase_date: Option<String>,
    pub lifespan_years: Option<i64>,
    pub note: Option<String>,
    pub tags: Option<Vec<String>>,
}

/// 已驗證的新增內容。
#[derive(Debug)]
pub struct ValidAsset {
    property_no: Option<String>,
    description: String,
    location: String,
    device_serial: Option<String>,
    brand: Option<String>,
    model: Option<String>,
    purchase_date: Option<String>,
    lifespan_years: Option<i64>,
    note: Option<String>,
    tags: Vec<String>,
}

/// 編輯資產的輸入。
///
/// 外層 `None`＝欄位未提供（維持原值）；`Some(None)`＝顯式 `null`（清除）；
/// `Some(Some(..))`＝設定新值。
#[derive(Debug, Default, Deserialize)]
pub struct AssetPatch {
    #[serde(default, deserialize_with = "double_option")]
    pub property_no: Option<Option<String>>,
    pub description: Option<String>,
    pub location: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub device_serial: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub brand: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub model: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub purchase_date: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub lifespan_years: Option<Option<i64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub note: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub tags: Option<Option<Vec<String>>>,
}

/// 已驗證的編輯內容；語意同 [`AssetPatch`]，但選填欄位已正規化。
#[derive(Debug)]
pub struct ValidPatch {
    property_no: Option<Option<String>>,
    description: Option<String>,
    location: Option<String>,
    device_serial: Option<Option<String>>,
    brand: Option<Option<String>>,
    model: Option<Option<String>>,
    purchase_date: Option<Option<String>>,
    lifespan_years: Option<Option<i64>>,
    note: Option<Option<String>>,
    tags: Option<Option<Vec<String>>>,
}

impl AssetInput {
    /// 驗證新增欄位：描述與位置必填（見 spec §2.1）。
    pub fn validate(self) -> Result<ValidAsset, ApiError> {
        Ok(ValidAsset {
            property_no: optional_text(self.property_no),
            description: require_text("description", "描述", self.description)?,
            location: require_text("location", "位置", self.location)?,
            device_serial: optional_text(self.device_serial),
            brand: optional_text(self.brand),
            model: optional_text(self.model),
            purchase_date: validate_purchase_date(self.purchase_date)?,
            lifespan_years: validate_lifespan(self.lifespan_years)?,
            note: optional_text(self.note),
            tags: normalize_tags(self.tags.unwrap_or_default()),
        })
    }
}

impl AssetPatch {
    /// 驗證編輯欄位；未提供的欄位不驗證也不變更。
    pub fn validate(self) -> Result<ValidPatch, ApiError> {
        Ok(ValidPatch {
            property_no: self.property_no.map(optional_text),
            description: match self.description {
                Some(value) => Some(require_text("description", "描述", Some(value))?),
                None => None,
            },
            location: match self.location {
                Some(value) => Some(require_text("location", "位置", Some(value))?),
                None => None,
            },
            device_serial: self.device_serial.map(optional_text),
            brand: self.brand.map(optional_text),
            model: self.model.map(optional_text),
            purchase_date: match self.purchase_date {
                Some(value) => Some(validate_purchase_date(value)?),
                None => None,
            },
            lifespan_years: match self.lifespan_years {
                Some(value) => Some(validate_lifespan(value)?),
                None => None,
            },
            note: self.note.map(optional_text),
            // 顯式 null（`Some(None)`）＝清空為空陣列。
            tags: self
                .tags
                .map(|tags| Some(normalize_tags(tags.unwrap_or_default()))),
        })
    }
}

/// 清單可排序欄位白名單（見票 11、票 19、票 08）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortField {
    PropertyNo,
    #[default]
    Description,
    Location,
    Brand,
    Model,
    Note,
    Tags,
    Expired,
    AssignedIps,
    LastSeen,
}

impl SortField {
    /// 由查詢參數字串解析；不在白名單回傳 `None`（由 API 層回 400）。
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "property_no" => Some(Self::PropertyNo),
            "description" => Some(Self::Description),
            "location" => Some(Self::Location),
            "brand" => Some(Self::Brand),
            "model" => Some(Self::Model),
            "note" => Some(Self::Note),
            "tags" => Some(Self::Tags),
            "expired" => Some(Self::Expired),
            "assigned_ips" => Some(Self::AssignedIps),
            "last_seen" => Some(Self::LastSeen),
            _ => None,
        }
    }

    /// `ORDER BY` 用的運算式；文字欄位以 `COLLATE NOCASE` 排序。
    fn order_expression(self) -> &'static str {
        match self {
            Self::PropertyNo => "property_no COLLATE NOCASE",
            Self::Description => "description COLLATE NOCASE",
            Self::Location => "location COLLATE NOCASE",
            Self::Brand => "brand COLLATE NOCASE",
            Self::Model => "model COLLATE NOCASE",
            Self::Note => "note COLLATE NOCASE",
            Self::Tags => "tags COLLATE NOCASE",
            Self::Expired => EXPIRED_EXPR,
            // 「已指派 IP」於 Rust 端比較位址數值（SQLite 無 inet 型別）；
            // SQL 端僅需穩定基底，實際排序見 `list_all_by_assigned_ips`（票 19）。
            Self::AssignedIps => "id",
            // 「最後可見」為觀測現況的聚合、且須 NULL-last；由 `push_order`
            // 以 [`LAST_SEEN_EXPR`] 特判（見票 08）。
            Self::LastSeen => "id",
        }
    }
}

/// 排序方向（見票 11）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortDir {
    #[default]
    Asc,
    Desc,
}

impl SortDir {
    /// 由查詢參數字串解析；`asc`／`desc` 以外回傳 `None`（由 API 層回 400）。
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "asc" => Some(Self::Asc),
            "desc" => Some(Self::Desc),
            _ => None,
        }
    }

    fn keyword(self) -> &'static str {
        match self {
            Self::Asc => "ASC",
            Self::Desc => "DESC",
        }
    }
}

/// 清單的搜尋、篩選、排序與分頁條件（皆為伺服器端，見 spec §2.1）。
#[derive(Debug, Default)]
pub struct AssetFilter {
    /// 關鍵字：財產編號／描述／設備序號／廠牌／型號／備註／MAC／已指派位址
    /// （子字串、不分大小寫；見 spec §2.1）。
    pub q: Option<String>,
    /// 位置：不分大小寫完全符合。
    pub location: Option<String>,
    /// 廠牌：不分大小寫完全符合。
    pub brand: Option<String>,
    /// 設備序號：不分大小寫完全符合（供重複提示與精確查找）。
    pub device_serial: Option<String>,
    /// 標籤：JSON 陣列任一元素不分大小寫完全符合（見票 11）。
    pub tag: Option<String>,
    /// 排序欄位；預設描述。
    pub sort: SortField,
    /// 排序方向；預設升冪。
    pub dir: SortDir,
    pub page: i64,
    pub per_page: i64,
}

/// 搜尋／篩選後的分頁查詢；回傳（當頁資產、符合總數）。
pub async fn list(pool: &SqlitePool, filter: &AssetFilter) -> sqlx::Result<(Vec<Asset>, i64)> {
    // 「已指派 IP」需以第一筆位址跨全部符合資產比較，改於 Rust 端排序後切頁
    // （見票 19）；其餘欄位維持 SQL 端排序與分頁。
    if filter.sort == SortField::AssignedIps {
        let all = list_all_by_assigned_ips(pool, filter).await?;
        let total = all.len() as i64;
        return Ok((take_page(all, filter), total));
    }

    let mut count = QueryBuilder::new("SELECT COUNT(*) FROM assets WHERE 1 = 1");
    push_filters(&mut count, filter);
    let total = count.build_query_scalar::<i64>().fetch_one(pool).await?;

    let mut query = QueryBuilder::new(format!("SELECT {COLUMNS} FROM assets WHERE 1 = 1"));
    push_filters(&mut query, filter);
    push_order(&mut query, filter);
    query.push(" LIMIT ").push_bind(filter.per_page);
    query
        .push(" OFFSET ")
        .push_bind((filter.page - 1).max(0) * filter.per_page);
    let rows: Vec<AssetRow> = query.build_query_as().fetch_all(pool).await?;

    Ok((into_assets(rows), total))
}

/// 搜尋／篩選後的全部符合資產（不分頁；供匯出使用，見票 04）。
pub async fn list_all(pool: &SqlitePool, filter: &AssetFilter) -> sqlx::Result<Vec<Asset>> {
    if filter.sort == SortField::AssignedIps {
        return list_all_by_assigned_ips(pool, filter).await;
    }

    let mut query = QueryBuilder::new(format!("SELECT {COLUMNS} FROM assets WHERE 1 = 1"));
    push_filters(&mut query, filter);
    push_order(&mut query, filter);
    let rows: Vec<AssetRow> = query.build_query_as().fetch_all(pool).await?;

    Ok(into_assets(rows))
}

/// 依第一筆已指派 IP 排序全部符合資產（供 `sort=assigned_ips` 的清單與匯出；
/// 見票 19）。
///
/// 排序鍵為顯示序第一筆位址（v4 先、v6 後、同族依數值；見
/// [`assignments::list_for_assets`]），未指派固定排最後（asc／desc 皆然），
/// 同鍵以 id 升冪決勝；desc 為位址鍵的完全反向（v6 排在 v4 前）。SQLite 無
/// inet 型別，於 Rust 端解析位址比較；成本與關鍵字搜尋同級，切頁由呼叫端
/// 以 [`take_page`] 負責。
async fn list_all_by_assigned_ips(
    pool: &SqlitePool,
    filter: &AssetFilter,
) -> sqlx::Result<Vec<Asset>> {
    let mut query = QueryBuilder::new(format!("SELECT {COLUMNS} FROM assets WHERE 1 = 1"));
    push_filters(&mut query, filter);
    let rows: Vec<AssetRow> = query.build_query_as().fetch_all(pool).await?;
    let assets = into_assets(rows);

    let ids: Vec<i64> = assets.iter().map(|asset| asset.id).collect();
    let mut assigned = assignments::list_for_assets(pool, &ids).await?;

    let mut items: Vec<(Asset, Option<String>)> = assets
        .into_iter()
        .map(|asset| {
            let first = assigned
                .remove(&asset.id)
                .and_then(|addresses| addresses.into_iter().next());
            (asset, first)
        })
        .collect();

    items.sort_by(|(left, left_ip), (right, right_ip)| {
        compare_first_assigned_ip(left_ip.as_deref(), right_ip.as_deref(), filter.dir)
            .then_with(|| left.id.cmp(&right.id))
    });

    Ok(items.into_iter().map(|(asset, _)| asset).collect())
}

/// 比較兩資產的第一筆已指派位址；未指派（`None`）固定排最後（asc／desc 皆然）。
fn compare_first_assigned_ip(left: Option<&str>, right: Option<&str>, dir: SortDir) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => {
            let order =
                assignments::address_sort_key(left).cmp(&assignments::address_sort_key(right));
            match dir {
                SortDir::Asc => order,
                SortDir::Desc => order.reverse(),
            }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// 資產「最後可見」的 SQL 純量運算式（相關子查詢；見票 08、spec §讀取端）。
///
/// 取該資產命中的 `ip_presence.last_seen_at` 最大值（觀測唯讀，見 ADR-0014）；
/// 命中路徑：
/// 1. 指派命中：任一介面的指派 `(subnet_id, address)` 與現況列相同；
/// 2. MAC 命中：現況 `last_seen_mac` 等於（不分大小寫）任一介面 MAC
///    （含未指派位址）。
///
/// 無命中或 `last_seen_at` 全為空時為 NULL；以 `assets` 為外層表名。
const LAST_SEEN_EXPR: &str = "(SELECT MAX(p.last_seen_at) \
     FROM ip_presence p \
    WHERE p.last_seen_at IS NOT NULL \
      AND (EXISTS (SELECT 1 \
                     FROM ip_assignments a \
                     JOIN interfaces i ON i.id = a.interface_id \
                    WHERE i.asset_id = assets.id \
                      AND a.subnet_id = p.subnet_id \
                      AND a.address = p.address) \
       OR EXISTS (SELECT 1 \
                    FROM interfaces m \
                   WHERE m.asset_id = assets.id \
                     AND LOWER(m.mac) = LOWER(p.last_seen_mac))))";

/// 批次讀取多資產的「最後可見」時間（單一查詢，避免逐資產 N+1；見票 08）。
///
/// 回傳僅含至少一個命中現況的資產（無命中者由呼叫端視為 NULL）；
/// 判定與 [`LAST_SEEN_EXPR`] 相同。
pub async fn last_seen_for_assets(
    pool: &SqlitePool,
    asset_ids: &[i64],
) -> sqlx::Result<HashMap<i64, String>> {
    if asset_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut query = QueryBuilder::new(format!(
        "SELECT assets.id AS asset_id, {LAST_SEEN_EXPR} AS last_seen_at \
           FROM assets WHERE assets.id IN ("
    ));
    let mut separated = query.separated(", ");
    for asset_id in asset_ids {
        separated.push_bind(*asset_id);
    }
    separated.push_unseparated(")");

    let rows: Vec<(i64, Option<String>)> = query.build_query_as().fetch_all(pool).await?;
    Ok(rows
        .into_iter()
        .filter_map(|(asset_id, last_seen_at)| last_seen_at.map(|value| (asset_id, value)))
        .collect())
}

/// 單一資產的「最後可見」；無命中回 `None`（見票 08）。
pub async fn last_seen_for_asset(pool: &SqlitePool, asset_id: i64) -> sqlx::Result<Option<String>> {
    Ok(last_seen_for_assets(pool, std::slice::from_ref(&asset_id))
        .await?
        .remove(&asset_id))
}

/// 依 `filter` 的頁碼與每頁筆數切取一頁；供 Rust 端排序後的清單使用。
fn take_page(assets: Vec<Asset>, filter: &AssetFilter) -> Vec<Asset> {
    let offset = ((filter.page - 1).max(0) * filter.per_page).max(0) as usize;
    let limit = filter.per_page.max(0) as usize;
    assets.into_iter().skip(offset).take(limit).collect()
}

/// 讀取單一資產；不存在回傳 `None`。
pub async fn get(pool: &SqlitePool, id: i64) -> sqlx::Result<Option<Asset>> {
    Ok(fetch_row(pool, id)
        .await?
        .map(|row| row.into_asset(today())))
}

/// 於既有連線（可為交易）內新增資產，回傳新列 id；不讀回完整資料。
///
/// 供匯入在同一交易內建立資產（見票 01）；一般建立路徑走 [`create`]。
pub(crate) async fn insert_asset(
    connection: &mut SqliteConnection,
    asset: ValidAsset,
) -> sqlx::Result<i64> {
    let result = sqlx::query(
        "INSERT INTO assets
             (property_no, description, location, device_serial, brand, model,
              purchase_date, lifespan_years, note, tags)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(asset.property_no)
    .bind(asset.description)
    .bind(asset.location)
    .bind(asset.device_serial)
    .bind(asset.brand)
    .bind(asset.model)
    .bind(asset.purchase_date)
    .bind(asset.lifespan_years)
    .bind(asset.note)
    .bind(tags_json(&asset.tags))
    .execute(connection)
    .await?;

    Ok(result.last_insert_rowid())
}

/// 新增資產並回傳入庫後的內容；由連線池取連線呼叫 [`insert_asset`]。
pub async fn create(pool: &SqlitePool, asset: ValidAsset) -> sqlx::Result<Asset> {
    let id = {
        let mut connection = pool.acquire().await?;
        insert_asset(&mut connection, asset).await?
    };

    fetch_row(pool, id)
        .await?
        .map(|row| row.into_asset(today()))
        .ok_or(sqlx::Error::RowNotFound)
}

/// 編輯資產；不存在回傳 `None`。
pub async fn update(pool: &SqlitePool, id: i64, patch: ValidPatch) -> sqlx::Result<Option<Asset>> {
    let mut query = QueryBuilder::new("UPDATE assets SET ");
    let mut first = true;

    macro_rules! set {
        ($column:literal, $value:expr) => {
            if !first {
                query.push(", ");
            }
            first = false;
            query.push($column).push(" = ").push_bind($value);
        };
    }

    if let Some(value) = patch.property_no {
        set!("property_no", value);
    }
    if let Some(value) = patch.description {
        set!("description", value);
    }
    if let Some(value) = patch.location {
        set!("location", value);
    }
    if let Some(value) = patch.device_serial {
        set!("device_serial", value);
    }
    if let Some(value) = patch.brand {
        set!("brand", value);
    }
    if let Some(value) = patch.model {
        set!("model", value);
    }
    if let Some(value) = patch.purchase_date {
        set!("purchase_date", value);
    }
    if let Some(value) = patch.lifespan_years {
        set!("lifespan_years", value);
    }
    if let Some(value) = patch.note {
        set!("note", value);
    }
    if let Some(value) = patch.tags {
        set!("tags", tags_json(&value.unwrap_or_default()));
    }
    if !first {
        query.push(", ");
    }
    query.push("updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')");
    query.push(" WHERE id = ").push_bind(id);

    let result = query.build().execute(pool).await?;
    if result.rows_affected() == 0 {
        return Ok(None);
    }

    Ok(fetch_row(pool, id)
        .await?
        .map(|row| row.into_asset(today())))
}

/// 刪除資產；介面與指派由外鍵連動刪除（確認數量由前端讀詳情計算，見票 08）。
pub async fn delete(pool: &SqlitePool, id: i64) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM assets WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// 位置建議值：既有位置去重（不分大小寫），供輸入時自動完成（見 spec §6）。
pub async fn locations(pool: &SqlitePool) -> sqlx::Result<Vec<String>> {
    suggestions(pool, "location").await
}

/// 廠牌建議值：規則同位置，供篩選選單使用。
pub async fn brands(pool: &SqlitePool) -> sqlx::Result<Vec<String>> {
    suggestions(pool, "brand").await
}

/// 標籤建議值：所有已使用標籤去重（不分大小寫），同值不同大小寫保留最早寫入的原文（見票 11）。
///
/// 以視窗函式單次展開所有標籤後，取每個標籤（NOCASE）中 id 最小的資產原文；原本的相關子查詢
/// 會對每個標籤列重掃全表展開標籤，成本隨 `資產數 × 標籤數` 的平方成長。
pub async fn tags(pool: &SqlitePool) -> sqlx::Result<Vec<String>> {
    sqlx::query_scalar(
        "SELECT value
         FROM (
             SELECT je.value AS value,
                    ROW_NUMBER() OVER (
                        PARTITION BY je.value COLLATE NOCASE
                        ORDER BY a.id, je.value COLLATE NOCASE, je.value
                    ) AS rn
             FROM assets a, json_each(a.tags) AS je
             WHERE trim(je.value) <> ''
         )
         WHERE rn = 1
         ORDER BY value COLLATE NOCASE ASC",
    )
    .fetch_all(pool)
    .await
}

/// 某文字欄位的既有值去重清單；同值不同大小寫保留最早寫入的原文。
async fn suggestions(pool: &SqlitePool, column: &str) -> sqlx::Result<Vec<String>> {
    let sql = format!(
        "SELECT {column} FROM assets
         WHERE id IN (
             SELECT MIN(id) FROM assets
             WHERE {column} IS NOT NULL AND trim({column}) <> ''
             GROUP BY {column} COLLATE NOCASE
         )
         ORDER BY {column} COLLATE NOCASE ASC"
    );
    sqlx::query_scalar(&sql).fetch_all(pool).await
}

/// 讀取一列；不存在回傳 `None`。
async fn fetch_row(pool: &SqlitePool, id: i64) -> sqlx::Result<Option<AssetRow>> {
    sqlx::query_as::<_, AssetRow>(&format!("SELECT {COLUMNS} FROM assets WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await
}

fn into_assets(rows: Vec<AssetRow>) -> Vec<Asset> {
    let today = today();
    rows.into_iter().map(|row| row.into_asset(today)).collect()
}

fn today() -> NaiveDate {
    Local::now().date_naive()
}

impl AssetRow {
    fn into_asset(self, today: NaiveDate) -> Asset {
        let expired = is_expired(self.purchase_date.as_deref(), self.lifespan_years, today);
        Asset {
            id: self.id,
            property_no: self.property_no,
            description: self.description,
            location: self.location,
            device_serial: self.device_serial,
            brand: self.brand,
            model: self.model,
            purchase_date: self.purchase_date,
            lifespan_years: self.lifespan_years,
            note: self.note,
            // 正常寫入一律為合法 JSON；手動改庫等異常值退化為空陣列。
            tags: serde_json::from_str(&self.tags).unwrap_or_default(),
            expired,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// 在篩選條件之後附加排序子句；欄位與方向經白名單驗證，以 id 作為穩定
/// 排序的決勝鍵（見 spec §6、票 11）。
///
/// 「最後可見」須 NULL（無命中現況）固定排最後、不分升降冪，比照 IP 清單
/// （見 spec §讀取端、票 08）。
fn push_order(query: &mut QueryBuilder<'_, Sqlite>, filter: &AssetFilter) {
    query.push(" ORDER BY ");
    if filter.sort == SortField::LastSeen {
        query
            .push("(")
            .push(LAST_SEEN_EXPR)
            .push(") IS NULL ASC, ")
            .push(LAST_SEEN_EXPR)
            .push(" ")
            .push(filter.dir.keyword());
    } else {
        query
            .push(filter.sort.order_expression())
            .push(" ")
            .push(filter.dir.keyword());
    }
    query.push(", id ASC");
}

/// 在篩選條件之後附加 SQL；關鍵字以 `LIKE` 子字串比對（SQLite 對 ASCII 不分大小寫）。
fn push_filters<'a>(query: &mut QueryBuilder<'a, Sqlite>, filter: &'a AssetFilter) {
    if let Some(q) = filter.q.as_deref().and_then(trimmed) {
        let pattern = format!("%{}%", escape_like(q));
        query.push(" AND (");
        for (index, column) in [
            "property_no",
            "description",
            "device_serial",
            "brand",
            "model",
            "note",
        ]
        .iter()
        .enumerate()
        {
            if index > 0 {
                query.push(" OR ");
            }
            query
                .push(*column)
                .push(" LIKE ")
                .push_bind(pattern.clone())
                .push(" ESCAPE '\\'");
        }
        // MAC 與已指派位址（經由介面）以 EXISTS 子查詢比對，避免 JOIN 造成列重複（見票 12）。
        query
            .push(" OR EXISTS (SELECT 1 FROM interfaces qi WHERE qi.asset_id = assets.id AND qi.mac LIKE ")
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\')");
        query
            .push(
                " OR EXISTS (SELECT 1 FROM ip_assignments qa
                          JOIN interfaces qi ON qi.id = qa.interface_id
                         WHERE qi.asset_id = assets.id AND qa.address LIKE ",
            )
            .push_bind(pattern.clone())
            .push(" ESCAPE '\\')");
        query.push(")");
    }
    if let Some(location) = filter.location.as_deref().and_then(trimmed) {
        query
            .push(" AND location COLLATE NOCASE = ")
            .push_bind(location);
    }
    if let Some(brand) = filter.brand.as_deref().and_then(trimmed) {
        query.push(" AND brand COLLATE NOCASE = ").push_bind(brand);
    }
    if let Some(serial) = filter.device_serial.as_deref().and_then(trimmed) {
        query
            .push(" AND device_serial COLLATE NOCASE = ")
            .push_bind(serial);
    }
    if let Some(tag) = filter.tag.as_deref().and_then(trimmed) {
        query
            .push(" AND EXISTS (SELECT 1 FROM json_each(assets.tags) WHERE value = ")
            .push_bind(tag)
            .push(" COLLATE NOCASE)");
    }
}

/// 正規化標籤：逐項 trim、忽略空字串、不分大小寫去重（保留首次出現原樣）。
///
/// 大小寫折疊比照 SQLite `COLLATE NOCASE`（僅 ASCII），與篩選／建議值一致。
/// 供匯入重用（見票 02）。
pub(crate) fn normalize_tags(tags: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for tag in tags {
        let tag = tag.trim();
        if tag.is_empty() || !seen.insert(tag.to_ascii_lowercase()) {
            continue;
        }
        normalized.push(tag.to_string());
    }
    normalized
}

/// 標籤以 JSON 陣列字串入庫；`Vec<String>` 序列化不會失敗。
fn tags_json(tags: &[String]) -> String {
    serde_json::to_string(tags).unwrap_or_else(|_| "[]".to_string())
}

/// 屆齡的 SQL 運算式（1＝屆齡）；語意與 [`is_expired`] 一致，供排序使用（見票 11）。
///
/// - 以 `date('now', 'localtime')` 對齊 Rust 的 `Local::now().date_naive()`。
/// - SQLite 的 `+N years` 對 2/29 會進位到 3/1，Rust 會退至 2/28，故特別修正。
/// - 無效日期／超界年份的 `date()` 為 NULL，比較結果為 NULL，落入 `ELSE 0`。
const EXPIRED_EXPR: &str = "CASE \
     WHEN purchase_date IS NULL OR lifespan_years IS NULL OR lifespan_years < 0 THEN 0 \
     ELSE CASE WHEN (CASE \
         WHEN strftime('%m-%d', purchase_date) = '02-29' \
              AND strftime('%m-%d', date(purchase_date, '+' || lifespan_years || ' years')) = '03-01' \
         THEN date(purchase_date, '+' || lifespan_years || ' years', '-1 day') \
         ELSE date(purchase_date, '+' || lifespan_years || ' years') \
     END) < date('now', 'localtime') THEN 1 ELSE 0 END \
 END";

/// 轉義 `LIKE` 的萬用字元，讓使用者輸入的 `%`、`_` 被視為字面字元。
fn escape_like(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// 去除前後空白；空字串視為未填。
pub(crate) fn optional_text(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn trimmed(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

/// 必填文字；缺漏或空白回傳結構驗證錯誤（見 ADR-0006）。
fn require_text(
    field: &'static str,
    label: &str,
    value: Option<String>,
) -> Result<String, ApiError> {
    optional_text(value).ok_or_else(|| ApiError::validation(format!("{label}為必填")).field(field))
}

/// 購置日期：空白視為清除；格式須為 `YYYY-MM-DD`。
fn validate_purchase_date(value: Option<String>) -> Result<Option<String>, ApiError> {
    let Some(text) = optional_text(value) else {
        return Ok(None);
    };
    NaiveDate::parse_from_str(&text, "%Y-%m-%d").map_err(|_| {
        ApiError::validation("購置日期須為有效日期（YYYY-MM-DD）").field("purchase_date")
    })?;
    Ok(Some(text))
}

/// 年限：空白視為清除；須為非負整數。
fn validate_lifespan(value: Option<i64>) -> Result<Option<i64>, ApiError> {
    match value {
        Some(years) if years < 0 => {
            Err(ApiError::validation("年限須為非負整數").field("lifespan_years"))
        }
        other => Ok(other),
    }
}

/// 屆齡：`purchase_date + lifespan_years < 今天`；缺購置日期或年限即無從判斷。
fn is_expired(purchase_date: Option<&str>, lifespan_years: Option<i64>, today: NaiveDate) -> bool {
    let (Some(raw), Some(years)) = (purchase_date, lifespan_years) else {
        return false;
    };
    if years < 0 {
        return false;
    }
    let Ok(purchase) = NaiveDate::parse_from_str(raw, "%Y-%m-%d") else {
        return false;
    };
    add_years(purchase, years).is_some_and(|due| due < today)
}

/// 加 `years` 年；2/29 遇非閏年退至 2/28，超出日期範圍回傳 `None`。
fn add_years(date: NaiveDate, years: i64) -> Option<NaiveDate> {
    let year = i32::try_from(i64::from(date.year()) + years).ok()?;
    (1..=date.day())
        .rev()
        .find_map(|day| NaiveDate::from_ymd_opt(year, date.month(), day))
}

/// 讓 `serde` 區分「欄位未提供」（`None`）與「顯式 null」（`Some(None)`）。
pub(crate) fn double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("有效日期")
    }

    #[test]
    fn expired_needs_both_purchase_date_and_lifespan() {
        let today = date(2026, 10, 4);
        assert!(!is_expired(None, Some(5), today));
        assert!(!is_expired(Some("2000-01-01"), None, today));
        assert!(is_expired(Some("2000-01-01"), Some(5), today));
    }

    #[test]
    fn exactly_due_today_is_not_expired() {
        assert!(!is_expired(Some("2020-01-01"), Some(5), date(2025, 1, 1)));
        assert!(is_expired(Some("2020-01-01"), Some(5), date(2025, 1, 2)));
    }

    #[test]
    fn leap_day_falls_back_to_february_28() {
        assert_eq!(add_years(date(2020, 2, 29), 1), Some(date(2021, 2, 28)));
        assert_eq!(add_years(date(2020, 2, 29), 4), Some(date(2024, 2, 29)));
    }

    #[test]
    fn escape_like_treats_wildcards_literally() {
        assert_eq!(escape_like("50%_x"), "50\\%\\_x");
        assert_eq!(escape_like("a\\b"), "a\\\\b");
    }

    /// 建立測試資料庫並套用 migrations（比照 `backend/tests/` 整合測試）。
    async fn test_pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("建立記憶體資料庫");

        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("套用 migrations");

        pool
    }

    /// 測試用已驗證資產。
    fn valid_asset(description: &str) -> ValidAsset {
        ValidAsset {
            property_no: None,
            description: description.to_string(),
            location: "機房 A".to_string(),
            device_serial: None,
            brand: None,
            model: None,
            purchase_date: None,
            lifespan_years: None,
            note: None,
            tags: Vec::new(),
        }
    }

    #[tokio::test]
    async fn insert_asset_is_visible_in_same_transaction() {
        let pool = test_pool().await;
        let mut transaction = pool.begin().await.expect("建立交易");

        let id = insert_asset(&mut transaction, valid_asset("交易內資產"))
            .await
            .expect("新增資產");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM assets WHERE id = ?")
            .bind(id)
            .fetch_one(&mut *transaction)
            .await
            .expect("同交易讀取資產");

        assert_eq!(count, 1, "原語建立後同交易可見");
    }

    #[tokio::test]
    async fn insert_asset_rollback_leaves_nothing() {
        let pool = test_pool().await;
        let mut transaction = pool.begin().await.expect("建立交易");

        insert_asset(&mut transaction, valid_asset("回滾資產"))
            .await
            .expect("新增資產");
        transaction.rollback().await.expect("回滾交易");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM assets")
            .fetch_one(&pool)
            .await
            .expect("回滾後讀取資產");

        assert_eq!(count, 0, "回滾後不留下資料");
    }
}
