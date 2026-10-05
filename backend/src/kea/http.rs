//! Kea 內建 HTTP 控制通道 client（見 `docs/adr/0010`）。
//!
//! 命令以 JSON POST 至控制通道端點；回應為單元素陣列，認證失敗等為單一物件，
//! `result` 0 表示成功、3 表示空結果（Kea ARM 18.2）。

use std::collections::HashMap;
use std::time::Duration;

use serde_json::{Value, json};

use super::KeaError;

/// 請求逾時：Kea 控制通道伺服端 10 秒關閉連線，取更短的 5 秒。
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// 修改命令的資料來源（`operation-target`）：一律操作 Kea 執行中設定。
///
/// host_cmds 的修改命令（add／del／update）預設只寫「主機資料庫」，
/// 未設定主機資料庫時會回「Host database not available」；本系統不使用
/// 主機資料庫，顯式指定 `memory` 後再以 `config-write` 持久化（見 ADR-0011）。
const OPERATION_TARGET: &str = "memory";

/// Kea 版本資訊（`version-get` 成功回應）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionInfo {
    /// 回應的 `text` 欄位（版本描述）。
    pub text: Option<String>,
    /// 回應 `arguments.version`（如 `3.0.3`）；伺服端未提供時為 `None`。
    pub version: Option<String>,
}

impl std::fmt::Display for VersionInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.version, &self.text) {
            (Some(version), Some(text)) => write!(f, "{version}（{text}）"),
            (Some(version), None) => write!(f, "{version}"),
            (None, Some(text)) => write!(f, "{text}"),
            (None, None) => write!(f, "（未提供版本資訊）"),
        }
    }
}

/// `reservation-get-all` 的一筆主機保留；只取本系統可比對的欄位。
///
/// 非 `hw-address` 形式的保留（circuit-id 等）`hw_address`／`ip_address` 為 `None`，
/// 由完整同步列入「跳過」報告、不觸碰（見 `docs/adr/0011`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeaHost {
    pub ip_address: Option<String>,
    pub hw_address: Option<String>,
    pub hostname: Option<String>,
}

/// 要推送的保留內容（IP＋MAC＋選填 hostname）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReservationRecord {
    pub ip_address: String,
    pub hw_address: String,
    pub hostname: Option<String>,
}

/// 命令回應（`result`／`text`／`arguments`）；`result` 由呼叫端解讀。
#[derive(Debug, Clone)]
struct CommandResult {
    result: i64,
    text: Option<String>,
    arguments: Option<Value>,
}

/// HTTP 控制通道 client。
#[derive(Debug, Clone)]
pub struct Client {
    base_url: reqwest::Url,
    http: reqwest::Client,
    basic_auth: Option<(String, String)>,
}

impl Client {
    /// 以控制通道位址建立 client（逾時 5 秒、不帶認證）。
    pub fn new(base_url: reqwest::Url) -> Self {
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .expect("建立 HTTP client");
        Self {
            base_url,
            http,
            basic_auth: None,
        }
    }

    /// 附加 HTTP Basic 認證（Kea 控制通道啟用 `authentication` 時使用）。
    pub fn with_basic_auth(mut self, username: String, password: String) -> Self {
        self.basic_auth = Some((username, password));
        self
    }

    /// 送 `version-get`：確認連得上並取得 Kea 版本資訊。
    pub async fn version_get(&self) -> Result<VersionInfo, KeaError> {
        let outcome = self.command("version-get", None).await?;
        let outcome = expect_success(outcome)?;
        let arguments = outcome.arguments.unwrap_or(Value::Null);
        Ok(VersionInfo {
            text: outcome.text,
            version: arguments
                .get("version")
                .and_then(Value::as_str)
                .map(str::to_string),
        })
    }

    /// 列出某 subnet-id 的全部主機保留（`reservation-get-all`）。
    ///
    /// `result` 3（空）視為空清單（Kea 對 0 筆保留回 3，見 3.2 實測）。
    pub async fn reservation_get_all(&self, subnet_id: i64) -> Result<Vec<KeaHost>, KeaError> {
        let outcome = self
            .command(
                "reservation-get-all",
                Some(json!({ "subnet-id": subnet_id, "operation-target": OPERATION_TARGET })),
            )
            .await?;

        match outcome.result {
            0 => {
                let hosts = outcome
                    .arguments
                    .as_ref()
                    .and_then(|arguments| arguments.get("hosts"))
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                Ok(hosts.iter().map(parse_host).collect())
            }
            3 => Ok(Vec::new()),
            result => Err(response_error(result, outcome.text)),
        }
    }

    /// 新增一筆主機保留（`reservation-add`）。
    pub async fn reservation_add(
        &self,
        subnet_id: i64,
        record: &ReservationRecord,
    ) -> Result<(), KeaError> {
        let outcome = self
            .command(
                "reservation-add",
                Some(json!({
                    "reservation": reservation_body(subnet_id, record),
                    "operation-target": OPERATION_TARGET,
                })),
            )
            .await?;
        expect_success(outcome).map(|_| ())
    }

