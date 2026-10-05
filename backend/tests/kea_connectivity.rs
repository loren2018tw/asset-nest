//! Kea 真機連線測試：對 `KEA_API_URL` 送 `version-get`、狀態唯讀命令與保留
//! roundtrip（見 `docs/adr/0010`、`docs/adr/0011`）。
//!
//! 預設忽略（需真機伺服器）；執行：`pnpm test:kea`。
//! roundtrip 會新增後刪除同一筆測試保留（不呼叫 config-write，不留持久變更）。

use std::net::Ipv4Addr;

use asset_nest::config::Config;
use asset_nest::kea::KeaError;
use asset_nest::kea::http::{Client, ReservationRecord};

/// 建立帶 `.env` 認證的 client 與位址。
fn live_client(config: &Config) -> (Client, reqwest::Url) {
    let url = config
        .kea_api_url
        .clone()
        .expect("KEA_API_URL 未設定：複製 .env.example 為 .env，並填入 Kea 控制通道位址");

    let mut client = Client::new(url.clone());
    if let Some(username) = config.kea_api_username.clone() {
        let password = config.kea_api_password.clone().unwrap_or_default();
        client = client.with_basic_auth(username, password);
    }

    (client, url)
}

#[tokio::test]
#[ignore = "需要真機 Kea（KEA_API_URL）"]
async fn version_get_against_live_server() {
    dotenvy::dotenv().ok();

    let config = Config::from_env().expect("讀取環境設定失敗");
    let (client, url) = live_client(&config);

    match client.version_get().await {
        Ok(info) => println!("Kea 連線成功：{info}"),
        Err(err) => panic!("Kea 連線測試失敗（{url}）：{err}"),
    }
}

/// 對真機做保留 roundtrip：add → 讀回驗證 → del；驗證 Kea 3.2 命令相容性。
#[tokio::test]
#[ignore = "需要真機 Kea（KEA_API_URL）"]
async fn reservation_roundtrip_against_live_server() {
    const TEST_MAC: &str = "02:00:5e:00:53:01";

    dotenvy::dotenv().ok();

    let config = Config::from_env().expect("讀取環境設定失敗");
    let (client, url) = live_client(&config);

    // 取第一個 Kea IPv4 網段，測試位址由該網段 broadcast−1 往下找首個未被
    // 保留的位址（避開既有真實保留，如 140.128.179.254）。
    let subnets = client
        .kea_subnets()
        .await
        .expect("讀取 Kea 網段失敗（檢查認證與控制通道）");
    let Some((&subnet_id, cidr)) = subnets.iter().next() else {
        panic!("Kea（{url}）未設定任何 IPv4 網段，無法測試保留");
    };
    let network: ipnet::Ipv4Net = cidr
        .parse()
        .unwrap_or_else(|_| panic!("Kea 網段 CIDR 無效：{cidr}"));

    let hosts = client
        .reservation_get_all(subnet_id)
        .await
        .expect("reservation-get-all 失敗");

    let mut candidate = u32::from(network.broadcast()) - 1;
    let test_ip = loop {
        let ip = Ipv4Addr::from(candidate).to_string();
        if !hosts
            .iter()
            .any(|host| host.ip_address.as_deref() == Some(ip.as_str()))
        {
            break ip;
        }
        assert!(
            candidate > u32::from(network.network()) + 1,
            "Kea 網段 {cidr}（subnet-id {subnet_id}）內找不到未被保留的位址"
        );
        candidate -= 1;
    };
    println!("保留 roundtrip 測試位址：subnet-id {subnet_id}、{test_ip}");

    let record = ReservationRecord {
        ip_address: test_ip.clone(),
        hw_address: TEST_MAC.to_string(),
        hostname: Some("asset-nest-test".to_string()),
    };

    client
        .reservation_add(subnet_id, &record)
        .await
        .unwrap_or_else(|err| panic!("reservation-add（{test_ip}）失敗：{err}"));

    let hosts = client
        .reservation_get_all(subnet_id)
        .await
        .expect("reservation-get-all 失敗");
    assert!(
        hosts.iter().any(|host| {
            host.ip_address.as_deref() == Some(test_ip.as_str())
                && host.hw_address.as_deref() == Some(TEST_MAC)
                && host.hostname.as_deref() == Some("asset-nest-test")
        }),
        "新增後應可讀回測試保留"
    );

    client
        .reservation_del(subnet_id, &test_ip)
        .await
        .unwrap_or_else(|err| panic!("reservation-del（{test_ip}）失敗：{err}"));

    let hosts = client
        .reservation_get_all(subnet_id)
        .await
        .expect("reservation-get-all 失敗");
    assert!(
        !hosts
            .iter()
            .any(|host| host.ip_address.as_deref() == Some(test_ip.as_str())),
        "刪除後不應再有測試保留"
    );

    println!("Kea 保留 roundtrip 成功：subnet-id {subnet_id}、測試位址 {test_ip}");
}

