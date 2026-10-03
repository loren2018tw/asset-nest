//! 資產（Asset）領域模組：欄位驗證、屆齡判斷與資料庫存取。
//!
//! 詞彙依 `CONTEXT.md`；規則見 `.scratch/asset-ip-management/spec.md` §2.1、§3。

use chrono::{Datelike, Local, NaiveDate};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, QueryBuilder, Sqlite, SqlitePool};

use crate::api::ApiError;

/// `assets` 資料表完整欄位清單。
const COLUMNS: &str = "id, property_no, description, location, device_serial, brand, model, \
                       purchase_date, lifespan_years, note, created_at, updated_at";

/// `assets` 資料表列。
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
        })
    }
}

/// 清單的搜尋、篩選與分頁條件（皆為伺服器端，見 spec §2.1）。
#[derive(Debug, Default)]
pub struct AssetFilter {
    /// 關鍵字：財產編號／描述／設備序號／廠牌／型號／備註（子字串、不分大小寫）。
    pub q: Option<String>,
    /// 位置：不分大小寫完全符合。
    pub location: Option<String>,
    /// 廠牌：不分大小寫完全符合。
    pub brand: Option<String>,
    /// 設備序號：不分大小寫完全符合（供重複提示與精確查找）。
    pub device_serial: Option<String>,
    pub page: i64,
    pub per_page: i64,
}

/// 搜尋／篩選後的分頁查詢；回傳（當頁資產、符合總數）。
pub async fn list(pool: &SqlitePool, filter: &AssetFilter) -> sqlx::Result<(Vec<Asset>, i64)> {
    let mut count = QueryBuilder::new("SELECT COUNT(*) FROM assets WHERE 1 = 1");
    push_filters(&mut count, filter);
    let total = count.build_query_scalar::<i64>().fetch_one(pool).await?;

    let mut query = QueryBuilder::new(format!("SELECT {COLUMNS} FROM assets WHERE 1 = 1"));
    push_filters(&mut query, filter);
    // 預設排序：描述（升冪）；以 id 作為穩定排序的決勝鍵（見 spec §6）。
    query.push(" ORDER BY description COLLATE NOCASE ASC, id ASC");
    query.push(" LIMIT ").push_bind(filter.per_page);
    query
        .push(" OFFSET ")
        .push_bind((filter.page - 1).max(0) * filter.per_page);
    let rows: Vec<AssetRow> = query.build_query_as().fetch_all(pool).await?;

    Ok((into_assets(rows), total))
}

/// 讀取單一資產；不存在回傳 `None`。
pub async fn get(pool: &SqlitePool, id: i64) -> sqlx::Result<Option<Asset>> {
    Ok(fetch_row(pool, id)
        .await?
        .map(|row| row.into_asset(today())))
}

/// 新增資產並回傳入庫後的內容。
pub async fn create(pool: &SqlitePool, asset: ValidAsset) -> sqlx::Result<Asset> {
    let result = sqlx::query(
        "INSERT INTO assets
             (property_no, description, location, device_serial, brand, model,
              purchase_date, lifespan_years, note)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
    .execute(pool)
    .await?;

    let id = result.last_insert_rowid();
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

/// 刪除資產；回傳是否確實刪除（後續票補上連動刪除與影響數量，見票 08）。
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
            expired,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
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
}

/// 轉義 `LIKE` 的萬用字元，讓使用者輸入的 `%`、`_` 被視為字面字元。
fn escape_like(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

/// 去除前後空白；空字串視為未填。
fn optional_text(value: Option<String>) -> Option<String> {
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
fn double_option<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
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
}
