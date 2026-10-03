//! Kea 整合邊界：本階段只定型別與介面，不實作任何連線（見 `docs/adr/0001`）。
//!
//! 整合機制未定（設定檔＋control-socket 或 REST Control Agent），
//! 留待功能階段決策；此處僅以使用案例定義介面，讓兩種機制皆可替換實作。

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

/// Kea 整合錯誤（功能階段擴充）。
#[derive(Debug, thiserror::Error)]
pub enum KeaError {
    #[error("Kea 整合尚未實作")]
    NotImplemented,
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
