//! 代理環境設定（systemd 由票 06 注入；開發時可直接設行程環境變數）。
//!
//! 變數（見 spec §觀測代理）：
//!
//! - `AGENT_SERVER_URL`、`AGENT_AUTH_CODE`、`AGENT_INSTANCE_ID`：必填。
//! - `AGENT_SUBNET_CIDR`：選填；空＝自動偵測本機介面（唯一非 loopback
//!   IPv4 網段才採用，多候選或找不到一律拒絕啟動並提示）。
//! - `AGENT_NAME`：選填；預設 hostname。
//! - `AGENT_SWEEP_INTERVAL_SECS`：選填；預設 900（首次掃描在間隔後）。
//! - `AGENT_SWEEP_RATE_PPS`：選填；預設 1000。
//!
//! 啟動驗證：解析後的網段必須有本機介面位址落在其中，否則無法探測，
//! 拒絕啟動並說明原因。

use std::net::Ipv4Addr;
use std::time::Duration;

use ipnet::Ipv4Net;

use crate::probe;

/// `AGENT_SWEEP_INTERVAL_SECS` 的預設值（15 分鐘；見 spec §觀測代理）。
pub const DEFAULT_SWEEP_INTERVAL_SECS: u64 = 900;
/// `AGENT_SWEEP_RATE_PPS` 的預設值（見 spec §觀測代理）。
pub const DEFAULT_SWEEP_RATE_PPS: u32 = 1_000;

/// 代理啟動設定。
#[derive(Debug, Clone)]
pub struct Config {
    /// 後端基底網址（已去除尾端 `/`；`AGENT_SERVER_URL`）。
    pub server_url: String,
    /// 代理入庫認證碼（`AGENT_AUTH_CODE`）。
    pub auth_code: String,
    /// 代理識別碼（`AGENT_INSTANCE_ID`；安裝時產生之 UUID）。
    pub instance_id: String,
    /// 自報名稱（`AGENT_NAME`；預設 hostname）。
    pub name: String,
    /// 要觀測的網段（已正規化為網路地址）。
    pub subnet: Ipv4Net,
    /// 主動掃描間隔（`AGENT_SWEEP_INTERVAL_SECS`；首次在間隔後）。
    pub sweep_interval: Duration,
    /// 主動掃描每秒送出的探測數上限（`AGENT_SWEEP_RATE_PPS`）。
    pub sweep_rate_pps: u32,
}

