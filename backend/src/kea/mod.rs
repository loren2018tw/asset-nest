//! Kea 整合邊界：整合機制為伺服器內建 HTTP 控制通道（見 `docs/adr/0010`）。
//!
//! 控制通道 client 見 [`http`]；保留推送與完整同步見 [`sync`]（決策見
//! `docs/adr/0011`）。保留推送與租約讀取以 [`KeaGateway`] 定義，待後續階段實作。

pub mod http;
pub mod sync;

use std::net::Ipv4Addr;

use ipnet::Ipv4Net;

/// Kea 主機保留：Subnet 內「IP＋MAC（＋hostname）」的固定對應。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reservation {
    /// MAC 位址（Kea `hw-address`）。
    pub hw_address: String,
    pub ip_address: Ipv4Addr,
    pub hostname: Option<String>,
}

/// Kea 動態租約（唯讀；見 `docs/adr/0002`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lease {
    pub hw_address: String,
    pub ip_address: Ipv4Addr,
    pub hostname: Option<String>,
}

/// Kea 整合錯誤。
#[derive(Debug, thiserror::Error)]
pub enum KeaError {
    #[error("Kea 整合尚未實作")]
    NotImplemented,
    #[error("HTTP 請求失敗：{0}")]
    Http(#[from] reqwest::Error),
    #[error("Kea 回應錯誤（result={result}）：{text}")]
    Response { result: i64, text: String },
    #[error("Kea 回應格式不符：{0}")]
    Malformed(String),
    #[error("Kea 推送資料異常：{0}")]
    Data(String),
}

/// Kea 整合介面：以使用案例定義；實作於功能階段。
#[allow(async_fn_in_trait)]
pub trait KeaGateway {
    /// 列出指定 Subnet 的所有保留。
    async fn list_reservations(&self, subnet: Ipv4Net) -> Result<Vec<Reservation>, KeaError>;

    /// 新增或更新一筆保留（冪等；見 `docs/adr/0002`）。
    async fn upsert_reservation(
        &self,
        subnet: Ipv4Net,
        reservation: Reservation,
    ) -> Result<(), KeaError>;

    /// 刪除指定 MAC 的保留。
    async fn delete_reservation(&self, subnet: Ipv4Net, hw_address: &str) -> Result<(), KeaError>;

    /// 重載 Kea 設定，讓變更生效。
    async fn reload(&self) -> Result<(), KeaError>;

    /// 列出指定 Subnet 的動態租約（唯讀）。
    async fn list_leases(&self, subnet: Ipv4Net) -> Result<Vec<Lease>, KeaError>;
}
