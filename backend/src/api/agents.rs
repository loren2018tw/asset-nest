//! `/api/v1` 觀測代理路由（見票 01、票 02、spec §HTTP API、ADR-0019）。
//!
//! 心跳與觀測回報端點以共用認證碼（`X-Auth-Code`）驗證：後端未設定
//! `AGENT_AUTH_CODE` 一律 503；不符 401 且不寫入代理、改記被拒回報。
//! 來源 IP 以 [`crate::peer::resolve_peer_ip`] 判定：連線來源為 loopback
//! （本機反向代理，如 nginx）時採 `X-Forwarded-For` 第一段，否則一律用
//! 連線來源、忽略 XFF 以免遠端偽造（見 ADR-0022；ADR-0019 修訂）。

use std::net::{Ipv4Addr, SocketAddr};

use axum::extract::rejection::JsonRejection;
use axum::extract::{ConnectInfo, State};
use axum::http::HeaderMap;
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, NaiveDateTime, Utc};
use ipnet::IpNet;
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::agents::{
    self, Agent, AuthFailure, Heartbeat, ObservationReport, PassiveReport, ReportEntry, SweepReport,
};
use crate::api::ApiError;
use crate::observation::normalize_mac;

/// 代理認證碼標頭（見 ADR-0019）。
const AUTH_CODE_HEADER: &str = "x-auth-code";

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/agents/heartbeat", post(heartbeat))
        .route("/agents/observations", post(observations))
        .route("/agents", get(list_agents))
        .route("/agents/auth-failures", get(list_auth_failures))
}

/// 心跳輸入；欄位缺漏、空字串或非法 CIDR 皆回 400（見票 01）。
#[derive(Debug, Deserialize)]
struct HeartbeatInput {
    #[serde(default)]
    instance_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    subnet_cidr: Option<String>,
}

/// 心跳回應：`subnet_matched` 為回報 CIDR 是否精確對應受管網段。
#[derive(Debug, Serialize)]
struct HeartbeatResponse {
    subnet_matched: bool,
}

/// 觀測回報輸入（見票 02、票 03、spec §HTTP API）：sweep 與 passive 共用
/// 欄位容器，`kind` 決定採用的欄位（其餘忽略）。
#[derive(Debug, Deserialize)]
struct ObservationsInput {
    #[serde(default)]
    instance_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    subnet_cidr: Option<String>,
    #[serde(default)]
    reports: Vec<ReportInput>,
}

/// 單筆報告輸入：`kind = "sweep"` 用 `checked`／`seen`；`kind = "passive"`
/// 用 `senders`。
#[derive(Debug, Deserialize)]
struct ReportInput {
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    observed_at: Option<String>,
    #[serde(default)]
    checked: Vec<String>,
    #[serde(default)]
    seen: Vec<AddressMacInput>,
    #[serde(default)]
    senders: Vec<AddressMacInput>,
}

/// `seen`／`senders` 的一筆：位址與 MAC。
#[derive(Debug, Deserialize)]
struct AddressMacInput {
    #[serde(default)]
    address: Option<String>,
    #[serde(default)]
    mac: Option<String>,
}

/// 觀測回報回應；未對應受管網段時 `stored = false` 並附原因。
#[derive(Debug, Serialize)]
struct ObservationsResponse {
    stored: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
}

/// 代理清單回應。
#[derive(Debug, Serialize)]
struct AgentItems {
    /// 在線門檻秒數（`AGENT_STALE_SECS`）。
    stale_secs: u64,
    items: Vec<Agent>,
}

/// 被拒回報清單回應。
#[derive(Debug, Serialize)]
struct AuthFailureItems {
    items: Vec<AuthFailure>,
}

/// `POST /api/v1/agents/heartbeat`：驗證認證碼、upsert 代理、回 `subnet_matched`。
async fn heartbeat(
    State(state): State<AppState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    payload: Result<Json<HeartbeatInput>, JsonRejection>,
) -> Result<Json<HeartbeatResponse>, ApiError> {
    let source_ip = crate::peer::resolve_peer_ip(
        remote.ip(),
        headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok()),
    )
    .to_string();
    let now = Utc::now();

    // 認證不符：401、不寫入代理，改記被拒回報（自報名稱／版本 best-effort）。
    let claimed = match &payload {
        Ok(Json(input)) => (
            non_empty(input.name.clone()),
            non_empty(input.version.clone()),
        ),
        Err(_) => (None, None),
    };
    authorize(&state, &headers, &source_ip, claimed, now).await?;

    let Json(input) = payload.map_err(|_| ApiError::validation("請求內容格式錯誤"))?;
    let heartbeat = Heartbeat {
        instance_id: required(input.instance_id, "instance_id")?,
        name: required(input.name, "name")?,
        version: required(input.version, "version")?,
        subnet_cidr: require_cidr(input.subnet_cidr)?,
    };

    let subnet_id = agents::find_subnet_id(&state.db, &heartbeat.subnet_cidr).await?;
    agents::record_heartbeat(&state.db, &heartbeat, &source_ip, subnet_id, now).await?;

    Ok(Json(HeartbeatResponse {
        subnet_matched: subnet_id.is_some(),
    }))
}