impl Config {
    /// 由行程環境變數讀取設定。
    pub fn from_env() -> anyhow::Result<Self> {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// 由任意取值函式讀取設定（單元測試注入用）。
    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> anyhow::Result<Self> {
        let server_url = normalize_server_url(&required(&lookup, "AGENT_SERVER_URL")?)?;
        let auth_code = required(&lookup, "AGENT_AUTH_CODE")?;
        let instance_id = required(&lookup, "AGENT_INSTANCE_ID")?;

        let subnet_cidr = non_empty(lookup("AGENT_SUBNET_CIDR"));
        let subnet = resolve_subnet(subnet_cidr.as_deref(), &probe::interface_networks())?;
        ensure_subnet_local(&subnet, &probe::local_ipv4_addresses())?;

        let name = non_empty(lookup("AGENT_NAME")).unwrap_or_else(hostname);
        let sweep_interval_secs = match non_empty(lookup("AGENT_SWEEP_INTERVAL_SECS")) {
            Some(raw) => parse_positive_u64(&raw, "AGENT_SWEEP_INTERVAL_SECS")?,
            None => DEFAULT_SWEEP_INTERVAL_SECS,
        };
        let sweep_rate_pps = match non_empty(lookup("AGENT_SWEEP_RATE_PPS")) {
            Some(raw) => parse_positive_u32(&raw, "AGENT_SWEEP_RATE_PPS")?,
            None => DEFAULT_SWEEP_RATE_PPS,
        };

        Ok(Self {
            server_url,
            auth_code,
            instance_id,
            name,
            subnet,
            sweep_interval: Duration::from_secs(sweep_interval_secs),
            sweep_rate_pps,
        })
    }
}

/// 必填設定：缺漏或全空白回錯誤並指明變數名稱。
fn required(lookup: &impl Fn(&str) -> Option<String>, variable: &str) -> anyhow::Result<String> {
    non_empty(lookup(variable)).ok_or_else(|| anyhow::anyhow!("{variable} 為必填"))
}

/// 正規化可選字串：缺漏、空字串或全空白皆視為未提供。
fn non_empty(value: Option<String>) -> Option<String> {
    value
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

/// 驗證並正規化後端網址：須為 http(s)，去除尾端 `/` 以便串接端點路徑。
fn normalize_server_url(raw: &str) -> anyhow::Result<String> {
    let url = reqwest::Url::parse(raw)
        .map_err(|error| anyhow::anyhow!("AGENT_SERVER_URL 格式錯誤：{raw}（{error}）"))?;
    if !matches!(url.scheme(), "http" | "https") {
        anyhow::bail!("AGENT_SERVER_URL 須為 http:// 或 https:// 開頭（收到 {raw}）");
    }
    Ok(url.to_string().trim_end_matches('/').to_string())
}

/// 解析掃描網段：明確值優先；空值＝自動偵測（去重後唯一候選才採用）。
fn resolve_subnet(explicit: Option<&str>, candidates: &[Ipv4Net]) -> anyhow::Result<Ipv4Net> {
    if let Some(raw) = explicit {
        return raw
            .trim()
            .parse::<Ipv4Net>()
            .map(|network| network.trunc())
            .map_err(|_| {
                anyhow::anyhow!(
                    "AGENT_SUBNET_CIDR 格式錯誤：{raw}（須為 IPv4 CIDR，例：10.1.0.0/24）"
                )
            });
    }

    let mut unique: Vec<Ipv4Net> = candidates.to_vec();
    unique.sort_by_key(|network| (network.network(), network.prefix_len()));
    unique.dedup();

    match unique.as_slice() {
        [] => anyhow::bail!(
            "未設定 AGENT_SUBNET_CIDR，且找不到可用的非 loopback IPv4 網段；請明確設定 AGENT_SUBNET_CIDR"
        ),
        [only] => Ok(*only),
        many => {
            let list = many
                .iter()
                .map(|network| network.to_string())
                .collect::<Vec<_>>()
                .join("、");
            anyhow::bail!(
                "未設定 AGENT_SUBNET_CIDR，且偵測到多個候選網段（{list}）；請明確設定 AGENT_SUBNET_CIDR"
            )
        }
    }
}

/// 啟動驗證：網段內必須有本機介面位址，否則無法探測。
fn ensure_subnet_local(subnet: &Ipv4Net, addresses: &[Ipv4Addr]) -> anyhow::Result<()> {
    if addresses.iter().any(|address| subnet.contains(address)) {
        return Ok(());
    }
    anyhow::bail!(
        "本機沒有任何介面位址落在網段 {subnet} 內，無法探測；請確認 AGENT_SUBNET_CIDR 或介面設定"
    )
}

/// 解析正整數秒：0 與非數字皆拒絕，錯誤訊息指明變數。
fn parse_positive_u64(raw: &str, variable: &str) -> anyhow::Result<u64> {
    let value: u64 = raw
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("{variable} 格式錯誤：{raw}（須為正整數秒數）"))?;
    if value == 0 {
        anyhow::bail!("{variable} 須為正整數（收到 0）");
    }
    Ok(value)
}

/// 解析正整數（每秒探測數）：0 與非數字皆拒絕，錯誤訊息指明變數。
fn parse_positive_u32(raw: &str, variable: &str) -> anyhow::Result<u32> {
    let value: u32 = raw
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("{variable} 格式錯誤：{raw}（須為正整數）"))?;
    if value == 0 {
        anyhow::bail!("{variable} 須為正整數（收到 0）");
    }
    Ok(value)
}