    /// 刪除一筆主機保留（`reservation-del`，以 subnet-id＋ip-address 識別）。
    ///
    /// 回傳是否確實刪除；`result` 3（不存在）視為已刪除、回 `false`（冪等）。
    pub async fn reservation_del(
        &self,
        subnet_id: i64,
        ip_address: &str,
    ) -> Result<bool, KeaError> {
        let outcome = self
            .command(
                "reservation-del",
                Some(json!({ "subnet-id": subnet_id, "ip-address": ip_address, "operation-target": OPERATION_TARGET })),
            )
            .await?;

        match outcome.result {
            0 => Ok(true),
            3 => Ok(false),
            result => Err(response_error(result, outcome.text)),
        }
    }

    /// 將執行中設定寫回 Kea 設定檔（`config-write`），讓推送的保留活過重啟。
    pub async fn config_write(&self) -> Result<(), KeaError> {
        let outcome = self.command("config-write", None).await?;
        expect_success(outcome).map(|_| ())
    }

    /// 讀取 Kea 設定的 IPv4 網段（`config-get` → `Dhcp4.subnet4`）。
    ///
    /// 回傳 `subnet-id → CIDR`；供完整同步檢查受管網段是否存在、CIDR 是否相符。
    pub async fn kea_subnets(&self) -> Result<HashMap<i64, String>, KeaError> {
        let outcome = self.command("config-get", None).await?;
        let outcome = expect_success(outcome)?;

        let mut subnets = HashMap::new();
        let entries = outcome
            .arguments
            .as_ref()
            .and_then(|arguments| arguments.get("Dhcp4"))
            .and_then(|dhcp4| dhcp4.get("subnet4"))
            .and_then(Value::as_array);

        if let Some(entries) = entries {
            for entry in entries {
                if let Some(id) = entry.get("id").and_then(Value::as_i64) {
                    let cidr = entry
                        .get("subnet")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    subnets.insert(id, cidr);
                }
            }
        }

        Ok(subnets)
    }

    /// 送一個命令並解析回應（陣列取首元素；認證失敗等單一物件直接使用）。
    async fn command(
        &self,
        command: &str,
        arguments: Option<Value>,
    ) -> Result<CommandResult, KeaError> {
        let mut payload = json!({ "command": command });
        if let Some(arguments) = arguments {
            payload["arguments"] = arguments;
        }

        let mut request = self.http.post(self.endpoint()).json(&payload);
        if let Some((username, password)) = &self.basic_auth {
            request = request.basic_auth(username, Some(password));
        }

        let response = request.send().await?;
        let status = response.status();
        let body = response.text().await?;
        let value: Value = serde_json::from_str(&body)
            .map_err(|e| KeaError::Malformed(format!("非 JSON 回應（HTTP {status}）：{e}")))?;

        // 正常／錯誤皆包成單元素陣列；認證失敗等為單一物件（Kea ARM 18.2）。
        let entry = match value {
            Value::Array(items) => items
                .into_iter()
                .next()
                .ok_or_else(|| KeaError::Malformed("回應為空陣列".to_string()))?,
            other @ Value::Object(_) => other,
            _ => {
                return Err(KeaError::Malformed(
                    "回應既非單元素陣列也非單一物件".to_string(),
                ));
            }
        };

        let result = entry
            .get("result")
            .and_then(Value::as_i64)
            .ok_or_else(|| KeaError::Malformed("回應缺少 result 欄位".to_string()))?;
        let text = entry
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_string);
        let arguments = entry.get("arguments").cloned();

        Ok(CommandResult {
            result,
            text,
            arguments,
        })
    }

    /// 控制通道端點：基底 URL 路徑以 `/` 結尾（未帶路徑者即根路徑）。
    fn endpoint(&self) -> reqwest::Url {
        let mut url = self.base_url.clone();
        if !url.path().ends_with('/') {
            url.set_path(&format!("{}/", url.path()));
        }
        url
    }
}

/// `result` 0 以外一律視為錯誤（保留原始 result 與 text）。
fn expect_success(outcome: CommandResult) -> Result<CommandResult, KeaError> {
    if outcome.result == 0 {
        Ok(outcome)
    } else {
        Err(response_error(outcome.result, outcome.text))
    }
}

fn response_error(result: i64, text: Option<String>) -> KeaError {
    KeaError::Response {
        result,
        text: text.unwrap_or_else(|| "（無訊息）".to_string()),
    }
}

/// 保留的 JSON 內容；hostname 未填時不帶欄位。
fn reservation_body(subnet_id: i64, record: &ReservationRecord) -> Value {
    let mut reservation = json!({
        "subnet-id": subnet_id,
        "ip-address": record.ip_address,
        "hw-address": record.hw_address,
    });
    if let Some(hostname) = &record.hostname {
        reservation["hostname"] = json!(hostname);
    }
    reservation
}

/// 解析一筆 Kea host；空字串視為未提供。
fn parse_host(host: &Value) -> KeaHost {
    let text = |key: &str| {
        host.get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };

    KeaHost {
        ip_address: text("ip-address"),
        hw_address: text("hw-address"),
        hostname: text("hostname"),
    }
}