/// `POST /api/v1/agents/observations`：驗證認證碼、套用 sweep／passive
/// 報告（見票 02、票 03）。
///
/// 認證與未設碼行為同心跳（401／503）。CIDR 對不到受管網段時只記代理狀態、
/// 觀測不入庫，回 `{"stored": false, "reason": "subnet_unmatched"}`。
async fn observations(
    State(state): State<AppState>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    payload: Result<Json<ObservationsInput>, JsonRejection>,
) -> Result<Json<ObservationsResponse>, ApiError> {
    let source_ip = crate::peer::resolve_peer_ip(
        remote.ip(),
        headers
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok()),
    )
    .to_string();
    let now = Utc::now();

    let claimed = match &payload {
        Ok(Json(input)) => (
            non_empty(input.name.clone()),
            non_empty(input.version.clone()),
        ),
        Err(_) => (None, None),
    };
    authorize(&state, &headers, &source_ip, claimed, now).await?;

    let Json(input) = payload.map_err(|_| ApiError::validation("請求內容格式錯誤"))?;
    let report = validate_observations(input)?;

    let subnet_id = agents::find_subnet_id(&state.db, &report.agent.subnet_cidr).await?;
    let Some(subnet_id) = subnet_id else {
        // 未對應：代理狀態照記（算一次回報），觀測不入庫（見 spec §HTTP API）。
        agents::record_heartbeat(&state.db, &report.agent, &source_ip, None, now).await?;
        return Ok(Json(ObservationsResponse {
            stored: false,
            reason: Some("subnet_unmatched"),
        }));
    };

    agents::record_observations(&state.db, &report, &source_ip, subnet_id, now).await?;

    Ok(Json(ObservationsResponse {
        stored: true,
        reason: None,
    }))
}

/// `GET /api/v1/agents`：代理清單（last_report_at 新到舊）與在線判定。
async fn list_agents(State(state): State<AppState>) -> Result<Json<AgentItems>, ApiError> {
    let items = agents::list(&state.db, Utc::now(), state.agent_stale_secs).await?;

    Ok(Json(AgentItems {
        stale_secs: state.agent_stale_secs,
        items,
    }))
}

/// `GET /api/v1/agents/auth-failures`：被拒回報清單（last_attempt_at 新到舊）。
async fn list_auth_failures(
    State(state): State<AppState>,
) -> Result<Json<AuthFailureItems>, ApiError> {
    let items = agents::list_auth_failures(&state.db).await?;

    Ok(Json(AuthFailureItems { items }))
}

/// 驗證代理入庫認證碼：未設碼回 503（入庫端點未啟用）；不符回 401，記被拒
/// 回報（來源 IP、自報名稱／版本 best-effort）且不寫入任何資料（見票 01）。
async fn authorize(
    state: &AppState,
    headers: &HeaderMap,
    source_ip: &str,
    claimed: (Option<String>, Option<String>),
    now: DateTime<Utc>,
) -> Result<(), ApiError> {
    // 未設定認證碼：入庫端點一律 503（見 ADR-0019）。
    let Some(configured) = state.agent_auth_code.as_deref() else {
        return Err(ApiError::unavailable(
            "後端未設定代理認證碼（AGENT_AUTH_CODE），代理入庫端點未啟用",
        ));
    };

    let provided = headers
        .get(AUTH_CODE_HEADER)
        .and_then(|value| value.to_str().ok());
    if provided != Some(configured) {
        agents::record_auth_failure(
            &state.db,
            source_ip,
            claimed.0.as_deref(),
            claimed.1.as_deref(),
            now,
        )
        .await?;

        return Err(ApiError::unauthorized("代理認證碼不符"));
    }

    Ok(())
}

/// 驗證觀測回報輸入（見票 02、票 03）：必填字串、CIDR 與各報告欄位；
/// `kind` 只接受 `sweep` 與 `passive`。
fn validate_observations(input: ObservationsInput) -> Result<ObservationReport, ApiError> {
    let agent = Heartbeat {
        instance_id: required(input.instance_id, "instance_id")?,
        name: required(input.name, "name")?,
        version: required(input.version, "version")?,
        subnet_cidr: require_cidr(input.subnet_cidr)?,
    };

    let mut reports = Vec::with_capacity(input.reports.len());
    for (index, report) in input.reports.into_iter().enumerate() {
        reports.push(validate_report(index, report)?);
    }

    Ok(ObservationReport { agent, reports })
}