/// 取得 hostname（Linux `gethostname(2)`；失敗時退回固定名稱）。
fn hostname() -> String {
    #[cfg(target_os = "linux")]
    {
        let mut buffer = [0u8; 256];
        // SAFETY: gethostname 最多寫入 buffer 長度位元組；buffer 為本函式持有。
        let result = unsafe {
            libc::gethostname(
                buffer.as_mut_ptr() as *mut libc::c_char,
                buffer.len() as libc::size_t,
            )
        };
        if result == 0 {
            let end = buffer
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(buffer.len());
            if let Ok(name) = std::str::from_utf8(&buffer[..end])
                && !name.is_empty()
            {
                return name.to_string();
            }
        }
    }

    "asset-nest-agent".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 測試用候選網段。
    fn network(text: &str) -> Ipv4Net {
        text.parse().expect("合法網段")
    }

    #[test]
    fn resolve_subnet_normalizes_explicit_cidr() {
        let resolved = resolve_subnet(Some("10.1.2.7/24"), &[]).expect("合法 CIDR");
        assert_eq!(resolved, network("10.1.2.0/24"), "host bits 收斂為網路地址");

        let error = resolve_subnet(Some("not-a-cidr"), &[])
            .expect_err("非法 CIDR")
            .to_string();
        assert!(
            error.contains("AGENT_SUBNET_CIDR"),
            "錯誤訊息須指明變數：{error}"
        );
    }

    #[test]
    fn resolve_subnet_auto_detects_single_candidate() {
        let candidates = [network("192.168.1.0/24")];
        assert_eq!(
            resolve_subnet(None, &candidates).expect("唯一候選"),
            network("192.168.1.0/24")
        );
    }

    #[test]
    fn resolve_subnet_auto_deduplicates_candidates() {
        let candidates = [network("192.168.1.0/24"), network("192.168.1.0/24")];
        assert_eq!(
            resolve_subnet(None, &candidates).expect("同網段去重後唯一"),
            network("192.168.1.0/24")
        );
    }

    #[test]
    fn resolve_subnet_auto_rejects_missing_and_ambiguous() {
        let error = resolve_subnet(None, &[])
            .expect_err("找不到候選應拒絕")
            .to_string();
        assert!(
            error.contains("AGENT_SUBNET_CIDR"),
            "錯誤訊息須提示明確設定：{error}"
        );

        let candidates = [network("192.168.1.0/24"), network("10.0.0.0/16")];
        let error = resolve_subnet(None, &candidates)
            .expect_err("多候選應拒絕")
            .to_string();
        assert!(
            error.contains("192.168.1.0/24") && error.contains("10.0.0.0/16"),
            "錯誤訊息須列出候選：{error}"
        );
        assert!(
            error.contains("AGENT_SUBNET_CIDR"),
            "錯誤訊息須提示明確設定：{error}"
        );
    }

    #[test]
    fn ensure_subnet_local_requires_address_inside_network() {
        let subnet = network("10.0.0.0/24");
        assert!(
            ensure_subnet_local(&subnet, &["10.0.0.5".parse().expect("位址")]).is_ok(),
            "網段內位址通過"
        );

        let error = ensure_subnet_local(&subnet, &["10.0.1.5".parse().expect("位址")])
            .expect_err("網段外位址應拒絕")
            .to_string();
        assert!(error.contains("10.0.0.0/24"), "錯誤訊息須說明網段：{error}");
    }

    #[test]
    fn required_variables_missing_report_variable_name() {
        for variable in ["AGENT_SERVER_URL", "AGENT_AUTH_CODE", "AGENT_INSTANCE_ID"] {
            let error = required(&|_| None, variable)
                .expect_err("缺漏應拒絕")
                .to_string();
            assert!(error.contains(variable), "錯誤訊息須指明變數：{error}");
        }

        let error = required(&|_| Some("   ".to_string()), "AGENT_AUTH_CODE")
            .expect_err("全空白應視為未提供")
            .to_string();
        assert!(error.contains("AGENT_AUTH_CODE"), "{error}");
    }

    #[test]
    fn normalize_server_url_requires_http_and_strips_trailing_slash() {
        assert_eq!(
            normalize_server_url("http://nest.local:8080/").expect("合法網址"),
            "http://nest.local:8080"
        );
        assert_eq!(
            normalize_server_url("https://nest.local").expect("合法網址"),
            "https://nest.local"
        );

        for invalid in ["ftp://nest.local", "nest.local:8080", ""] {
            assert!(normalize_server_url(invalid).is_err(), "應拒絕 {invalid:?}");
        }
    }

    #[test]
    fn positive_integer_parsers_reject_zero_and_non_numeric() {
        assert_eq!(parse_positive_u64("900", "X").expect("合法秒數"), 900);
        assert_eq!(parse_positive_u64(" 60 ", "X").expect("容許空白"), 60);
        assert_eq!(parse_positive_u32("1000", "X").expect("合法速率"), 1_000);

        for invalid in ["0", "-1", "abc", "1.5", ""] {
            let variable = "AGENT_SWEEP_INTERVAL_SECS";
            let error = parse_positive_u64(invalid, variable)
                .expect_err(&format!("應拒絕 {invalid:?}"))
                .to_string();
            assert!(error.contains(variable), "錯誤訊息須指明變數：{error}");
        }

        for invalid in ["0", "-1", "abc", "1.5", ""] {
            let variable = "AGENT_SWEEP_RATE_PPS";
            let error = parse_positive_u32(invalid, variable)
                .expect_err(&format!("應拒絕 {invalid:?}"))
                .to_string();
            assert!(error.contains(variable), "錯誤訊息須指明變數：{error}");
        }
    }

    #[test]
    fn defaults_are_fifteen_minutes_and_1000_pps() {
        assert_eq!(DEFAULT_SWEEP_INTERVAL_SECS, 900);
        assert_eq!(DEFAULT_SWEEP_RATE_PPS, 1_000);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn from_lookup_applies_defaults_with_explicit_loopback_subnet() {
        let config = Config::from_lookup(|name| match name {
            "AGENT_SERVER_URL" => Some("http://127.0.0.1:8080".to_string()),
            "AGENT_AUTH_CODE" => Some("secret".to_string()),
            "AGENT_INSTANCE_ID" => Some("11111111-1111-1111-1111-111111111111".to_string()),
            // loopback 必有本機位址，可離線驗證設定解析與預設值。
            "AGENT_SUBNET_CIDR" => Some("127.0.0.0/8".to_string()),
            _ => None,
        })
        .expect("合法設定");

        assert_eq!(config.server_url, "http://127.0.0.1:8080");
        assert_eq!(config.subnet, network("127.0.0.0/8"));
        assert_eq!(config.sweep_interval, Duration::from_secs(900));
        assert_eq!(config.sweep_rate_pps, 1_000);
        assert!(!config.name.is_empty(), "名稱預設 hostname");
    }

    #[test]
    fn from_lookup_rejects_subnet_without_local_address() {
        let error = Config::from_lookup(|name| match name {
            "AGENT_SERVER_URL" => Some("http://127.0.0.1:8080".to_string()),
            "AGENT_AUTH_CODE" => Some("secret".to_string()),
            "AGENT_INSTANCE_ID" => Some("11111111-1111-1111-1111-111111111111".to_string()),
            // TEST-NET-1 保留位址，本機不可能有介面落在其中。
            "AGENT_SUBNET_CIDR" => Some("192.0.2.0/24".to_string()),
            _ => None,
        })
        .expect_err("網段內無本機位址應拒絕啟動")
        .to_string();

        assert!(
            error.contains("192.0.2.0/24"),
            "錯誤訊息須說明網段：{error}"
        );
    }
}
