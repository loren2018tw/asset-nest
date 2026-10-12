//! 資產報廢（disposal）領域模組：報廢日期驗證、搬移、復原與報廢清單。
//!
//! 詞彙依 `GLOSSARY.md`「報廢詞彙」；規則見 `.scratch/asset-disposal/spec.md` §2、§3
//! 與 `docs/adr/0026`。報廢＝將資產整列複製進 `disposed_assets`（＋報廢日期）後自
//! `assets` 刪除（FK CASCADE 連動刪除介面、指派與借還）；復原＝搬回 `assets`
//! （原 id、原 `created_at`）並刪除報廢列，介面與 IP 指派不回復。

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, QueryBuilder, Sqlite, SqlitePool};

use crate::api::ApiError;
use crate::assets::{Asset, escape_like, optional_text};

/// `disposed_assets` 資料表完整欄位清單。
const COLUMNS: &str = "id, asset_id, property_no, description, location, device_serial, brand, \
                       model, purchase_date, lifespan_years, note, tags, disposal_date, \
                       created_at, updated_at";

/// 搬移時自 `assets` 讀取的欄位清單（同 [`DisposedAssetRow`] 去掉 `asset_id`／`disposal_date`）。
const ASSET_COLUMNS: &str = "id, property_no, description, location, device_serial, brand, \
                             model, purchase_date, lifespan_years, note, tags, created_at, \
                             updated_at";

/// 報廢清單關鍵字比對的七個文字欄位（見 spec §3）。
const SEARCH_COLUMNS: [&str; 7] = [
    "property_no",
    "description",
    "location",
    "device_serial",
    "brand",
    "model",
    "note",
];

/// `disposed_assets` 資料表列；`tags` 為 JSON 陣列字串（DB 格式）。
#[derive(Debug, FromRow)]
struct DisposedAssetRow {
    id: i64,
    asset_id: i64,
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
    disposal_date: String,
    created_at: String,
    updated_at: String,
}

