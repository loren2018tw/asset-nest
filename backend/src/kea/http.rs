//! Kea 內建 HTTP 控制通道 client（見 `docs/adr/0010`）。
//!
//! 命令以 JSON POST 至控制通道端點；回應為單元素陣列，認證失敗等為單一物件，
//! `result` 0 表示成功、3 表示空結果（Kea ARM 18.2）。

use std::collections::HashMap;
use std::time::Duration;

use serde::Serialize;
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

/// `config-get` 的 DHCPv4 設定摘要。
///
/// 取 `Dhcp4.interfaces-config.interfaces`（Kea 實際監聽的作業系統介面；
/// 空清單＝不主動服務 DHCP）、`Dhcp4.lease-database.type`（租約庫類型）與
/// `Dhcp4.subnet4`（subnet-id → CIDR）。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Dhcp4Config {
    pub interfaces: Vec<String>,
    pub lease_backend: Option<String>,
    pub subnets: HashMap<i64, String>,
}

/// `status-get` 的 socket 狀態；Kea 3.2.1 實測回物件（如 `{"status":"ready"}`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SocketStatus {
    /// 控制通道 socket 狀態（如 `ready`）；未提供為 `None`。
    pub status: Option<String>,
}

/// `status-get` 的伺服器運行資訊。
///
/// 真機 3.2.1 實測：`uptime`／`reload` 為相對秒數（非 epoch 時間）、
/// `sockets` 為狀態物件而非綁定清單；缺欄位一律 `None`。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct StatusInfo {
    pub pid: Option<i64>,
    /// 伺服器啟動後經過秒數（相對值）。
    pub uptime: Option<i64>,
    /// 距上次設定重載秒數（相對值）。
    pub reload: Option<i64>,
    /// socket 狀態；伺服端未提供為 `None`。
    pub sockets: Option<SocketStatus>,
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