/// 唯讀實測系統狀態相關命令：`version-get`、`config-get`（interfaces／subnet4）
/// 與 `status-get`；印出實測欄位供定案（見 `.scratch/kea-pages/spec.md`）。
/// 只呼叫讀取命令，不 `config-write`、不留任何變更。
#[tokio::test]
#[ignore = "需要真機 Kea（KEA_API_URL）"]
async fn status_commands_against_live_server() {
    dotenvy::dotenv().ok();

    let config = Config::from_env().expect("讀取環境設定失敗");
    let (client, url) = live_client(&config);

    println!("顯示用連線位址（base_url）：{}", client.base_url());

    let version = client.version_get().await.expect("version-get 失敗");
    println!(
        "version-get：version={:?}、text={:?}",
        version.version, version.text
    );

    let dhcp4 = client.config_get_dhcp4().await.expect("config-get 失敗");
    println!("config-get 監聽介面：{:?}", dhcp4.interfaces);
    println!("config-get 租約庫類型：{:?}", dhcp4.lease_backend);
    println!("config-get 網段：{:?}", dhcp4.subnets);

    let status = client.status_get().await.expect("status-get 失敗");
    println!("status-get：{status:#?}");

    println!("唯讀狀態實測完成（{url}）");
}

/// 唯讀實測網段層同步所需欄位：`config-get` 的 subnet4 pools／routers 解析，
/// 並以 `list-commands` 檢查 subnet_cmds hook 是否提供 `subnet4-update`
/// （未載入時照實記錄）。不送修改命令、不留變更（見 `docs/adr/0013`）。
#[tokio::test]
#[ignore = "需要真機 Kea（KEA_API_URL）"]
async fn subnet_settings_against_live_server() {
    dotenvy::dotenv().ok();

    let config = Config::from_env().expect("讀取環境設定失敗");
    let (client, url) = live_client(&config);

    let dhcp4 = client.config_get_dhcp4().await.expect("config-get 失敗");
    assert!(
        !dhcp4.subnets.is_empty(),
        "真機應至少一個 IPv4 網段（{url}）"
    );
    for (id, subnet) in &dhcp4.subnets {
        println!(
            "subnet-id {id}：cidr={:?}、pools={:?}、gateway={:?}",
            subnet.cidr,
            subnet
                .pools
                .iter()
                .map(|range| range.to_compact_string())
                .collect::<Vec<_>>(),
            subnet.gateway
        );
    }

    let commands = client.list_commands().await.expect("list-commands 失敗");
    if commands.iter().any(|command| command == "subnet4-update") {
        println!("subnet_cmds hook 已載入：subnet4-update 可用");
    } else {
        println!(
            "subnet4-update 不支援：此機 Kea 未載入 subnet_cmds hook；\
             載入後網段層同步（pool／gateway）才能套用（見 .scratch/kea-subnet-sync/issues/03）"
        );
    }
}

/// 唯讀實測 `lease4-get-all`：支援時逐筆斷言欄位解析合理（合法 IPv4、
/// MAC 形式 `hw_address`、`state` 在正規化集合內），並印出實測供定案
/// （見 `.scratch/kea-pages/spec.md`）。不送任何修改命令、不留變更。
#[tokio::test]
#[ignore = "需要真機 Kea（KEA_API_URL）"]
async fn lease4_get_all_against_live_server() {
    dotenvy::dotenv().ok();

    let config = Config::from_env().expect("讀取環境設定失敗");
    let (client, url) = live_client(&config);

    match client.lease4_get_all().await {
        Ok(leases) => {
            println!("lease4-get-all 共 {} 筆", leases.len());
            for lease in &leases {
                println!("{lease:#?}");

                assert!(
                    lease
                        .ip_address
                        .as_deref()
                        .and_then(|ip| ip.parse::<Ipv4Addr>().ok())
                        .is_some_and(|ip| ip != Ipv4Addr::UNSPECIFIED),
                    "租約 ip_address 非合法 IPv4：{lease:#?}"
                );
                assert!(
                    lease.hw_address.as_deref().is_some_and(is_mac_like),
                    "租約 hw_address 為空或格式不合理：{lease:#?}"
                );
                assert!(
                    lease.state.as_deref().is_none_or(|state| {
                        matches!(state, "default" | "declined" | "expired" | "released")
                    }),
                    "租約 state 非預期值：{lease:#?}"
                );
            }
            if leases.is_empty() {
                println!("（0 筆：memfile 尚無租約；client 對 result 0／3 皆收斂為空清單）");
            }
        }
        // 實測：Kea 未載入 lease_cmds（result 2）為環境設定限制而非連線失敗，
        // 照實記錄、不讓唯讀探測卡住；載入 hook 後走上方欄位斷言路徑。
        Err(KeaError::Response { result: 2, text }) => println!(
            "lease4-get-all 不支援（result 2：{text}）：此機 Kea 未載入 lease_cmds hook；\
             載入後本測試會改走欄位斷言（見 .scratch/kea-pages/issues/04）"
        ),
        Err(err) => panic!("lease4-get-all 失敗（{url}）：{err}"),
    }
}

/// 寬鬆 MAC 檢查：`:` 分隔的 6／8 組兩位十六進位（Kea `hw-address` 格式）。
fn is_mac_like(value: &str) -> bool {
    let octets: Vec<&str> = value.split(':').collect();
    matches!(octets.len(), 6 | 8)
        && octets
            .iter()
            .all(|octet| octet.len() == 2 && octet.chars().all(|c| c.is_ascii_hexdigit()))
}