/// 搬移前自 `assets` 讀出的整列（欄位同 [`DisposedAssetRow`]，不含報廢欄位）。
#[derive(Debug, FromRow)]
struct AssetSnapshotRow {
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

/// API 回傳的報廢資產。
#[derive(Debug, Serialize)]
pub struct DisposedAsset {
    /// 報廢列自身的 id（復原以此為鍵）。
    pub id: i64,
    /// 原資產 id；復原時沿用。
    pub asset_id: i64,
    pub property_no: Option<String>,
    pub description: String,
    pub location: String,
    pub device_serial: Option<String>,
    pub brand: Option<String>,
    pub model: Option<String>,
    pub purchase_date: Option<String>,
    pub lifespan_years: Option<i64>,
    pub note: Option<String>,
    /// 標籤：多值自由文字。
    pub tags: Vec<String>,
    /// 報廢日期：`YYYY-MM-DD`。
    pub disposal_date: String,
    /// 原資產的建立時間（搬移沿用）。
    pub created_at: String,
    /// 原資產的更新時間（搬移沿用）。
    pub updated_at: String,
}

/// 報廢的輸入；缺漏的 `disposal_date` 由 `validate` 回報。
#[derive(Debug, Default, Deserialize)]
pub struct DisposeInput {
    pub disposal_date: Option<String>,
}

/// 已驗證的報廢內容。
#[derive(Debug)]
pub struct ValidDispose {
    disposal_date: String,
}

impl DisposeInput {
    /// 驗證報廢日期：必填、嚴格 `YYYY-MM-DD`、不可為未來（今天可；見 spec §3）。
    pub fn validate(self, today: NaiveDate) -> Result<ValidDispose, ApiError> {
        let text = optional_text(self.disposal_date)
            .ok_or_else(|| ApiError::validation("報廢日期為必填").field("disposal_date"))?;
        let date = parse_disposal_date(&text)?;
        if date > today {
            return Err(ApiError::validation("報廢日期不可為未來日期").field("disposal_date"));
        }
        Ok(ValidDispose {
            disposal_date: text,
        })
    }
}

/// 報廢：單一交易內將 `assets` 整列複製進 `disposed_assets` 後刪除原列（見 spec §3）。
///
/// 資產不存在回傳 `Ok(None)`（交易回滾、不留變更）；原列的介面、指派與借還紀錄
/// 由 FK CASCADE 連動刪除（見 spec §2 不變量）。
pub async fn dispose(
    pool: &SqlitePool,
    asset_id: i64,
    valid: ValidDispose,
) -> sqlx::Result<Option<DisposedAsset>> {
    let mut transaction = pool.begin().await?;

    let asset = sqlx::query_as::<_, AssetSnapshotRow>(&format!(
        "SELECT {ASSET_COLUMNS} FROM assets WHERE id = ?"
    ))
    .bind(asset_id)
    .fetch_optional(&mut *transaction)
    .await?;
    let Some(asset) = asset else {
        return Ok(None);
    };

    let result = sqlx::query(
        "INSERT INTO disposed_assets
             (asset_id, property_no, description, location, device_serial, brand, model,
              purchase_date, lifespan_years, note, tags, disposal_date, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(asset.id)
    .bind(asset.property_no)
    .bind(asset.description)
    .bind(asset.location)
    .bind(asset.device_serial)
    .bind(asset.brand)
    .bind(asset.model)
    .bind(asset.purchase_date)
    .bind(asset.lifespan_years)
    .bind(asset.note)
    .bind(asset.tags)
    .bind(valid.disposal_date)
    .bind(asset.created_at)
    .bind(asset.updated_at)
    .execute(&mut *transaction)
    .await?;

    sqlx::query("DELETE FROM assets WHERE id = ?")
        .bind(asset.id)
        .execute(&mut *transaction)
        .await?;

    let row: DisposedAssetRow = sqlx::query_as(&format!(
        "SELECT {COLUMNS} FROM disposed_assets WHERE id = ?"
    ))
    .bind(result.last_insert_rowid())
    .fetch_one(&mut *transaction)
    .await?;

    transaction.commit().await?;
    Ok(Some(row.into_disposed()))
}

/// 報廢清單：關鍵字以 LIKE 子字串比對七個文字欄位（見 [`SEARCH_COLUMNS`]）；
/// 排序 `disposal_date DESC, id DESC`；分頁（`page` 由 1 起）；
/// 回傳（當頁項目、符合總數）（見 spec §3）。
pub async fn list(
    pool: &SqlitePool,
    q: Option<&str>,
    page: i64,
    per_page: i64,
) -> sqlx::Result<(Vec<DisposedAsset>, i64)> {
    let mut count = QueryBuilder::new("SELECT COUNT(*) FROM disposed_assets WHERE 1 = 1");
    push_keyword(&mut count, q);
    let total = count.build_query_scalar::<i64>().fetch_one(pool).await?;

    let mut query = QueryBuilder::new(format!("SELECT {COLUMNS} FROM disposed_assets WHERE 1 = 1"));
    push_keyword(&mut query, q);
    query
        .push(" ORDER BY disposal_date DESC, id DESC LIMIT ")
        .push_bind(per_page)
        .push(" OFFSET ")
        .push_bind((page - 1).max(0) * per_page);
    let rows: Vec<DisposedAssetRow> = query.build_query_as().fetch_all(pool).await?;

    Ok((
        rows.into_iter()
            .map(DisposedAssetRow::into_disposed)
            .collect(),
        total,
    ))
}

/// 復原：單一交易內將報廢列搬回 `assets`（原 id、原 `created_at`，`updated_at` 為當下）
/// 後刪除報廢列；提交後以 [`crate::assets::get`] 讀回（見 spec §3）。
///
/// 報廢列不存在回傳 `Ok(None)`；介面與 IP 指派不回復（見 spec §2 不變量）。
pub async fn restore(pool: &SqlitePool, id: i64) -> sqlx::Result<Option<Asset>> {
    let mut transaction = pool.begin().await?;

    let row = sqlx::query_as::<_, DisposedAssetRow>(&format!(
        "SELECT {COLUMNS} FROM disposed_assets WHERE id = ?"
    ))
    .bind(id)
    .fetch_optional(&mut *transaction)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };

    sqlx::query(
        "INSERT INTO assets
             (id, property_no, description, location, device_serial, brand, model,
              purchase_date, lifespan_years, note, tags, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))",
    )
    .bind(row.asset_id)
    .bind(row.property_no)
    .bind(row.description)
    .bind(row.location)
    .bind(row.device_serial)
    .bind(row.brand)
    .bind(row.model)
    .bind(row.purchase_date)
    .bind(row.lifespan_years)
    .bind(row.note)
    .bind(row.tags)
    .bind(row.created_at)
    .execute(&mut *transaction)
    .await?;

    sqlx::query("DELETE FROM disposed_assets WHERE id = ?")
        .bind(row.id)
        .execute(&mut *transaction)
        .await?;

    transaction.commit().await?;

    crate::assets::get(pool, row.asset_id).await
}

impl DisposedAssetRow {
    fn into_disposed(self) -> DisposedAsset {
        DisposedAsset {
            id: self.id,
            asset_id: self.asset_id,
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
            disposal_date: self.disposal_date,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// 在查詢後附加關鍵字條件：`q` 以 LIKE 子字串比對七個文字欄位；
/// `%`／`_` 轉義為字面（比照 `assets::escape_like`；見 spec §3）。
fn push_keyword(query: &mut QueryBuilder<'_, Sqlite>, q: Option<&str>) {
    let Some(q) = q.map(str::trim).filter(|q| !q.is_empty()) else {
        return;
    };
    let pattern = format!("%{}%", escape_like(q));
    query.push(" AND (");
    for (index, column) in SEARCH_COLUMNS.iter().enumerate() {
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

/// 嚴格 `YYYY-MM-DD`：4-2-2 位數字＋`NaiveDate` 日曆檢查（比照
/// `lendings::validate_due_date`；chrono 對位數寬容，`1-2-3` 須視為格式錯）。
fn parse_disposal_date(text: &str) -> Result<NaiveDate, ApiError> {
    let invalid =
        || ApiError::validation("報廢日期須為有效日期（YYYY-MM-DD）").field("disposal_date");
    let well_formed = text.split('-').collect::<Vec<_>>().len() == 3
        && ["%Y", "%m", "%d"]
            .iter()
            .zip(text.split('-'))
            .all(|(format, part)| {
                let expected = if *format == "%Y" { 4 } else { 2 };
                part.len() == expected && part.bytes().all(|byte| byte.is_ascii_digit())
            });
    if !well_formed {
        return Err(invalid());
    }
    NaiveDate::parse_from_str(text, "%Y-%m-%d").map_err(|_| invalid())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("有效日期")
    }

    /// 建立測試資料庫並套用 migrations（比照 `assets.rs`／`lendings.rs`）。
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

    /// 測試用已驗證報廢內容。
    fn valid_dispose(disposal_date: &str) -> ValidDispose {
        ValidDispose {
            disposal_date: disposal_date.to_string(),
        }
    }

    /// 直接入庫一筆全欄位測試資產（時間欄位固定，供搬移比對），回傳 id。
    async fn create_full_asset(pool: &SqlitePool) -> i64 {
        let result = sqlx::query(
            "INSERT INTO assets
                 (property_no, description, location, device_serial, brand, model,
                  purchase_date, lifespan_years, note, tags, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind("P-001")
        .bind("測試筆電")
        .bind("機房 A")
        .bind("SN-123")
        .bind("Dell")
        .bind("Latitude 5540")
        .bind("2024-01-15")
        .bind(5)
        .bind("測試備註")
        .bind(r#"["標籤A","標籤B"]"#)
        .bind("2020-01-01T00:00:00Z")
        .bind("2020-02-02T00:00:00Z")
        .execute(pool)
        .await
        .expect("新增資產");
        result.last_insert_rowid()
    }

    /// 直接入庫一筆最小測試資產，回傳 id（報廢清單測試用）。
    async fn create_asset(pool: &SqlitePool, property_no: &str, description: &str) -> i64 {
        sqlx::query("INSERT INTO assets (property_no, description, location) VALUES (?, ?, ?)")
            .bind(property_no)
            .bind(description)
            .bind("機房 A")
            .execute(pool)
            .await
            .expect("新增資產")
            .last_insert_rowid()
    }

    /// 為資產建立介面、指派與已歸還借還紀錄（驗證 FK CASCADE），回傳介面 id。
    async fn add_linkage(pool: &SqlitePool, asset_id: i64) -> i64 {
        let interface_id = sqlx::query("INSERT INTO interfaces (asset_id, mac) VALUES (?, ?)")
            .bind(asset_id)
            .bind("aa:bb:cc:dd:ee:ff")
            .execute(pool)
            .await
            .expect("新增介面")
            .last_insert_rowid();

        let subnet_id = sqlx::query("INSERT INTO subnets (cidr) VALUES (?)")
            .bind("192.168.1.0/24")
            .execute(pool)
            .await
            .expect("新增網段")
            .last_insert_rowid();

        sqlx::query(
            "INSERT INTO ip_assignments (subnet_id, address, interface_id, purpose)
             VALUES (?, ?, ?, 'static')",
        )
        .bind(subnet_id)
        .bind("192.168.1.10")
        .bind(interface_id)
        .execute(pool)
        .await
        .expect("新增指派");

        sqlx::query(
            "INSERT INTO lendings (asset_id, borrower, lent_at, returned_at)
             VALUES (?, ?, '2026-10-01T00:00:00Z', '2026-10-02T00:00:00Z')",
        )
        .bind(asset_id)
        .bind("王小明")
        .execute(pool)
        .await
        .expect("新增借還紀錄");

        interface_id
    }

    #[test]
    fn disposal_date_is_required_after_trim() {
        let today = date(2026, 10, 12);
        for input in [
            DisposeInput::default(),
            DisposeInput {
                disposal_date: Some("   ".to_string()),
            },
        ] {
            let error = input.validate(today).expect_err("缺漏報廢日期");
            assert_eq!(error.field_name(), Some("disposal_date"));
            assert_eq!(error.message(), "報廢日期為必填");
        }
    }

    #[test]
    fn disposal_date_must_be_strict_yyyy_mm_dd() {
        let today = date(2026, 10, 12);
        for bad in ["2026/1/1", "1-2-3", "2026-13-01", "不是日期"] {
            let input = DisposeInput {
                disposal_date: Some(bad.to_string()),
            };
            let error = input.validate(today).expect_err("格式錯誤的報廢日期");
            assert_eq!(error.field_name(), Some("disposal_date"), "{bad}");
            assert_eq!(
                error.message(),
                "報廢日期須為有效日期（YYYY-MM-DD）",
                "{bad}"
            );
        }
    }

    #[test]
    fn disposal_date_must_not_be_in_future() {
        let today = date(2026, 10, 12);

        let error = DisposeInput {
            disposal_date: Some("2026-10-13".to_string()),
        }
        .validate(today)
        .expect_err("未來日期");
        assert_eq!(error.field_name(), Some("disposal_date"));
        assert_eq!(error.message(), "報廢日期不可為未來日期");

        // 今天可、可回填過去。
        for ok in ["2026-10-12", "2026-10-11"] {
            let valid = DisposeInput {
                disposal_date: Some(ok.to_string()),
            }
            .validate(today)
            .expect("合法報廢日期");
            assert_eq!(valid.disposal_date, ok);
        }
    }

    #[tokio::test]
    async fn dispose_moves_full_asset_and_clears_linkage() {
        let pool = test_pool().await;
        let asset_id = create_full_asset(&pool).await;
        let interface_id = add_linkage(&pool, asset_id).await;

        let disposed = dispose(&pool, asset_id, valid_dispose("2026-10-12"))
            .await
            .expect("報廢")
            .expect("資產存在");

        assert_eq!(disposed.asset_id, asset_id);
        assert_eq!(disposed.property_no.as_deref(), Some("P-001"));
        assert_eq!(disposed.description, "測試筆電");
        assert_eq!(disposed.location, "機房 A");
        assert_eq!(disposed.device_serial.as_deref(), Some("SN-123"));
        assert_eq!(disposed.brand.as_deref(), Some("Dell"));
        assert_eq!(disposed.model.as_deref(), Some("Latitude 5540"));
        assert_eq!(disposed.purchase_date.as_deref(), Some("2024-01-15"));
        assert_eq!(disposed.lifespan_years, Some(5));
        assert_eq!(disposed.note.as_deref(), Some("測試備註"));
        assert_eq!(
            disposed.tags,
            vec!["標籤A".to_string(), "標籤B".to_string()]
        );
        assert_eq!(disposed.disposal_date, "2026-10-12");
        // 原 `created_at`／`updated_at` 整列複製。
        assert_eq!(disposed.created_at, "2020-01-01T00:00:00Z");
        assert_eq!(disposed.updated_at, "2020-02-02T00:00:00Z");

        // 原列自 assets 消失；介面、指派與借還由 CASCADE 連動刪除。
        let assets: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM assets WHERE id = ?")
            .bind(asset_id)
            .fetch_one(&pool)
            .await
            .expect("讀取資產列數");
        assert_eq!(assets, 0, "資產列應刪除");
        let interfaces: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM interfaces WHERE id = ?")
            .bind(interface_id)
            .fetch_one(&pool)
            .await
            .expect("讀取介面列數");
        assert_eq!(interfaces, 0, "介面應連動刪除");
        let assignments: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM ip_assignments WHERE interface_id = ?")
                .bind(interface_id)
                .fetch_one(&pool)
                .await
                .expect("讀取指派列數");
        assert_eq!(assignments, 0, "指派應連動刪除");
        let lendings: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM lendings WHERE asset_id = ?")
            .bind(asset_id)
            .fetch_one(&pool)
            .await
            .expect("讀取借還列數");
        assert_eq!(lendings, 0, "借還紀錄應連動刪除");
    }

    #[tokio::test]
    async fn dispose_missing_asset_returns_none() {
        let pool = test_pool().await;

        let result = dispose(&pool, 9999, valid_dispose("2026-10-12"))
            .await
            .expect("報廢不存在資產");
        assert!(result.is_none());

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM disposed_assets")
            .fetch_one(&pool)
            .await
            .expect("讀取報廢列數");
        assert_eq!(count, 0, "不存在的資產不應產生報廢列");
    }

    #[tokio::test]
    async fn restore_returns_asset_with_original_id_and_created_at() {
        let pool = test_pool().await;
        let asset_id = create_full_asset(&pool).await;
        add_linkage(&pool, asset_id).await;
        let disposed = dispose(&pool, asset_id, valid_dispose("2026-10-12"))
            .await
            .expect("報廢")
            .expect("資產存在");

        let asset = restore(&pool, disposed.id)
            .await
            .expect("復原")
            .expect("報廢列存在");

        assert_eq!(asset.id, asset_id, "復原沿用原 id");
        assert_eq!(asset.created_at, "2020-01-01T00:00:00Z", "created_at 保留");
        assert_ne!(
            asset.updated_at, "2020-02-02T00:00:00Z",
            "updated_at 更新為當下"
        );
        assert!(asset.updated_at.ends_with('Z'), "updated_at 為 UTC ISO8601");
        assert_eq!(asset.tags, vec!["標籤A".to_string(), "標籤B".to_string()]);
        assert_eq!(asset.description, "測試筆電");
        assert_eq!(asset.property_no.as_deref(), Some("P-001"));

        // 報廢列消失；介面與指派不回復。
        let disposed_rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM disposed_assets")
            .fetch_one(&pool)
            .await
            .expect("讀取報廢列數");
        assert_eq!(disposed_rows, 0, "報廢列應刪除");
        let interfaces: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM interfaces")
            .fetch_one(&pool)
            .await
            .expect("讀取介面列數");
        assert_eq!(interfaces, 0, "介面不回復");
        let assignments: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ip_assignments")
            .fetch_one(&pool)
            .await
            .expect("讀取指派列數");
        assert_eq!(assignments, 0, "指派不回復");
    }

    #[tokio::test]
    async fn restore_missing_disposed_returns_none() {
        let pool = test_pool().await;

        let result = restore(&pool, 9999).await.expect("復原不存在報廢列");
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn list_searches_columns_and_treats_wildcards_literally() {
        let pool = test_pool().await;

        let percent = create_asset(&pool, "P-100", "50% 折扣機").await;
        let plain = create_asset(&pool, "P-200", "一般筆電").await;
        let underscore = create_asset(&pool, "P-300", "A_B 測試機").await;
        for (asset_id, disposal_date) in [
            (percent, "2026-10-01"),
            (plain, "2026-10-02"),
            (underscore, "2026-10-03"),
        ] {
            dispose(&pool, asset_id, valid_dispose(disposal_date))
                .await
                .expect("報廢")
                .expect("資產存在");
        }

        // 描述子字串命中。
        let (items, total) = list(&pool, Some("筆電"), 1, 10).await.expect("搜尋描述");
        assert_eq!(total, 1);
        assert_eq!(items[0].asset_id, plain);

        // property_no 亦在比對欄位內。
        let (items, total) = list(&pool, Some("P-100"), 1, 10)
            .await
            .expect("搜尋財產編號");
        assert_eq!(total, 1);
        assert_eq!(items[0].asset_id, percent);

        // `%`／`_` 視為字面，不當萬用字元（否則會命中全部）。
        let (items, total) = list(&pool, Some("%"), 1, 10).await.expect("搜尋 %");
        assert_eq!(total, 1);
        assert_eq!(items[0].asset_id, percent);
        let (items, total) = list(&pool, Some("_"), 1, 10).await.expect("搜尋 _");
        assert_eq!(total, 1);
        assert_eq!(items[0].asset_id, underscore);

        // 無命中。
        let (items, total) = list(&pool, Some("不存在"), 1, 10)
            .await
            .expect("搜尋無命中");
        assert_eq!(total, 0);
        assert!(items.is_empty());
    }

    #[tokio::test]
    async fn list_paginates_and_orders_by_disposal_date_desc() {
        let pool = test_pool().await;

        let oldest = create_asset(&pool, "P-001", "最舊").await;
        let middle = create_asset(&pool, "P-002", "中間").await;
        let newest = create_asset(&pool, "P-003", "最新").await;
        for (asset_id, disposal_date) in [
            (oldest, "2026-10-01"),
            (middle, "2026-10-03"),
            (newest, "2026-10-03"),
        ] {
            dispose(&pool, asset_id, valid_dispose(disposal_date))
                .await
                .expect("報廢")
                .expect("資產存在");
        }

        // disposal_date 新→舊；同日期以 id 新→舊（newest 晚於 middle 建立）。
        let (page1, total) = list(&pool, None, 1, 2).await.expect("第一頁");
        assert_eq!(total, 3);
        assert_eq!(page1.len(), 2);
        assert_eq!(page1[0].asset_id, newest);
        assert_eq!(page1[1].asset_id, middle);

        let (page2, total) = list(&pool, None, 2, 2).await.expect("第二頁");
        assert_eq!(total, 3);
        assert_eq!(page2.len(), 1);
        assert_eq!(page2[0].asset_id, oldest);

        let (beyond, total) = list(&pool, None, 3, 2).await.expect("超出範圍頁");
        assert_eq!(total, 3);
        assert!(beyond.is_empty());
    }
}
