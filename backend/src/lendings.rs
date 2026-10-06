//! 借還（Lending）領域模組：欄位驗證、逾期判定與資料庫存取。
//!
//! 詞彙依 `GLOSSARY.md`「借還詞彙」；規則見 `.scratch/asset-lending/spec.md` §2、§3。
//! 出借中＝存在未歸還（`returned_at IS NULL`）紀錄；逾期＝未歸還且預計歸還日已過。

use std::collections::HashMap;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, QueryBuilder, SqlitePool};

use crate::api::ApiError;
use crate::assets::optional_text;

/// `lendings` 資料表完整欄位清單。
const COLUMNS: &str = "id, asset_id, borrower, lent_at, due_at, note, returned_at";

/// 列表用的欄位清單：以別名 `l` 附資產編號與描述（JOIN `assets`，避免 N+1）。
const JOIN_COLUMNS: &str = "l.id, l.asset_id, l.borrower, l.lent_at, l.due_at, l.note, \
                            l.returned_at, a.property_no, a.description";

/// `lendings` 資料表列；`due_at`／`returned_at` 可能為 NULL。
#[derive(Debug, FromRow)]
struct LendingRow {
    id: i64,
    asset_id: i64,
    borrower: String,
    lent_at: String,
    due_at: Option<String>,
    note: Option<String>,
    returned_at: Option<String>,
}

/// 列表用的列：借還紀錄附資產編號與描述。
#[derive(Debug, FromRow)]
struct LendingJoinRow {
    id: i64,
    asset_id: i64,
    borrower: String,
    lent_at: String,
    due_at: Option<String>,
    note: Option<String>,
    returned_at: Option<String>,
    property_no: Option<String>,
    description: String,
}

/// API 回傳的借出紀錄（含後端計算的逾期旗標）。
#[derive(Debug, Clone, Serialize)]
pub struct Lending {
    pub id: i64,
    pub asset_id: i64,
    pub borrower: String,
    /// 借出時間：伺服器當下（UTC ISO8601），不開放修改（見 spec §2）。
    pub lent_at: String,
    /// 預計歸還日：`YYYY-MM-DD` 日期字串，選填。
    pub due_at: Option<String>,
    /// 備註：自由文字，選填。
    pub note: Option<String>,
    /// 歸還時間：UTC ISO8601；`None` 即出借中。
    pub returned_at: Option<String>,
    /// 逾期：未歸還且預計歸還日早於今天（見 spec §2）。
    pub overdue: bool,
}

/// 列表用的借出紀錄：附加資產編號與描述（見 spec §3）。
#[derive(Debug, Clone, Serialize)]
pub struct LendingWithAsset {
    pub property_no: Option<String>,
    pub description: String,
    #[serde(flatten)]
    pub lending: Lending,
}

/// 資產清單列附帶的出借中摘要：`id`／借用人／借出時間／預計歸還日
/// （見 spec §4）。
///
/// 票 01 未定義，於票 02 補於本模組，供 `api::assets::AssetListRow` 使用；
/// 不含歸還時間、備註與逾期旗標（詳情見 [`Lending`]）。
#[derive(Debug, Clone, Serialize)]
pub struct LendingBrief {
    pub id: i64,
    pub borrower: String,
    pub lent_at: String,
    pub due_at: Option<String>,
}

/// 新增借出的輸入；所有欄位先收為 `Option`，缺漏必填欄位由 `validate` 回報。
#[derive(Debug, Default, Deserialize)]
pub struct LendingInput {
    pub borrower: Option<String>,
    pub due_at: Option<String>,
    pub note: Option<String>,
}

/// 已驗證的借出內容。
#[derive(Debug)]
pub struct ValidLending {
    borrower: String,
    due_at: Option<String>,
    note: Option<String>,
}

/// 歸還操作的結果（見 spec §2 不變量）。
#[derive(Debug)]
pub enum ReturnOutcome {
    /// 歸還成功，附更新後的紀錄（逾期旗標已重算）。
    Returned(Lending),
    /// 紀錄不存在（API 層回 404）。
    NotFound,
    /// 已歸還，不可再次歸還（API 層回 409）。
    AlreadyReturned,
}