/// `lease4-get-all` 的一筆 DHCPv4 動態租約；空字串視為未提供。
///
/// `cltt`／`valid_lft` 為秒數（`cltt` 為 Unix epoch）；`state` 已正規化：
/// 數字 0／1／2／3 → `default`／`declined`／`expired`／`released`、文字小寫、
/// 未知保留原值；缺欄位為 `None`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeaLease {
    pub ip_address: Option<String>,
    pub hw_address: Option<String>,
    pub hostname: Option<String>,
    pub subnet_id: Option<i64>,
    /// 租約開始時間（Unix epoch 秒）。
    pub cltt: Option<i64>,
    /// 租約有效期（秒）。
    pub valid_lft: Option<i64>,
    pub state: Option<String>,
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

    /// 顯示用連線位址：去掉 userinfo（認證資訊）與結尾斜線。
    pub fn base_url(&self) -> String {
        let mut url = self.base_url.clone();
        let _ = url.set_username("");
        let _ = url.set_password(None);
        url.as_str().trim_end_matches('/').to_string()
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

    /// 讀取 Kea 的 DHCPv4 設定摘要（`config-get` → `Dhcp4`）。
    ///
    /// 取監聽介面、租約庫類型與 `subnet4`（subnet-id → CIDR）；皆為唯讀。
    pub async fn config_get_dhcp4(&self) -> Result<Dhcp4Config, KeaError> {
        let outcome = self.command("config-get", None).await?;
        let outcome = expect_success(outcome)?;
        let dhcp4 = outcome
            .arguments
            .as_ref()
            .and_then(|arguments| arguments.get("Dhcp4"));

        let interfaces = dhcp4
            .and_then(|dhcp4| dhcp4.get("interfaces-config"))
            .and_then(|interfaces_config| interfaces_config.get("interfaces"))
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();

        let lease_backend = dhcp4
            .and_then(|dhcp4| dhcp4.get("lease-database"))
            .and_then(|lease_database| lease_database.get("type"))
            .and_then(Value::as_str)
            .map(str::to_string);

        let mut subnets = HashMap::new();
        if let Some(entries) = dhcp4
            .and_then(|dhcp4| dhcp4.get("subnet4"))
            .and_then(Value::as_array)
        {
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

        Ok(Dhcp4Config {
            interfaces,
            lease_backend,
            subnets,
        })
    }

    /// 讀取 Kea 運行狀態（`status-get`）：pid／uptime／reload／sockets。
    ///
    /// 欄位以 Kea 3.2.1 真機實測為準（見 `.scratch/kea-pages/spec.md`）：
    /// `uptime`／`reload` 為相對秒數，`sockets` 為狀態物件（如
    /// `{"status":"ready"}`）而非綁定清單；缺欄位為 `None`。
    pub async fn status_get(&self) -> Result<StatusInfo, KeaError> {
        let outcome = self.command("status-get", None).await?;
        let outcome = expect_success(outcome)?;
        let arguments = outcome.arguments.as_ref();

        let number = |key: &str| {
            arguments
                .and_then(|arguments| arguments.get(key))
                .and_then(Value::as_i64)
        };

        let sockets = arguments
            .and_then(|arguments| arguments.get("sockets"))
            .and_then(Value::as_object)
            .map(|sockets| SocketStatus {
                status: sockets
                    .get("status")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            });

        Ok(StatusInfo {
            pid: number("pid"),
            uptime: number("uptime"),
            reload: number("reload"),
            sockets,
        })
    }

    /// 列出 Kea 全部 DHCPv4 動態租約（`lease4-get-all`；唯讀，不帶 arguments）。
    ///
    /// `result` 3（空）視為空清單（比照 [`Client::reservation_get_all`]）。
    pub async fn lease4_get_all(&self) -> Result<Vec<KeaLease>, KeaError> {
        let outcome = self.command("lease4-get-all", None).await?;

        match outcome.result {
            0 => {
                let leases = outcome
                    .arguments
                    .as_ref()
                    .and_then(|arguments| arguments.get("leases"))
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                Ok(leases.iter().map(parse_lease).collect())
            }
            3 => Ok(Vec::new()),
            result => Err(response_error(result, outcome.text)),
        }
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

    /// 讀取 Kea 設定的 IPv4 網段（重用 [`Client::config_get_dhcp4`] 的 `subnet4`）。
    ///
    /// 回傳 `subnet-id → CIDR`；供完整同步檢查受管網段是否存在、CIDR 是否相符。
    /// 行為與過往 `config-get` 直取一致。
    pub async fn kea_subnets(&self) -> Result<HashMap<i64, String>, KeaError> {
        Ok(self.config_get_dhcp4().await?.subnets)
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

/// 解析一筆 Kea 租約；空字串視為未提供（比照 [`parse_host`]）。
///
/// 數值欄位接受 JSON 數字或數字字串（不同版本／後端可能給字串）。
fn parse_lease(lease: &Value) -> KeaLease {
    let text = |key: &str| {
        lease
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let number = |key: &str| {
        lease.get(key).and_then(|value| match value {
            Value::Number(number) => number.as_i64(),
            Value::String(text) => {
                let text = text.trim();
                if text.is_empty() {
                    None
                } else {
                    text.parse::<i64>().ok()
                }
            }
            _ => None,
        })
    };

    KeaLease {
        ip_address: text("ip-address"),
        hw_address: text("hw-address"),
        hostname: text("hostname"),
        subnet_id: number("subnet-id"),
        cltt: number("cltt"),
        valid_lft: number("valid-lft"),
        state: normalize_state(lease.get("state")),
    }
}

/// `state` 正規化：數字 0／1／2／3 → `default`／`declined`／`expired`／`released`；
/// 文字原樣小寫；未知保留原值；缺欄位為 `None`。
fn normalize_state(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::Number(number) => Some(match number.as_i64() {
            Some(0) => "default".to_string(),
            Some(1) => "declined".to_string(),
            Some(2) => "expired".to_string(),
            Some(3) => "released".to_string(),
            _ => number.to_string(),
        }),
        Value::String(text) => Some(text.to_lowercase()),
        other => Some(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_strips_userinfo_and_trailing_slashes() {
        let client = Client::new("http://admin:secret@127.0.0.1:8000/".parse().expect("URL"));
        assert_eq!(client.base_url(), "http://127.0.0.1:8000");

        let client = Client::new("http://127.0.0.1:8000".parse().expect("URL"));
        assert_eq!(client.base_url(), "http://127.0.0.1:8000");

        let client = Client::new("http://127.0.0.1:8000/kea///".parse().expect("URL"));
        assert_eq!(client.base_url(), "http://127.0.0.1:8000/kea");
    }

    #[test]
    fn parse_lease_normalizes_fields_and_state() {
        let lease = json!({
            "ip-address": "10.0.0.5",
            "hw-address": "aa:bb:cc:dd:ee:ff",
            "hostname": "  ",
            "subnet-id": "1",
            "cltt": 1791201600,
            "valid-lft": 3600,
            "state": 2,
        });

        let parsed = parse_lease(&lease);

        assert_eq!(parsed.ip_address.as_deref(), Some("10.0.0.5"));
        assert_eq!(parsed.hw_address.as_deref(), Some("aa:bb:cc:dd:ee:ff"));
        assert_eq!(parsed.hostname, None, "空字串／空白視為未提供");
        assert_eq!(parsed.subnet_id, Some(1), "數字字串可解析");
        assert_eq!(parsed.cltt, Some(1791201600));
        assert_eq!(parsed.valid_lft, Some(3600));
        assert_eq!(parsed.state.as_deref(), Some("expired"));
    }

    #[test]
    fn parse_lease_missing_fields_are_none() {
        let parsed = parse_lease(&json!({}));

        assert_eq!(
            parsed,
            KeaLease {
                ip_address: None,
                hw_address: None,
                hostname: None,
                subnet_id: None,
                cltt: None,
                valid_lft: None,
                state: None,
            }
        );
    }

    #[test]
    fn normalize_state_handles_numbers_strings_and_unknown() {
        assert_eq!(normalize_state(Some(&json!(0))).as_deref(), Some("default"));
        assert_eq!(
            normalize_state(Some(&json!(1))).as_deref(),
            Some("declined")
        );
        assert_eq!(normalize_state(Some(&json!(2))).as_deref(), Some("expired"));
        assert_eq!(
            normalize_state(Some(&json!(3))).as_deref(),
            Some("released")
        );
        assert_eq!(
            normalize_state(Some(&json!(9))).as_deref(),
            Some("9"),
            "未知數字保留原值"
        );
        assert_eq!(
            normalize_state(Some(&json!("DECLINED"))).as_deref(),
            Some("declined"),
            "文字原樣小寫"
        );
        assert_eq!(normalize_state(None), None);
    }
}
