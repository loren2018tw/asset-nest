//! 資產 CSV 匯出（Export）：套用清單的搜尋／篩選／排序取全部符合資產，
//! 輸出 `docs/adr/0008` 的 14 欄格式（見 spec §4、票 04）。
//!
//! 一列一資產；介面與位址選取規則見 [`crate::assignments::export_networks`]。
//! 輸出為 UTF-8 BOM＋標題列（不含範例列），供空庫重建時直接匯入。

use std::collections::HashMap;

use sqlx::SqlitePool;

use crate::api::ApiError;
use crate::assets::{self, Asset, AssetFilter};
use crate::assignments::{self, ExportNetwork};

/// 套用篩選與排序輸出全部符合資產的 CSV（UTF-8 BOM＋標題列；見 spec §4）。
///
/// 分頁由 [`AssetFilter`] 攜帶但匯出忽略；資料讀取為兩批查詢（資產、
/// 介面＋指派），避免逐資產 N+1（見票 04）。
pub async fn export_csv(pool: &SqlitePool, filter: &AssetFilter) -> Result<Vec<u8>, ApiError> {
    let assets = assets::list_all(pool, filter)
        .await
        .map_err(|error| ApiError::internal("讀取資產匯出資料失敗", error))?;

    let ids: Vec<i64> = assets.iter().map(|asset| asset.id).collect();
    let networks = assignments::export_networks(pool, &ids)
        .await
        .map_err(|error| ApiError::internal("讀取資產匯出網路資料失敗", error))?;

    to_csv(&assets, &networks)
}

/// 資產欄位與網路欄位轉 CSV（UTF-8 BOM＋標題列；14 欄與 ADR-0008 一致）。
///
/// 選填欄位缺值為空字串；標籤以 `|` 串接；日期入庫時已是 `YYYY-MM-DD`。
/// 無介面的資產（不在 `networks`）MAC／IPv4／IPv6／hostname 全空。
pub fn to_csv(
    assets: &[Asset],
    networks: &HashMap<i64, ExportNetwork>,
) -> Result<Vec<u8>, ApiError> {
    let empty = ExportNetwork::default();
    let mut writer = csv::Writer::from_writer(Vec::new());

    writer
        .write_record([
            "財產編號",
            "描述",
            "位置",
            "設備序號",
            "廠牌",
            "型號",
            "購置日期",
            "年限",
            "備註",
            "標籤",
            "MAC",
            "IPv4",
            "IPv6",
            "hostname",
        ])
        .map_err(write_error)?;

    for asset in assets {
        let network = networks.get(&asset.id).unwrap_or(&empty);
        let lifespan = asset
            .lifespan_years
            .map(|years| years.to_string())
            .unwrap_or_default();
        let tags = asset.tags.join("|");

        writer
            .write_record([
                asset.property_no.as_deref().unwrap_or(""),
                asset.description.as_str(),
                asset.location.as_str(),
                asset.device_serial.as_deref().unwrap_or(""),
                asset.brand.as_deref().unwrap_or(""),
                asset.model.as_deref().unwrap_or(""),
                asset.purchase_date.as_deref().unwrap_or(""),
                lifespan.as_str(),
                asset.note.as_deref().unwrap_or(""),
                tags.as_str(),
                network.mac.as_deref().unwrap_or(""),
                network.ipv4.as_deref().unwrap_or(""),
                network.ipv6.as_deref().unwrap_or(""),
                network.hostname.as_deref().unwrap_or(""),
            ])
            .map_err(write_error)?;
    }

    let body = writer
        .into_inner()
        .map_err(|error| ApiError::internal("產生資產匯出 CSV 失敗", error))?;

    let mut bytes = Vec::with_capacity(body.len() + 3);
    bytes.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
    bytes.extend_from_slice(&body);
    Ok(bytes)
}

