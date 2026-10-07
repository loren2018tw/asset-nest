//! Kea 真機連線測試：對 `KEA_API_URL` 送 `version-get`、狀態唯讀命令、保留
//! roundtrip 與完整同步建立缺少網段 roundtrip（見 `docs/adr/0010`、
//! `docs/adr/0011`、`docs/adr/0023`）。
//!
//! 預設忽略（需真機伺服器）；執行：`pnpm test:kea`。
//! 保留 roundtrip 會新增後刪除同一筆測試保留（不呼叫 config-write，不留持久變更）。
//! 網段建立 roundtrip 以 RFC 5737 測試段與未使用 subnet-id 建立後完整清理
//! （`reservation-del` → `subnet4-del` → `config-write`）；兩者皆變更執行中設定，
//! 以序列鎖互斥執行（見 [`LIVE_MUTATION_LOCK`]）。

use std::net::Ipv4Addr;

use asset_nest::config::Config;
use asset_nest::kea::KeaError;
use asset_nest::kea::http::{Client, ReservationRecord};
use asset_nest::kea::sync;
use serde_json::json;
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;

/// 真機「變更型」測試的序列鎖：`pnpm test:kea` 併行執行所有 ignored 測試；
/// 保留 roundtrip 與網段建立 roundtrip 皆會變更 Kea 執行中設定，序列化可避免
/// 網段建立測試的 `config-write` 把另一測試的暫時保留持久化到設定檔。
static LIVE_MUTATION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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

    // 變更執行中設定；與其他變更型真機測試互斥（見 LIVE_MUTATION_LOCK）。
    let _guard = LIVE_MUTATION_LOCK.lock().await;

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
                        matches!(
                            state,
                            "default"
                                | "declined"
                                | "expired-reclaimed"
                                | "released"
                                | "registered"
                        )
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

/// 建立測試用記憶體資料庫並套用 migrations（模式同 `tests/kea_sync.rs`）。
async fn test_pool() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .expect("建立記憶體資料庫");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("套用 migrations");

    pool
}

/// 真機清理用：直送 `subnet4-del`（`Client` 無此命令；不為測試擴大生產介面；
/// 見 `.scratch/kea-subnet-create/issues/03`）。回傳 Kea 的 `text`；非 result 0 為 Err。
async fn subnet4_del_against_live(
    url: &reqwest::Url,
    config: &Config,
    subnet_id: i64,
) -> Result<String, String> {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|error| error.to_string())?;
    let mut request = http.post(url.clone()).json(&json!({
        "command": "subnet4-del",
        "arguments": { "id": subnet_id },
    }));
    if let Some(username) = config.kea_api_username.clone() {
        request = request.basic_auth(username, config.kea_api_password.clone());
    }

    let response = request.send().await.map_err(|error| error.to_string())?;
    let body: serde_json::Value = response.json().await.map_err(|error| error.to_string())?;
    let entry = body.get(0).cloned().unwrap_or(serde_json::Value::Null);
    let result = entry
        .get("result")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(-1);
    let text = entry
        .get("text")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_string();

    if result == 0 {
        Ok(text)
    } else {
        Err(format!("result={result}：{text}"))
    }
}

