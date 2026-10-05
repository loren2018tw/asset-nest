//! `/api/v1` Kea 路由（見 `docs/adr/0010`、`docs/adr/0011`）。
//!
//! `GET /kea/status`：連線診斷；一律 200、分區容錯（見票 01）。
//! `GET /kea/sync/plan`：完整同步計畫（dry-run、唯讀）。
//! `POST /kea/sync`：重算計畫並套用；回傳每網段增／改／刪計數、失敗清單與
//! `config-write` 狀態。`KEA_API_URL` 未設定時回 400。

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
        .route("/kea/status", get(status))
        .route("/kea/sync/plan", get(sync_plan))
        .route("/kea/sync", post(apply_sync))
}

/// 取得 Kea client；未設定時回 400（前端據訊息提示；同步端點用）。
fn client(state: &AppState) -> Result<&Client, ApiError> {
    state
        .kea
        .as_ref()
        .ok_or_else(|| ApiError::validation("Kea 未設定（KEA_API_URL），無法同步"))
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

async fn sync_plan(State(state): State<AppState>) -> Result<Json<SyncPlan>, ApiError> {
    let plan = sync::plan(&state.db, client(&state)?).await?;
    Ok(Json(plan))
}

async fn apply_sync(State(state): State<AppState>) -> Result<Json<SyncApplyReport>, ApiError> {
    let report = sync::apply(&state.db, client(&state)?).await?;
    Ok(Json(report))
}
