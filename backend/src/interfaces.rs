//! 網路介面（Interface）領域模組：MAC 正規化、名稱門檻與資料庫存取。
//!
//! 詞彙依 `GLOSSARY.md`；規則見 `.scratch/asset-ip-management/spec.md` §2.2、§3.1。

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqliteConnection, SqlitePool};

use crate::api::ApiError;
use crate::assets::{double_option, optional_text};

/// `interfaces` 資料表完整欄位清單。
const COLUMNS: &str = "id, asset_id, name, mac, note, created_at, updated_at";

/// `interfaces` 資料表列。
#[derive(Debug, FromRow)]
struct InterfaceRow {
    id: i64,
    asset_id: i64,
    name: Option<String>,
    mac: Option<String>,
    note: Option<String>,
    created_at: String,
    updated_at: String,
}

/// API 回傳的網路介面。
#[derive(Debug, Serialize)]
pub struct Interface {
    pub id: i64,
    pub asset_id: i64,
    pub name: Option<String>,
    /// 正規化為小寫冒號格式；`None`＝MAC 空白（手動設定介面）。
    pub mac: Option<String>,
    pub note: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 語意警示：僅提示、不阻擋儲存（見 ADR-0006；票 07 將沿用此機制）。
#[derive(Debug, PartialEq, Serialize)]
pub struct Warning {
    pub code: &'static str,
    pub message: String,
}

/// 新增介面的輸入；缺漏欄位視為未填。
#[derive(Debug, Default, Deserialize)]
pub struct InterfaceInput {
    pub name: Option<String>,
    pub mac: Option<String>,
    pub note: Option<String>,
}

/// 已驗證的新增內容。
#[derive(Debug)]
pub struct ValidInterface {
    name: Option<String>,
    mac: Option<String>,
    note: Option<String>,
}

/// 編輯介面的輸入。
///
/// 外層 `None`＝欄位未提供（維持原值）；`Some(None)`＝顯式 `null`（清除）；
/// `Some(Some(..))`＝設定新值。
#[derive(Debug, Default, Deserialize)]
pub struct InterfacePatch {
    #[serde(default, deserialize_with = "double_option")]
    pub name: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub mac: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub note: Option<Option<String>>,
}

impl InterfaceInput {
    /// 驗證新增欄位：MAC 空白時名稱必填；MAC 正規化為小寫冒號格式。
    pub fn validate(self) -> Result<ValidInterface, ApiError> {
        validate_fields(self.name, self.mac, self.note)
    }
}

impl InterfacePatch {
    /// 與既有介面合併後驗證：以合併後的最終狀態判斷「MAC 空白時名稱必填」。
    pub fn apply_to(self, existing: &Interface) -> Result<ValidInterface, ApiError> {
        validate_fields(
            match self.name {
                Some(value) => value,
                None => existing.name.clone(),
            },
            match self.mac {
                Some(value) => value,
                None => existing.mac.clone(),
            },
            match self.note {
                Some(value) => value,
                None => existing.note.clone(),
            },
        )
    }
}

/// 驗證並正規化介面欄位；結構錯誤阻擋儲存（見 ADR-0006）。
fn validate_fields(
    name: Option<String>,
    mac: Option<String>,
    note: Option<String>,
) -> Result<ValidInterface, ApiError> {
    let name = optional_text(name);
    let mac = normalize_mac(mac)?;

    if name.is_none() && mac.is_none() {
        return Err(ApiError::validation("MAC 空白時名稱為必填").field("name"));
    }

    Ok(ValidInterface {
        name,
        mac,
        note: optional_text(note),
    })
}

/// 讀取某資產的介面清單；依建立順序（id 升冪）。
pub async fn list_for_asset(pool: &SqlitePool, asset_id: i64) -> sqlx::Result<Vec<Interface>> {
    let rows = sqlx::query_as::<_, InterfaceRow>(&format!(
        "SELECT {COLUMNS} FROM interfaces WHERE asset_id = ? ORDER BY id ASC"
    ))
    .bind(asset_id)
    .fetch_all(pool)
    .await?;

    Ok(rows.into_iter().map(InterfaceRow::into_interface).collect())
}

/// 讀取單一介面；不存在回傳 `None`。
pub async fn get(pool: &SqlitePool, id: i64) -> sqlx::Result<Option<Interface>> {
    Ok(fetch_row(pool, id).await?.map(InterfaceRow::into_interface))
}

/// 全系統已登錄的 Interface MAC（trim＋小寫；排除空值）。
///
/// 供 IP 清單的觀測 `unknown_mac` 篩選比對（見票 07）：位址的現況
/// `last_seen_mac` 不在此集合即視為未登錄；比較不分大小寫。
pub async fn macs(pool: &SqlitePool) -> sqlx::Result<HashSet<String>> {
    let values: Vec<String> =
        sqlx::query_scalar("SELECT mac FROM interfaces WHERE mac IS NOT NULL AND trim(mac) <> ''")
            .fetch_all(pool)
            .await?;

    Ok(values
        .into_iter()
        .map(|mac| mac.trim().to_ascii_lowercase())
        .collect())
}

/// 於既有連線（可為交易）內新增介面，回傳新列 id；不讀回完整資料。
///
/// 供匯入在同一交易內建立介面（見票 01）；一般建立路徑走 [`create`]。
pub(crate) async fn insert_interface(
    connection: &mut SqliteConnection,
    asset_id: i64,
    valid: ValidInterface,
) -> sqlx::Result<i64> {
    let result =
        sqlx::query("INSERT INTO interfaces (asset_id, name, mac, note) VALUES (?, ?, ?, ?)")
            .bind(asset_id)
            .bind(valid.name)
            .bind(valid.mac)
            .bind(valid.note)
            .execute(connection)
            .await?;

    Ok(result.last_insert_rowid())
}

/// 新增介面；`asset_id` 須存在（外鍵）。由連線池取連線呼叫 [`insert_interface`]。
pub async fn create(
    pool: &SqlitePool,
    asset_id: i64,
    valid: ValidInterface,
) -> sqlx::Result<Interface> {
    let id = {
        let mut connection = pool.acquire().await?;
        insert_interface(&mut connection, asset_id, valid).await?
    };

    fetch_row(pool, id)
        .await?
        .map(InterfaceRow::into_interface)
        .ok_or(sqlx::Error::RowNotFound)
}

/// 編輯介面；不存在回傳 `None`。
pub async fn update(
    pool: &SqlitePool,
    id: i64,
    valid: ValidInterface,
) -> sqlx::Result<Option<Interface>> {
    let result = sqlx::query(
        "UPDATE interfaces
             SET name = ?, mac = ?, note = ?,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
           WHERE id = ?",
    )
    .bind(valid.name)
    .bind(valid.mac)
    .bind(valid.note)
    .bind(id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Ok(None);
    }

    Ok(fetch_row(pool, id).await?.map(InterfaceRow::into_interface))
}

/// 刪除介面；回傳是否確實刪除（指派由外鍵連動刪除，見 spec §2.2、票 08）。
pub async fn delete(pool: &SqlitePool, id: i64) -> sqlx::Result<bool> {
    let result = sqlx::query("DELETE FROM interfaces WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() > 0)
}

/// 全系統重複 MAC 的警示（正規化後比對；僅提示、不阻擋，見 spec §3.2）。
///
/// 一律排除自身 id，避免新增／編輯時誤報。
pub async fn mac_warnings(
    pool: &SqlitePool,
    mac: Option<&str>,
    exclude_id: i64,
) -> sqlx::Result<Vec<Warning>> {
    let Some(mac) = mac else {
        return Ok(Vec::new());
    };

    let count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM interfaces WHERE mac = ? AND id <> ?")
            .bind(mac)
            .bind(exclude_id)
            .fetch_one(pool)
            .await?;

    if count == 0 {
        return Ok(Vec::new());
    }

    Ok(vec![Warning {
        code: "duplicate_mac",
        message: format!("MAC {mac} 已被其他介面使用（僅提示，不阻擋儲存）"),
    }])
}

/// 讀取一列；不存在回傳 `None`。
async fn fetch_row(pool: &SqlitePool, id: i64) -> sqlx::Result<Option<InterfaceRow>> {
    sqlx::query_as::<_, InterfaceRow>(&format!("SELECT {COLUMNS} FROM interfaces WHERE id = ?"))
        .bind(id)
        .fetch_optional(pool)
        .await
}

impl InterfaceRow {
    fn into_interface(self) -> Interface {
        Interface {
            id: self.id,
            asset_id: self.asset_id,
            name: self.name,
            mac: self.mac,
            note: self.note,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// 正規化 MAC：接受冒號、連字號、點號或無分隔的 12 位十六進位，
/// 儲存為小寫冒號格式；空白視為未填，其餘格式視為結構錯誤。
///
/// 供匯入共用（見票 01）：規則與錯誤文案與介面建立一致。
pub(crate) fn normalize_mac(value: Option<String>) -> Result<Option<String>, ApiError> {
    let Some(text) = optional_text(value) else {
        return Ok(None);
    };

    let hex: String = text
        .chars()
        .filter(|character| !matches!(character, ':' | '-' | '.'))
        .collect();

    if hex.len() != 12 || !hex.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(ApiError::validation(
            "MAC 格式錯誤：須為 12 位十六進位（可用冒號、連字號或無分隔）",
        )
        .field("mac"));
    }

    let hex = hex.to_ascii_lowercase();
    let mut normalized = String::with_capacity(17);
    for (index, character) in hex.chars().enumerate() {
        if index > 0 && index % 2 == 0 {
            normalized.push(':');
        }
        normalized.push(character);
    }

    Ok(Some(normalized))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mac_normalization_accepts_common_formats() {
        for input in [
            "AA:BB:CC:DD:EE:FF",
            "aa-bb-cc-dd-ee-ff",
            "aabbccddeeff",
            "AA.BB.CC.DD.EE.FF",
            " aa:bb:cc:dd:ee:ff ",
        ] {
            assert_eq!(
                normalize_mac(Some(input.to_string())).expect("合法 MAC"),
                Some("aa:bb:cc:dd:ee:ff".to_string()),
                "{input}"
            );
        }
    }

    #[test]
    fn mac_normalization_rejects_invalid_values() {
        for input in [
            "aa:bb:cc:dd:ee",
            "aa:bb:cc:dd:ee:ff:00",
            "gg:hh:ii:jj:kk:ll",
            "not-a-mac",
        ] {
            assert!(
                normalize_mac(Some(input.to_string())).is_err(),
                "{input} 應視為格式錯誤"
            );
        }
    }

    #[test]
    fn blank_mac_is_treated_as_missing() {
        assert_eq!(normalize_mac(None).expect("未提供"), None);
        assert_eq!(
            normalize_mac(Some("   ".to_string())).expect("全空白"),
            None
        );
    }

    #[test]
    fn empty_mac_requires_name() {
        assert!(validate_fields(None, None, None).is_err(), "兩者皆空應阻擋");
        assert!(
            validate_fields(Some("   ".to_string()), None, None).is_err(),
            "全空白名稱應阻擋"
        );
        assert!(validate_fields(Some("eth0".to_string()), None, None).is_ok());
        assert!(validate_fields(None, Some("aabbccddeeff".to_string()), None).is_ok());
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

    /// 於同一交易內建立測試資產（介面外鍵）並回傳 id。
    async fn insert_test_asset(connection: &mut SqliteConnection) -> i64 {
        let asset = crate::assets::AssetInput {
            description: Some("測試資產".to_string()),
            location: Some("機房 A".to_string()),
            ..Default::default()
        }
        .validate()
        .expect("有效資產");

        crate::assets::insert_asset(connection, asset)
            .await
            .expect("新增資產")
    }

    /// 測試用已驗證介面。
    fn valid_interface() -> ValidInterface {
        ValidInterface {
            name: Some("eth0".to_string()),
            mac: Some("aa:bb:cc:dd:ee:ff".to_string()),
            note: None,
        }
    }

    #[tokio::test]
    async fn insert_interface_is_visible_in_same_transaction() {
        let pool = test_pool().await;
        let mut transaction = pool.begin().await.expect("建立交易");
        let asset_id = insert_test_asset(&mut transaction).await;

        let id = insert_interface(&mut transaction, asset_id, valid_interface())
            .await
            .expect("新增介面");
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM interfaces WHERE id = ?")
            .bind(id)
            .fetch_one(&mut *transaction)
            .await
            .expect("同交易讀取介面");

        assert_eq!(count, 1, "原語建立後同交易可見");
    }

    #[tokio::test]
    async fn insert_interface_rollback_leaves_nothing() {
        let pool = test_pool().await;
        let mut transaction = pool.begin().await.expect("建立交易");
        let asset_id = insert_test_asset(&mut transaction).await;

        insert_interface(&mut transaction, asset_id, valid_interface())
            .await
            .expect("新增介面");
        transaction.rollback().await.expect("回滾交易");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM interfaces")
            .fetch_one(&pool)
            .await
            .expect("回滾後讀取介面");

        assert_eq!(count, 0, "回滾後不留下資料");
    }
}
