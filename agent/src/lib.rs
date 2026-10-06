//! asset-nest 觀測代理函式庫：設定、ARP 探測、被動聚合、掃描排程與回報推送。
//!
//! binary `asset-nest-agent` 只負責啟動與環境設定；其餘邏輯集中於此以利
//! 單元與整合測試（見 `.scratch/observation-agent/issues/`）。

pub mod config;
pub mod heartbeat;
pub mod host_range;
pub mod passive;
pub mod probe;
pub mod push;
pub mod runner;

/// 代理版本（由 Cargo 套件版本注入；心跳與觀測回報一併送出）。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