/// 驗證單筆報告（見票 02、票 03）：`kind` 決定採用的欄位；未知 kind 回
/// 400 並標示 `reports[i].kind`。
fn validate_report(index: usize, report: ReportInput) -> Result<ReportEntry, ApiError> {
    let kind_field = format!("reports[{index}].kind");
    let kind = report.kind.as_deref().map(str::trim).ok_or_else(|| {
        ApiError::validation("kind 為必填且須為 sweep 或 passive").field(&kind_field)
    })?;

    let observed_at = require_observed_at(index, report.observed_at.as_deref())?;

    match kind {
        "sweep" => {
            let mut checked = Vec::with_capacity(report.checked.len());
            for (position, raw) in report.checked.into_iter().enumerate() {
                let field = format!("reports[{index}].checked[{position}]");
                checked.push(require_ipv4(Some(raw), &field)?);
            }

            let seen = require_address_macs(index, "seen", report.seen)?;
            Ok(ReportEntry::Sweep(SweepReport {
                observed_at,
                checked,
                seen,
            }))
        }
        "passive" => {
            let senders = require_address_macs(index, "senders", report.senders)?;
            Ok(ReportEntry::Passive(PassiveReport {
                observed_at,
                senders,
            }))
        }
        other => Err(
            ApiError::validation(format!("報告種類僅支援 sweep 或 passive：{other}"))
                .field(&kind_field),
        ),
    }
}

/// 報告時間欄位（`reports[i].observed_at`；嚴格 `YYYY-MM-DDTHH:MM:SSZ`）。
fn require_observed_at(index: usize, raw: Option<&str>) -> Result<DateTime<Utc>, ApiError> {
    let field = format!("reports[{index}].observed_at");
    raw.map(str::trim).and_then(parse_timestamp).ok_or_else(|| {
        ApiError::validation("observed_at 格式錯誤（須為 YYYY-MM-DDTHH:MM:SSZ）").field(&field)
    })
}

/// 驗證 `seen`／`senders` 的位址與 MAC 清單（欄位標示比照票 02 慣例）。
fn require_address_macs(
    index: usize,
    list: &str,
    entries: Vec<AddressMacInput>,
) -> Result<Vec<(Ipv4Addr, String)>, ApiError> {
    let mut output = Vec::with_capacity(entries.len());
    for (position, entry) in entries.into_iter().enumerate() {
        let address_field = format!("reports[{index}].{list}[{position}].address");
        let mac_field = format!("reports[{index}].{list}[{position}].mac");
        output.push((
            require_ipv4(entry.address, &address_field)?,
            require_mac(entry.mac, &mac_field)?,
        ));
    }
    Ok(output)
}

/// 必填字串：缺漏、空字串或全空白皆回 400 並標示欄位。
fn required(value: Option<String>, field: &'static str) -> Result<String, ApiError> {
    non_empty(value).ok_or_else(|| ApiError::validation(format!("{field} 為必填")).field(field))
}

/// 正規化可選字串：缺漏、空字串或全空白皆視為未提供。
fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

/// 必填 CIDR：合法者以 [`IpNet`] 正規化（host bits 收斂為網路地址後儲存／比對）。
fn require_cidr(value: Option<String>) -> Result<IpNet, ApiError> {
    let Some(raw) = non_empty(value) else {
        return Err(ApiError::validation("subnet_cidr 為必填").field("subnet_cidr"));
    };

    raw.parse::<IpNet>()
        .map(|network| network.trunc())
        .map_err(|_| {
            ApiError::validation(format!(
                "subnet_cidr 格式錯誤：{raw}（須為 CIDR，例：10.1.0.0/24）"
            ))
            .field("subnet_cidr")
        })
}

/// 必填 IPv4 位址：非法值回 400 並標示欄位（見票 02）。
fn require_ipv4(value: Option<String>, field: &str) -> Result<Ipv4Addr, ApiError> {
    let Some(raw) = non_empty(value) else {
        return Err(ApiError::validation(format!("{field} 為必填")).field(field));
    };

    raw.parse::<Ipv4Addr>().map_err(|_| {
        ApiError::validation(format!("{field} 位址格式錯誤：{raw}（須為 IPv4）")).field(field)
    })
}

/// 必填 MAC：以 [`normalize_mac`] 正規化為小寫冒號格式；格式不符回 400。
fn require_mac(value: Option<String>, field: &str) -> Result<String, ApiError> {
    let Some(raw) = non_empty(value) else {
        return Err(ApiError::validation(format!("{field} 為必填")).field(field));
    };

    normalize_mac(&raw).ok_or_else(|| {
        ApiError::validation(format!(
            "{field} MAC 格式錯誤：{raw}（須為 6 或 8 組兩位十六進位，可用冒號或連字號分隔）"
        ))
        .field(field)
    })
}

/// 解析回報時間（`YYYY-MM-DDTHH:MM:SSZ`；與資料庫時間格式一致）。
fn parse_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%SZ")
        .ok()
        .map(|naive| naive.and_utc())
}
