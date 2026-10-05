//! Kea 真機連線測試：對 `KEA_API_URL` 送 `version-get`、狀態唯讀命令與保留
//! roundtrip（見 `docs/adr/0010`、`docs/adr/0011`）。
//!
//! 預設忽略（需真機伺服器）；執行：`pnpm test:kea`。
//! roundtrip 會新增後刪除同一筆測試保留（不呼叫 config-write，不留持久變更）。

use std::net::Ipv4Addr;

use asset_nest::config::Config;
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

    // 取第一個 Kea IPv4 網段，測試位址用該網段最後一個可用主機位址附近（冷門）。
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
    let test_ip = Ipv4Addr::from(u32::from(network.broadcast()) - 1).to_string();

    let hosts = client
        .reservation_get_all(subnet_id)
        .await
        .expect("reservation-get-all 失敗");
    assert!(
        !hosts
            .iter()
            .any(|host| host.ip_address.as_deref() == Some(test_ip.as_str())),
        "測試位址 {test_ip} 在 Kea 已有保留；請更換測試位址"
    );

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