impl LendingInput {
    /// 驗證新增欄位：借用人必填（trim 後非空）；預計歸還日若提供須為
    /// `YYYY-MM-DD`（見 spec §3）。
    pub fn validate(self) -> Result<ValidLending, ApiError> {
        Ok(ValidLending {
            borrower: require_text("borrower", "借用人", self.borrower)?,
            due_at: validate_due_date(self.due_at)?,
            note: optional_text(self.note),
        })
    }
}

/// 該資產是否已有未歸還（出借中）的借出紀錄（見 spec §2 不變量）。
pub async fn has_open_lending(pool: &SqlitePool, asset_id: i64) -> sqlx::Result<bool> {
    let open: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM lendings WHERE asset_id = ? AND returned_at IS NULL",
    )
    .bind(asset_id)
    .fetch_one(pool)
    .await?;
    Ok(open > 0)
}

/// 出借中清單：`returned_at IS NULL`，依借出時間倒序（見 spec §3）。
pub async fn list_open(pool: &SqlitePool, today: NaiveDate) -> sqlx::Result<Vec<LendingWithAsset>> {
    let rows: Vec<LendingJoinRow> = sqlx::query_as(&format!(
        "SELECT {JOIN_COLUMNS}
           FROM lendings l
           JOIN assets a ON a.id = l.asset_id
          WHERE l.returned_at IS NULL
          ORDER BY l.lent_at DESC, l.id DESC"
    ))
    .fetch_all(pool)
    .await?;

    Ok(into_with_assets(rows, today))
}

/// 已歸還紀錄：`returned_at IS NOT NULL`，依借出時間倒序、分頁（`page` 由 1 起）；
/// 回傳（當頁紀錄、符合總數）（見 spec §3）。
pub async fn list_returned(
    pool: &SqlitePool,
    page: i64,
    per_page: i64,
    today: NaiveDate,
) -> sqlx::Result<(Vec<LendingWithAsset>, i64)> {
    let total: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM lendings WHERE returned_at IS NOT NULL")
            .fetch_one(pool)
            .await?;

    let rows: Vec<LendingJoinRow> = sqlx::query_as(&format!(
        "SELECT {JOIN_COLUMNS}
           FROM lendings l
           JOIN assets a ON a.id = l.asset_id
          WHERE l.returned_at IS NOT NULL
          ORDER BY l.lent_at DESC, l.id DESC
          LIMIT ? OFFSET ?"
    ))
    .bind(per_page)
    .bind((page - 1).max(0) * per_page)
    .fetch_all(pool)
    .await?;

    Ok((into_with_assets(rows, today), total))
}

/// 借用人建議值：既有借用人去重（不分大小寫），依最近借出時間倒序；空白除外。
///
/// 以視窗函式取每組（NOCASE）最近一筆借出的原文（同值不同大小寫保留最近
/// 使用的寫法），並以各組最大的 `lent_at` 排序（同秒以最大 id 決勝）。
pub async fn list_borrowers(pool: &SqlitePool) -> sqlx::Result<Vec<String>> {
    sqlx::query_scalar(
        "SELECT borrower
           FROM (
               SELECT borrower,
                      ROW_NUMBER() OVER (
                          PARTITION BY borrower COLLATE NOCASE
                          ORDER BY lent_at DESC, id DESC
                      ) AS rn,
                      MAX(lent_at) OVER (
                          PARTITION BY borrower COLLATE NOCASE
                      ) AS latest,
                      MAX(id) OVER (
                          PARTITION BY borrower COLLATE NOCASE
                      ) AS max_id
                 FROM lendings
                WHERE trim(borrower) <> ''
           )
          WHERE rn = 1
          ORDER BY latest DESC, max_id DESC",
    )
    .fetch_all(pool)
    .await
}

