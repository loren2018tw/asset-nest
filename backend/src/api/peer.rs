//! `/api/v1` 連線主機 MAC 反查路由（見票 09）。
//!
//! 以連線來源 IP 反查伺服器 ARP 表；查不到回 `{ "mac": null }`。

use std::net::SocketAddr;

use axum::extract::ConnectInfo;
use axum::http::HeaderMap;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

use crate::AppState;
use crate::peer;

pub fn router() -> Router<AppState> {
    Router::new().route("/peer-mac", get(get_peer_mac))
}

#[derive(Debug, Serialize)]
struct PeerMacResponse {
    /// 查不到時為 `null`（不做本機網卡 fallback）。
    mac: Option<String>,
}

/// 連線來源為 loopback 時才信任 `X-Forwarded-For`（見 [`peer::resolve_peer_ip`]）。
async fn get_peer_mac(
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Json<PeerMacResponse> {
    let forwarded_for = headers
        .get("x-forwarded-for")
        .and_then(|value| value.to_str().ok());
    let ip = peer::resolve_peer_ip(remote.ip(), forwarded_for);

    Json(PeerMacResponse {
        mac: peer::peer_mac(ip),
    })
}