/// CSV 寫入錯誤一律視為內部錯誤（寫入目標為記憶體緩衝區）。
fn write_error(error: csv::Error) -> ApiError {
    ApiError::internal("產生資產匯出 CSV 失敗", error)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 測試用資產；其餘欄位固定。
    fn asset(id: i64, tags: &[&str]) -> Asset {
        Asset {
            id,
            property_no: Some("PC-001".to_string()),
            description: "測試資產".to_string(),
            location: "機房 A".to_string(),
            device_serial: Some("SN-001".to_string()),
            brand: Some("ASUS".to_string()),
            model: Some("BM6630".to_string()),
            purchase_date: Some("2024-01-15".to_string()),
            lifespan_years: Some(5),
            note: Some("含,逗號".to_string()),
            tags: tags.iter().map(|tag| tag.to_string()).collect(),
            expired: false,
            created_at: "2026-10-05T00:00:00Z".to_string(),
            updated_at: "2026-10-05T00:00:00Z".to_string(),
        }
    }

    /// 解析 CSV（跳過 BOM）；回傳標題與資料列。
    fn parse(bytes: &[u8]) -> (Vec<String>, Vec<Vec<String>>) {
        assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF], "須有 UTF-8 BOM");
        let mut reader = csv::Reader::from_reader(&bytes[3..]);
        let headers = reader
            .headers()
            .expect("標題列")
            .iter()
            .map(str::to_string)
            .collect();
        let rows = reader
            .into_records()
            .map(|record| record.expect("資料列").iter().map(str::to_string).collect())
            .collect();
        (headers, rows)
    }

    #[test]
    fn to_csv_writes_14_columns_with_tags_dates_and_quoting() {
        let mut networks = HashMap::new();
        networks.insert(
            1,
            ExportNetwork {
                mac: Some("aa:bb:cc:dd:ee:ff".to_string()),
                ipv4: Some("10.0.0.2".to_string()),
                ipv6: Some("fd00::2".to_string()),
                hostname: Some("pc-001".to_string()),
            },
        );

        let bytes = to_csv(&[asset(1, &["行政", "電腦"])], &networks).expect("匯出成功");
        let (headers, rows) = parse(&bytes);

        assert_eq!(
            headers,
            [
                "財產編號",
                "描述",
                "位置",
                "設備序號",
                "廠牌",
                "型號",
                "購置日期",
                "年限",
                "備註",
                "標籤",
                "MAC",
                "IPv4",
                "IPv6",
                "hostname"
            ]
        );
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].len(), 14, "一列 14 欄");
        assert_eq!(rows[0][0], "PC-001");
        assert_eq!(rows[0][6], "2024-01-15", "日期已是 YYYY-MM-DD");
        assert_eq!(rows[0][7], "5");
        assert_eq!(rows[0][8], "含,逗號", "含逗號欄位經引號往返");
        assert_eq!(rows[0][9], "行政|電腦", "標籤以 | 串接");
        assert_eq!(rows[0][10], "aa:bb:cc:dd:ee:ff");
        assert_eq!(rows[0][11], "10.0.0.2");
        assert_eq!(rows[0][12], "fd00::2");
        assert_eq!(rows[0][13], "pc-001");
    }

    #[test]
    fn to_csv_leaves_missing_values_empty() {
        let bare = Asset {
            property_no: None,
            device_serial: None,
            brand: None,
            model: None,
            purchase_date: None,
            lifespan_years: None,
            note: None,
            tags: Vec::new(),
            ..asset(1, &[])
        };

        let bytes = to_csv(&[bare], &HashMap::new()).expect("匯出成功");
        let (_, rows) = parse(&bytes);
        assert_eq!(rows.len(), 1);
        for index in [0, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13] {
            assert_eq!(rows[0][index], "", "第 {} 欄缺值為空字串", index + 1);
        }
        assert_eq!(rows[0][1], "測試資產");
        assert_eq!(rows[0][2], "機房 A");
    }

    #[test]
    fn to_csv_writes_header_only_when_no_assets() {
        let bytes = to_csv(&[], &HashMap::new()).expect("匯出成功");
        let (headers, rows) = parse(&bytes);
        assert_eq!(headers.len(), 14);
        assert!(rows.is_empty());
    }
}
