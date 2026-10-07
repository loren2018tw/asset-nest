//! `/api/v1` Kea 路由（見 `docs/adr/0010`、`docs/adr/0011`）。
//!
//! `GET /kea/leases`：DHCPv4 動態租約清單（唯讀；見票 02）。
//! `GET /kea/status`：連線診斷；一律 200、分區容錯（見票 01）。
//! `GET /kea/sync/plan`：完整同步計畫（dry-run、唯讀）。
//! `POST /kea/sync`：重算計畫並套用；回傳每網段增／改／刪計數、失敗清單與
//! `config-write` 狀態。`KEA_API_URL` 未設定時回 400。

use std::collections::{HashMap, HashSet};

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Serialize;

use crate::AppState;
use crate::api::ApiError;
use crate::kea::http::{Client, StatusInfo};
use crate::kea::sync::{self, SyncApplyReport, SyncPlan};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/kea/leases", get(leases))
        .route("/kea/status", get(status))
        .route("/kea/sync/plan", get(sync_plan))
        .route("/kea/sync", post(apply_sync))
}

/// 取得 Kea client；未設定時回 400（前端據訊息提示）。
///
/// `action` 接在訊息後（如「無法同步」、「無法讀取租約」），沿用既有提示風格。
fn client<'a>(state: &'a AppState, action: &str) -> Result<&'a Client, ApiError> {
    state
        .kea
        .as_ref()
        .ok_or_else(|| ApiError::validation(format!("Kea 未設定（KEA_API_URL），{action}")))
}

/// `GET /kea/status` 回應（一律 200、分區容錯；見票 01）。
#[derive(Serialize)]
struct KeaStatus {
    configured: bool,
    reachable: bool,
    url: Option<String>,
    version: Option<VersionBlock>,
    interfaces: Option<Vec<String>>,
    runtime: Option<StatusInfo>,
    dhcp4: Option<Dhcp4Block>,
    #[serde(skip_serializing_if = "Errors::is_empty")]
    errors: Errors,
}

/// 版本區塊（`version-get` 成功時）。
#[derive(Serialize)]
struct VersionBlock {
    version: Option<String>,
    text: Option<String>,
}

/// DHCPv4 摘要區塊（`config-get` 成功時）。
#[derive(Serialize)]
struct Dhcp4Block {
    /// Kea `Dhcp4.subnet4` 筆數。
    subnet_count: usize,
    /// 本地 `kea_subnet_id IS NOT NULL` 的網段數。
    managed_subnet_count: i64,
    lease_backend: Option<String>,
}

/// 各命令獨立的失敗訊息；皆無失敗時整個 `errors` 省略。
#[derive(Default, Serialize)]
struct Errors {
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    config: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
}

impl Errors {
    fn is_empty(&self) -> bool {
        self.version.is_none() && self.config.is_none() && self.status.is_none()
    }
}

/// Kea 系統狀態：未設定 `KEA_API_URL` 時回中性結果（不發任何命令）。
async fn status(State(state): State<AppState>) -> Result<Json<KeaStatus>, ApiError> {
    // 此端點不得走會回 400 的 `client()`：未設定視為未配置，而非錯誤。
    let Some(client) = state.kea.as_ref() else {
        return Ok(Json(KeaStatus {
            configured: false,
            reachable: false,
            url: None,
            version: None,
            interfaces: None,
            runtime: None,
            dhcp4: None,
            errors: Errors::default(),
        }));
    };

    // 三命令各自獨立並行嘗試；任一失敗不影響其他區塊。
    let (version, config, runtime) = tokio::join!(
        client.version_get(),
        client.config_get_dhcp4(),
        client.status_get()
    );

    let mut errors = Errors::default();

    let version = match version {
        Ok(info) => Some(VersionBlock {
            version: info.version,
            text: info.text,
        }),
        Err(err) => {
            errors.version = Some(err.to_string());
            None
        }
    };
    let reachable = version.is_some();

    let interfaces = match &config {
        Ok(config) => Some(config.interfaces.clone()),
        Err(err) => {
            errors.config = Some(err.to_string());
            None
        }
    };

    let runtime = match runtime {
        Ok(info) => Some(info),
        Err(err) => {
            errors.status = Some(err.to_string());
            None
        }
    };

    let dhcp4 = match config {
        Ok(config) => {
            let managed_subnet_count = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM subnets WHERE kea_subnet_id IS NOT NULL",
            )
            .fetch_one(&state.db)
            .await
            .map_err(|err| ApiError::internal("讀取本地受管網段數失敗", err))?;

            Some(Dhcp4Block {
                subnet_count: config.subnets.len(),
                managed_subnet_count,
                lease_backend: config.lease_backend,
            })
        }
        Err(_) => None,
    };

    Ok(Json(KeaStatus {
        configured: true,
        reachable,
        url: Some(client.base_url()),
        version,
        interfaces,
        runtime,
        dhcp4,
        errors,
    }))
}