/// 批次讀取多資產的出借中摘要（單一查詢，避免逐資產 N+1；見 spec §4）。
///
/// 以 `LEFT JOIN`（`returned_at IS NULL`）載入：未出借（或僅有已歸還紀錄）的
/// 資產無對應列，呼叫端以 `None` 呈現。每資產至多一筆出借中（見 spec §2
/// 不變量），資料異常出現多列時以最後一列為準。
pub async fn open_briefs_for_assets(
    pool: &SqlitePool,
    asset_ids: &[i64],
) -> sqlx::Result<HashMap<i64, LendingBrief>> {
    if asset_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut query = QueryBuilder::new(
        "SELECT a.id AS asset_id, l.id, l.borrower, l.lent_at, l.due_at \
           FROM assets a \
           LEFT JOIN lendings l ON l.asset_id = a.id AND l.returned_at IS NULL \
          WHERE a.id IN (",
    );
    let mut separated = query.separated(", ");
    for asset_id in asset_ids {
        separated.push_bind(*asset_id);
    }
    separated.push_unseparated(")");

    let rows: Vec<(
        i64,
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
    )> = query.build_query_as().fetch_all(pool).await?;

    Ok(rows
        .into_iter()
        .filter_map(|(asset_id, id, borrower, lent_at, due_at)| {
            Some((
                asset_id,
                LendingBrief {
                    id: id?,
                    borrower: borrower?,
                    lent_at: lent_at?,
                    due_at,
                },
            ))
        })
        .collect())
}

/// 新增借出紀錄：`lent_at` 由伺服器以當下 UTC 產生（見 spec §2），回傳入庫後內容。
///
/// 「同一資產至多一筆未歸還」不在此檢查，由呼叫端先以 [`has_open_lending`]
/// 確認（見 spec §2 不變量）；資產不存在由外鍵約束阻擋。
pub async fn create(
    pool: &SqlitePool,
    asset_id: i64,
    lending: ValidLending,
    today: NaiveDate,
) -> sqlx::Result<Lending> {
    let result = sqlx::query(
        "INSERT INTO lendings (asset_id, borrower, lent_at, due_at, note)
         VALUES (?, ?, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), ?, ?)",
    )
    .bind(asset_id)
    .bind(lending.borrower)
    .bind(lending.due_at)
    .bind(lending.note)
    .execute(pool)
    .await?;

    fetch_row(pool, result.last_insert_rowid())
        .await?
        .map(|row| row.into_lending(today))
        .ok_or(sqlx::Error::RowNotFound)
}

/// 歸還：對未歸還的紀錄記 `returned_at`（當下 UTC），回傳歸還結果（見 spec §2）。
///
/// 以 `WHERE returned_at IS NULL` 守門，已歸還的紀錄不可再次歸還
/// （[`ReturnOutcome::AlreadyReturned`]，由 API 層回 409）。
pub async fn return_one(
    pool: &SqlitePool,
    id: i64,
    today: NaiveDate,
) -> sqlx::Result<ReturnOutcome> {
    let result = sqlx::query(
        "UPDATE lendings
            SET returned_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
          WHERE id = ? AND returned_at IS NULL",
    )
    .bind(id)
    .execute(pool)
    .await?;

    let Some(row) = fetch_row(pool, id).await? else {
        return Ok(ReturnOutcome::NotFound);
    };
    if result.rows_affected() == 0 {
        return Ok(ReturnOutcome::AlreadyReturned);
    }
    Ok(ReturnOutcome::Returned(row.into_lending(today)))
}