/// 完整同步建立缺少受管網段的真機 roundtrip（見 `docs/adr/0023`、spec 票 03）：
/// 以 RFC 5737 測試段 `192.0.2.0/24` 與未使用的 subnet-id（掃描現有 id 後取
/// max+1）在臨時 SQLite DB 建立受管網段（gateway＋兩段 pool＋一筆保留指派）→
/// `sync::plan` 應含 `subnet_add` → `sync::apply` 應 `subnet_added` 且同回合推送
/// 保留 → 真機 `config-get`／`reservation-get-all` 驗證落地 → 重送
/// `subnet4-add` 記錄拒絕訊息形狀（不改變狀態）→ 清理（`reservation-del` →
/// `subnet4-del` → `config-write`）→ 驗證無殘留。僅動測試段；驗證值先收集、
/// 清理完成後才斷言（避免 assert panic 造成殘留）。測試段若已存在於 Kea
/// 或不支援建立，即停止不執行（絕不動既有網段、不造假）。
#[tokio::test]
#[ignore = "需要真機 Kea（KEA_API_URL）"]
async fn create_missing_subnet_roundtrip_against_live_server() {
    const TEST_CIDR: &str = "192.0.2.0/24";
    const TEST_GATEWAY: &str = "192.0.2.1";
    /// 兩段 pool（輸入順序非數值序，驗證計畫排序）。
    const TEST_POOLS: [(&str, &str); 2] =
        [("192.0.2.30", "192.0.2.40"), ("192.0.2.10", "192.0.2.20")];
    const TEST_IP: &str = "192.0.2.5";
    /// 測試 MAC（本機管理；避開既有真機保留）。
    const TEST_MAC: &str = "02:00:5e:00:53:99";
    const TEST_HOSTNAME: &str = "asset-nest-live-test";

    dotenvy::dotenv().ok();
    // 變更執行中設定；與其他變更型真機測試互斥（見 LIVE_MUTATION_LOCK）。
    let _guard = LIVE_MUTATION_LOCK.lock().await;

    let config = Config::from_env().expect("讀取環境設定失敗");
    let (client, url) = live_client(&config);

    // 前置（唯讀）：掃描現有 subnet-id 與 CIDR。測試段重疊＝不執行（絕不動既有
    // 網段）；測試 id 取現有最大值 +1（未使用）。
    let test_net: ipnet::Ipv4Net = TEST_CIDR.parse().expect("測試 CIDR 格式");
    let before = client
        .config_get_dhcp4()
        .await
        .expect("讀取 Kea 設定失敗（檢查認證與控制通道）");
    let overlaps = before.subnets.values().any(|subnet| {
        subnet.cidr.parse::<ipnet::Ipv4Net>().is_ok_and(|network| {
            network.contains(&test_net.network()) || test_net.contains(&network.network())
        })
    });
    if overlaps {
        println!(
            "Kea 已有網段與 {TEST_CIDR} 重疊：不執行真機建立驗證（不得動既有網段），本測試結束"
        );
        return;
    }
    let commands = client.list_commands().await.expect("list-commands 失敗");
    if !commands.iter().any(|command| command == "subnet4-add") {
        println!(
            "subnet4-add 不支援：此機 Kea 未載入 subnet_cmds hook；不執行建立驗證（見 docs/adr/0023）"
        );
        return;
    }
    let test_id = before.subnets.keys().max().copied().unwrap_or(0) + 1;
    let mut existing_ids: Vec<i64> = before.subnets.keys().copied().collect();
    existing_ids.sort_unstable();
    println!("真機建立測試：{TEST_CIDR}、subnet-id {test_id}（現有 id：{existing_ids:?}）");

    // 臨時 SQLite DB（in-memory）：受管網段＋gateway＋兩段 pool＋一筆保留指派。
    let pool = test_pool().await;
    let subnet_row =
        sqlx::query("INSERT INTO subnets (cidr, name, gateway, kea_subnet_id) VALUES (?, ?, ?, ?)")
            .bind(TEST_CIDR)
            .bind("真機建立測試")
            .bind(TEST_GATEWAY)
            .bind(test_id)
            .execute(&pool)
            .await
            .expect("插入測試網段");
    let db_subnet_id = subnet_row.last_insert_rowid();
    for (start, end) in TEST_POOLS {
        sqlx::query("INSERT INTO subnet_pools (subnet_id, start_ip, end_ip) VALUES (?, ?, ?)")
            .bind(db_subnet_id)
            .bind(start)
            .bind(end)
            .execute(&pool)
            .await
            .expect("插入測試位址池");
    }
    let asset_row = sqlx::query("INSERT INTO assets (description, location) VALUES (?, ?)")
        .bind(TEST_HOSTNAME)
        .bind("真機驗證")
        .execute(&pool)
        .await
        .expect("插入測試資產");
    let interface_row = sqlx::query("INSERT INTO interfaces (asset_id, mac) VALUES (?, ?)")
        .bind(asset_row.last_insert_rowid())
        .bind(TEST_MAC)
        .execute(&pool)
        .await
        .expect("插入測試介面");
    sqlx::query(
        "INSERT INTO ip_assignments (subnet_id, address, interface_id, purpose, hostname) \
         VALUES (?, ?, ?, 'reservation', ?)",
    )
    .bind(db_subnet_id)
    .bind(TEST_IP)
    .bind(interface_row.last_insert_rowid())
    .bind(TEST_HOSTNAME)
    .execute(&pool)
    .await
    .expect("插入測試保留指派");

    // 計畫（唯讀；此刻尚未變更 Kea，斷言失敗不會殘留）。
    let plan = sync::plan(&pool, &client).await.expect("產生同步計畫失敗");
    let planned = plan
        .subnets
        .iter()
        .find(|subnet| subnet.kea_subnet_id == test_id)
        .expect("計畫應含測試網段");
    assert_eq!(plan.totals.subnet_add, 1, "計畫應有一筆新增網段");
    let planned_add = planned
        .subnet_add
        .as_ref()
        .expect("計畫應將測試網段列為新增（subnet_add）");
    assert_eq!(
        planned_add.pools,
        vec![
            "192.0.2.10-192.0.2.20".to_string(),
            "192.0.2.30-192.0.2.40".to_string()
        ],
        "計畫 pool 應正規化（start-end）且數值排序"
    );
    assert_eq!(
        planned_add.gateway.as_deref(),
        Some(TEST_GATEWAY),
        "計畫 gateway 應為測試值"
    );
    assert!(
        planned.pool_add.is_empty() && planned.pool_delete.is_empty() && planned.gateway.is_none(),
        "新建網段不重複列 pool／gateway 差異"
    );
    assert_eq!(
        planned.add.len(),
        1,
        "缺少網段的保留差異照算（Kea 端視為空集合）"
    );
    println!(
        "計畫：subnet_add pools={:?}、gateway={:?}；保留待新增 {} 筆",
        planned_add.pools,
        planned_add.gateway,
        planned.add.len()
    );

    // 套用：建立缺少網段→保留三相位→既有網段設定→單次 config-write。
    let report = sync::apply(&pool, &client).await.expect("套用完整同步失敗");
    let applied = report
        .subnets
        .iter()
        .find(|subnet| subnet.kea_subnet_id == test_id)
        .expect("報告應含測試網段");
    let subnet_added = applied.subnet_added;
    let subnet_add_error = applied.subnet_add_error.clone();
    let reservations_added = applied.added;
    let settings_counts = (
        applied.pool_added,
        applied.pool_deleted,
        applied.gateway_updated,
    );
    let failures = applied.failures.len();
    let config_write = report.config_write;
    let config_write_message = report.config_write_message.clone();
    println!(
        "套用報告：subnet_added={subnet_added}、subnet_add_error={subnet_add_error:?}、\
         保留新增={reservations_added}／失敗={failures}、pool/gateway 重複計數={settings_counts:?}、\
         config_write={config_write}（{config_write_message:?}）"
    );

    // 收集真機驗證值（不在此斷言；先確保清理執行，見票 03 安全約束）。
    let mut observation_errors: Vec<String> = Vec::new();
    let observed_subnet = match client.config_get_dhcp4().await {
        Ok(config) => config.subnets.get(&test_id).map(|subnet| {
            (
                subnet.cidr.clone(),
                subnet
                    .pools
                    .iter()
                    .map(|range| range.to_compact_string())
                    .collect::<Vec<_>>(),
                subnet.gateway.clone(),
            )
        }),
        Err(error) => {
            observation_errors.push(format!("驗證 config-get 失敗：{error}"));
            None
        }
    };
    let observed_hosts = match client.reservation_get_all(test_id).await {
        Ok(hosts) => Some(
            hosts
                .into_iter()
                .map(|host| (host.ip_address, host.hw_address, host.hostname))
                .collect::<Vec<_>>(),
        ),
        Err(error) => {
            observation_errors.push(format!("驗證 reservation-get-all 失敗：{error}"));
            None
        }
    };

    // 對同一 id 重送 `subnet4-add`：記錄 Kea 拒絕訊息形狀（被拒、不改變狀態）。
    let duplicate = json!({
        "id": test_id,
        "subnet": TEST_CIDR,
        "pools": TEST_POOLS
            .iter()
            .map(|(start, end)| json!({ "pool": format!("{start} - {end}") }))
            .collect::<Vec<_>>(),
        "option-data": [{ "name": "routers", "code": 3, "space": "dhcp4", "data": TEST_GATEWAY }],
    });
    let duplicate_error = client.subnet4_add(&duplicate).await.err();
    match &duplicate_error {
        Some(error) => println!("重送 subnet4-add 的 Kea 拒絕訊息：{error}"),
        None => observation_errors.push("重送 subnet4-add 竟成功（不應發生）".to_string()),
    }

    // 清理：`reservation-del` → `subnet4-del` → `config-write`（逐項記錄、不 panic）。
    let reservation_cleared = match client.reservation_del(test_id, TEST_IP).await {
        Ok(changed) => Some(changed),
        Err(error) => {
            println!("清理 reservation-del 未成功（繼續）：{error}");
            None
        }
    };
    let reservation_gone = match client.reservation_get_all(test_id).await {
        Ok(hosts) => Some(
            !hosts
                .iter()
                .any(|host| host.ip_address.as_deref() == Some(TEST_IP)),
        ),
        Err(error) => {
            println!("清理後 reservation-get-all 未成功（繼續）：{error}");
            None
        }
    };
    let subnet_deleted = match subnet4_del_against_live(&url, &config, test_id).await {
        Ok(text) => {
            println!("subnet4-del 成功：{text}");
            true
        }
        Err(error) => {
            println!("清理 subnet4-del 未成功：{error}");
            false
        }
    };
    let config_rewritten = if subnet_deleted {
        match client.config_write().await {
            Ok(()) => true,
            Err(error) => {
                println!("清理 config-write 未成功：{error}");
                false
            }
        }
    } else {
        false
    };
    let residual_subnet = match client.config_get_dhcp4().await {
        Ok(config) => Some(config.subnets.contains_key(&test_id)),
        Err(error) => {
            println!("清理後 config-get 未成功：{error}");
            None
        }
    };
    println!(
        "清理結果：reservation-del={reservation_cleared:?}、保留已不在={reservation_gone:?}、\
         subnet4-del={subnet_deleted}、config-write={config_rewritten}、殘留網段={residual_subnet:?}"
    );

    // ---- 清理完成後才斷言（避免 assert panic 造成殘留）----

    assert!(
        observation_errors.is_empty(),
        "驗證過程有錯誤：{observation_errors:?}（清理已完成）"
    );
    assert!(
        subnet_added,
        "apply 應建立缺少的網段：subnet_add_error={subnet_add_error:?}"
    );
    assert_eq!(subnet_add_error, None, "建立不應失敗");
    assert_eq!(reservations_added, 1, "新建網段的保留應同回合推送");
    assert_eq!(failures, 0, "同回合保留推送不應失敗");
    assert_eq!(
        settings_counts,
        (0, 0, false),
        "新建網段不另計 pool／gateway 變更"
    );
    assert_eq!(
        config_write, "ok",
        "有成功變更應寫入 Kea 設定檔一次（{config_write_message:?}）"
    );
    assert_eq!(
        observed_subnet,
        Some((
            TEST_CIDR.to_string(),
            vec![
                "192.0.2.10-192.0.2.20".to_string(),
                "192.0.2.30-192.0.2.40".to_string()
            ],
            Some(TEST_GATEWAY.to_string()),
        )),
        "config-get 應見新建網段的 CIDR、pools、gateway"
    );
    assert_eq!(
        observed_hosts,
        Some(vec![(
            Some(TEST_IP.to_string()),
            Some(TEST_MAC.to_string()),
            Some(TEST_HOSTNAME.to_string()),
        )]),
        "reservation-get-all 應見同回合推送的保留"
    );
    assert!(
        matches!(duplicate_error, Some(KeaError::Response { .. })),
        "重送 subnet4-add 應被 Kea 以回應錯誤拒絕：{duplicate_error:?}"
    );
    assert_eq!(reservation_cleared, Some(true), "清理應刪除測試保留");
    assert_eq!(reservation_gone, Some(true), "清理後測試保留不應存在");
    assert!(subnet_deleted, "清理應刪除測試網段");
    assert!(config_rewritten, "刪除後應 config-write 持久化");
    assert_eq!(residual_subnet, Some(false), "config-get 不應再見測試網段");

    println!("真機建立 roundtrip 成功且已清理：subnet-id {test_id}、{TEST_CIDR}");
}