/// `GET /kea/leases` 回應（唯讀；見票 02）。
#[derive(Serialize)]
struct KeaLeases {
    leases: Vec<KeaLeaseEntry>,
}

/// 租約清單中的一筆；本地無對應受管網段時 `subnet_cidr`／`subnet_name` 為 null。
///
/// `is_reservation` 僅在「本地受管網段（`kea_subnet_id` 相符）內、且位址與
/// `purpose = 'reservation'` 指派完全相符」時為 true；無對應受管網段一律 false
/// （無從判定，不得誤標）。
#[derive(Serialize)]
struct KeaLeaseEntry {
    ip_address: Option<String>,
    hw_address: Option<String>,
    hostname: Option<String>,
    subnet_id: Option<i64>,
    subnet_cidr: Option<String>,
    subnet_name: Option<String>,
    /// `cltt + valid_lft`（ISO 8601 UTC）；任一缺欄位為 null。
    expires_at: Option<String>,
    state: Option<String>,
    /// 是否為本地「保留」（受管網段內、位址與 reservation 指派相符）。
    is_reservation: bool,
}

/// Kea 動態租約清單：未設定 `KEA_API_URL` 回 400、命令失敗回 502。
async fn leases(State(state): State<AppState>) -> Result<Json<KeaLeases>, ApiError> {
    let client = client(&state, "無法讀取租約")?;

    let leases = client
        .lease4_get_all()
        .await
        .map_err(|error| ApiError::kea(format!("讀取 Kea 租約失敗：{error}")))?;

    // 本地受管網段（`kea_subnet_id` → CIDR＋名稱）與其中的保留位址集合
    // （`(kea_subnet_id, address)`），供租約對應顯示與「保留」標示。
    let rows = sqlx::query_as::<_, (i64, String, Option<String>, Option<String>)>(
        "SELECT s.kea_subnet_id, s.cidr, s.name, a.address
         FROM subnets s
         LEFT JOIN ip_assignments a
           ON a.subnet_id = s.id AND a.purpose = 'reservation'
         WHERE s.kea_subnet_id IS NOT NULL",
    )
    .fetch_all(&state.db)
    .await
    .map_err(|error| ApiError::internal("讀取網段失敗", error))?;

    let mut subnets: HashMap<i64, (String, Option<String>)> = HashMap::new();
    let mut reservations: HashSet<(i64, String)> = HashSet::new();
    for (kea_subnet_id, cidr, name, address) in rows {
        subnets.insert(kea_subnet_id, (cidr, name));
        if let Some(address) = address {
            reservations.insert((kea_subnet_id, address));
        }
    }

    let leases = leases
        .into_iter()
        .map(|lease| {
            let (subnet_cidr, subnet_name) = lease
                .subnet_id
                .and_then(|subnet_id| subnets.get(&subnet_id))
                .map(|(cidr, name)| (Some(cidr.clone()), name.clone()))
                .unwrap_or((None, None));

            // 保留判定：受管網段內、位址與本地 reservation 指派完全相符。
            let is_reservation = lease
                .subnet_id
                .zip(lease.ip_address.as_deref())
                .is_some_and(|(subnet_id, ip)| reservations.contains(&(subnet_id, ip.to_string())));

            KeaLeaseEntry {
                expires_at: expires_at(lease.cltt, lease.valid_lft),
                ip_address: lease.ip_address,
                hw_address: lease.hw_address,
                hostname: lease.hostname,
                subnet_id: lease.subnet_id,
                subnet_cidr,
                subnet_name,
                state: lease.state,
                is_reservation,
            }
        })
        .collect();

    Ok(Json(KeaLeases { leases }))
}

/// `expires_at = cltt + valid_lft` 轉 ISO 8601 UTC；任一缺欄位或溢位為 null。
fn expires_at(cltt: Option<i64>, valid_lft: Option<i64>) -> Option<String> {
    let expires = cltt?.checked_add(valid_lft?)?;
    chrono::DateTime::from_timestamp(expires, 0)
        .map(|time| time.format("%Y-%m-%dT%H:%M:%SZ").to_string())
}

async fn sync_plan(State(state): State<AppState>) -> Result<Json<SyncPlan>, ApiError> {
    let plan = sync::plan(&state.db, client(&state, "無法同步")?).await?;
    Ok(Json(plan))
}

async fn apply_sync(State(state): State<AppState>) -> Result<Json<SyncApplyReport>, ApiError> {
    let report = sync::apply(&state.db, client(&state, "無法同步")?).await?;
    Ok(Json(report))
}