/// 讀取一列；不存在回傳 `None`。
async fn fetch_row(pool: &SqlitePool, id: i64) -> sqlx::Result<Option<LendingRow>> {
    sqlx::query_as::<_, LendingRow>(&format!("SELECT {COLUMNS} FROM lendings WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await
}

fn into_with_assets(rows: Vec<LendingJoinRow>, today: NaiveDate) -> Vec<LendingWithAsset> {
    rows.into_iter()
        .map(|row| row.into_with_asset(today))
        .collect()
}

impl LendingRow {
    fn into_lending(self, today: NaiveDate) -> Lending {
        Lending {
            overdue: is_overdue(self.returned_at.as_deref(), self.due_at.as_deref(), today),
            id: self.id,
            asset_id: self.asset_id,
            borrower: self.borrower,
            lent_at: self.lent_at,
            due_at: self.due_at,
            note: self.note,
            returned_at: self.returned_at,
        }
    }
}

impl LendingJoinRow {
    fn into_with_asset(self, today: NaiveDate) -> LendingWithAsset {
        LendingWithAsset {
            property_no: self.property_no,
            description: self.description,
            lending: Lending {
                overdue: is_overdue(self.returned_at.as_deref(), self.due_at.as_deref(), today),
                id: self.id,
                asset_id: self.asset_id,
                borrower: self.borrower,
                lent_at: self.lent_at,
                due_at: self.due_at,
                note: self.note,
                returned_at: self.returned_at,
            },
        }
    }
}

/// 必填文字；缺漏或空白回傳結構驗證錯誤（見 ADR-0006）。
fn require_text(
    field: &'static str,
    label: &str,
    value: Option<String>,
) -> Result<String, ApiError> {
    optional_text(value).ok_or_else(|| ApiError::validation(format!("{label}為必填")).field(field))
}

/// 預計歸還日：空白視為未填；格式須為 `YYYY-MM-DD`（比照購置日期）。
///
/// 票 01 明確要求 `1-2-3` 等短式寫法視為格式錯誤，而 chrono 的
/// `%Y-%m-%d` 解析對位數寬容（`1-2-3` 會被讀作西元 1 年），故先以
/// 4-2-2 位數字檢查後再交 `NaiveDate` 驗證日曆有效性（如月份不得為 13）。
fn validate_due_date(value: Option<String>) -> Result<Option<String>, ApiError> {
    let Some(text) = optional_text(value) else {
        return Ok(None);
    };
    let valid = text.split('-').collect::<Vec<_>>().len() == 3
        && ["%Y", "%m", "%d"]
            .iter()
            .zip(text.split('-'))
            .all(|(format, part)| {
                let expected = if *format == "%Y" { 4 } else { 2 };
                part.len() == expected && part.bytes().all(|byte| byte.is_ascii_digit())
            })
        && NaiveDate::parse_from_str(&text, "%Y-%m-%d").is_ok();
    if !valid {
        return Err(ApiError::validation("預計歸還日須為有效日期（YYYY-MM-DD）").field("due_at"));
    }
    Ok(Some(text))
}

/// 逾期：未歸還且預計歸還日存在且早於今天（見 spec §2）。
///
/// 比照 [`crate::assets::is_expired`]：`today` 由呼叫端注入；`lent_at`／
/// `returned_at` 等 UTC 時間欄位不參與判定；日期無法解析時視為未填。
fn is_overdue(returned_at: Option<&str>, due_at: Option<&str>, today: NaiveDate) -> bool {
    if returned_at.is_some() {
        return false;
    }
    let Some(raw) = due_at else {
        return false;
    };
    let Ok(due) = NaiveDate::parse_from_str(raw, "%Y-%m-%d") else {
        return false;
    };
    due < today
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::SqlitePool;

    fn date(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("有效日期")
    }

    /// 建立測試資料庫並套用 migrations（比照 `assets.rs`）。
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

    /// 測試用已驗證借出內容。
    fn valid_lending(borrower: &str) -> ValidLending {
        ValidLending {
            borrower: borrower.to_string(),
            due_at: None,
            note: None,
        }
    }

    /// 直接入庫一筆測試資產，回傳 id（借出紀錄須依附存在的資產）。
    async fn create_asset(pool: &SqlitePool, description: &str) -> i64 {
        let result = sqlx::query("INSERT INTO assets (description, location) VALUES (?, ?)")
            .bind(description)
            .bind("測試位置")
            .execute(pool)
            .await
            .expect("新增資產");
        result.last_insert_rowid()
    }

    #[test]
    fn borrower_is_required_after_trim() {
        let missing = LendingInput {
            borrower: None,
            ..Default::default()
        }
        .validate()
        .expect_err("缺漏借用人");
        assert_eq!(missing.field_name(), Some("borrower"));

        let blank = LendingInput {
            borrower: Some("   ".to_string()),
            ..Default::default()
        }
        .validate()
        .expect_err("空白借用人");
        assert_eq!(blank.field_name(), Some("borrower"));
    }

    #[test]
    fn borrower_is_trimmed() {
        let input = LendingInput {
            borrower: Some("  王小明  ".to_string()),
            ..Default::default()
        };
        let valid = input.validate().expect("合法借用人");
        assert_eq!(valid.borrower, "王小明");
    }

    #[test]
    fn due_at_must_be_valid_yyyy_mm_dd() {
        for bad in ["2026/1/1", "1-2-3", "2026-13-01", "不是日期"] {
            let input = LendingInput {
                borrower: Some("王小明".to_string()),
                due_at: Some(bad.to_string()),
                ..Default::default()
            };
            let error = input.validate().expect_err("格式錯誤的預計歸還日");
            assert_eq!(error.field_name(), Some("due_at"), "{bad}");
        }

        let input = LendingInput {
            borrower: Some("王小明".to_string()),
            due_at: Some("2026-10-08".to_string()),
            note: Some("  測試備註  ".to_string()),
        };
        let valid = input.validate().expect("合法輸入");
        assert_eq!(valid.borrower, "王小明");
        assert_eq!(valid.due_at.as_deref(), Some("2026-10-08"));
        assert_eq!(valid.note.as_deref(), Some("測試備註"));
    }

    #[test]
    fn empty_due_at_treated_as_unset() {
        let input = LendingInput {
            borrower: Some("王小明".to_string()),
            due_at: Some("".to_string()),
            ..Default::default()
        };
        let valid = input.validate().expect("空字串視為未填");
        assert_eq!(valid.due_at, None);
    }

    #[test]
    fn overdue_requires_open_due_date_in_past() {
        let today = date(2026, 10, 7);
        // 無預計歸還日 → 不逾期。
        assert!(!is_overdue(None, None, today));
        // 昨天到期 → 逾期；今天到期 → 不逾期。
        assert!(is_overdue(None, Some("2026-10-06"), today));
        assert!(!is_overdue(None, Some("2026-10-07"), today));
        // 已歸還 → 不逾期（即使預計歸還日已過）。
        assert!(!is_overdue(
            Some("2026-10-08T09:00:00Z"),
            Some("2026-10-01"),
            today
        ));
        // 無法解析的日期視為未填 → 不逾期。
        assert!(!is_overdue(None, Some("格式錯"), today));
    }

    #[tokio::test]
    async fn create_then_return_roundtrip() {
        let pool = test_pool().await;
        let today = date(2026, 10, 7);
        let asset_id = create_asset(&pool, "測試筆電").await;

        assert!(
            !has_open_lending(&pool, asset_id).await.expect("查詢出借中"),
            "未借出時不應有未歸還紀錄"
        );

        let lending = create(&pool, asset_id, valid_lending("王小明"), today)
            .await
            .expect("建立借出");
        assert_eq!(lending.asset_id, asset_id);
        assert_eq!(lending.borrower, "王小明");
        assert!(lending.returned_at.is_none());
        assert!(!lending.overdue);
        // 借出時間由伺服器產生（UTC ISO8601）。
        assert!(lending.lent_at.ends_with('Z'));

        assert!(
            has_open_lending(&pool, asset_id).await.expect("查詢出借中"),
            "建立後應有未歸還紀錄"
        );

        let outcome = return_one(&pool, lending.id, today).await.expect("歸還");
        let ReturnOutcome::Returned(returned) = outcome else {
            panic!("預期歸還成功");
        };
        assert!(returned.returned_at.is_some());
        assert!(!returned.overdue, "歸還後不應逾期");
        assert!(
            !has_open_lending(&pool, asset_id).await.expect("查詢出借中"),
            "歸還後不應有未歸還紀錄"
        );

        // 已歸還的紀錄不可再次歸還；不存在的紀錄回 NotFound。
        assert!(matches!(
            return_one(&pool, lending.id, today)
                .await
                .expect("再次歸還"),
            ReturnOutcome::AlreadyReturned
        ));
        assert!(matches!(
            return_one(&pool, 9999, today).await.expect("歸還不存在"),
            ReturnOutcome::NotFound
        ));
    }

    #[tokio::test]
    async fn open_lending_with_past_due_date_is_overdue() {
        let pool = test_pool().await;
        let asset_id = create_asset(&pool, "測試筆電").await;

        let lending = create(
            &pool,
            asset_id,
            ValidLending {
                borrower: "王小明".to_string(),
                due_at: Some("2026-10-06".to_string()),
                note: None,
            },
            date(2026, 10, 7),
        )
        .await
        .expect("建立借出");
        assert!(lending.overdue, "預計歸還日已過應逾期");

        let open = list_open(&pool, date(2026, 10, 7)).await.expect("列出借中");
        assert_eq!(open.len(), 1);
        assert!(open[0].lending.overdue);
        assert_eq!(open[0].property_no, None);
        assert_eq!(open[0].description, "測試筆電");

        let outcome = return_one(&pool, lending.id, date(2026, 10, 7))
            .await
            .expect("歸還");
        let ReturnOutcome::Returned(returned) = outcome else {
            panic!("預期歸還成功");
        };
        assert!(!returned.overdue, "歸還後不應逾期");
    }

    #[tokio::test]
    async fn list_returned_paginates_and_orders_by_lent_at_desc() {
        let pool = test_pool().await;
        let today = date(2026, 10, 7);

        for index in 0..7 {
            let asset_id = create_asset(&pool, &format!("資產 {index}")).await;
            let lending = create(&pool, asset_id, valid_lending("王小明"), today)
                .await
                .expect("建立借出");
            return_one(&pool, lending.id, today).await.expect("歸還");
        }
        // 一筆仍出借中，不列入已歸還清單。
        let open_asset = create_asset(&pool, "出借中資產").await;
        create(&pool, open_asset, valid_lending("陳大頭"), today)
            .await
            .expect("建立借出");

        let (page1, total) = list_returned(&pool, 1, 5, today).await.expect("第一頁");
        assert_eq!(total, 7);
        assert_eq!(page1.len(), 5);

        let (page2, total) = list_returned(&pool, 2, 5, today).await.expect("第二頁");
        assert_eq!(total, 7);
        assert_eq!(page2.len(), 2);

        let (beyond, total) = list_returned(&pool, 3, 5, today).await.expect("超出範圍頁");
        assert_eq!(total, 7);
        assert!(beyond.is_empty());

        // 借出時間倒序（同秒以 id 倒序決勝）：最新建立的在前。
        let first_ids: Vec<i64> = page1.iter().map(|row| row.lending.id).collect();
        let second_ids: Vec<i64> = page2.iter().map(|row| row.lending.id).collect();
        assert_eq!(first_ids, vec![7, 6, 5, 4, 3]);
        assert_eq!(second_ids, vec![2, 1]);
    }

    #[tokio::test]
    async fn list_borrowers_dedupes_and_orders_by_latest() {
        let pool = test_pool().await;
        let today = date(2026, 10, 7);

        // 王小明借出→歸還；陳大頭借出；Wang 借出→歸還→以小寫 wang 再借出。
        let asset_a = create_asset(&pool, "A").await;
        let asset_b = create_asset(&pool, "B").await;
        let asset_c = create_asset(&pool, "C").await;
        let asset_d = create_asset(&pool, "D").await;

        let first = create(&pool, asset_a, valid_lending("王小明"), today)
            .await
            .expect("建立借出");
        return_one(&pool, first.id, today).await.expect("歸還");
        create(&pool, asset_b, valid_lending("陳大頭"), today)
            .await
            .expect("建立借出");
        let wang = create(&pool, asset_c, valid_lending("Wang"), today)
            .await
            .expect("建立借出");
        return_one(&pool, wang.id, today).await.expect("歸還");
        create(&pool, asset_d, valid_lending("wang"), today)
            .await
            .expect("再借出");

        // 空白借用人直接入庫（繞過驗證）應被排除。
        sqlx::query(
            "INSERT INTO lendings (asset_id, borrower, lent_at)
             VALUES (?, ?, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))",
        )
        .bind(asset_a)
        .bind("   ")
        .execute(&pool)
        .await
        .expect("插入空白借用人");

        let borrowers = list_borrowers(&pool).await.expect("借用人建議");
        assert_eq!(
            borrowers,
            vec![
                "wang".to_string(),
                "陳大頭".to_string(),
                "王小明".to_string()
            ],
            "最近借出者在前、不分大小寫去重（保留最近原文）、排除空白"
        );
    }
}
