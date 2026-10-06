//! Kea 租約觀測整合測試（見票 08、spec §後端移除範圍）。
//!
//! 以本機 stub Kea 直接呼叫 [`observation::record_lease_observations`] 驗證：
//! 目前有效（`state=default`）租約以 `cltt` 記為最後可見（來源 `kea_lease`）、
//! 缺 `cltt`／MAC 的處理、與 ARP 的 latest-wins 語意，以及 Kea 未設定／
//! 讀取失敗時的行為。探測已外移給代理，本檔不含任何探測。

use std::sync::{Arc, Mutex};

use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use chrono::DateTime;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;

use asset_nest::kea::http::Client;
use asset_nest::observation;

// ---------- stub Kea（只實作租約讀取） ----------

/// 記憶體版假 Kea：`lease4-get-all` 回設定租約；可切換為命令失敗。
#[derive(Clone, Default)]
struct StubKea {
    leases: Arc<Mutex<Vec<Value>>>,
    fail: Arc<Mutex<bool>>,
    log: Arc<Mutex<Vec<String>>>,
}

impl StubKea {
    /// 啟動 stub 並回傳 client。
    async fn client(&self) -> Client {
        let router = Router::new()
            .route("/", post(handle))
            .with_state(self.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("綁定 stub 位址");
        let addr = listener.local_addr().expect("stub 位址");
        tokio::spawn(async move {
            axum::serve(listener, router).await.expect("stub 伺服器");
        });
        Client::new(format!("http://{addr}").parse().expect("URL"))
    }

    fn set_leases(&self, leases: &[Value]) {
        *self.leases.lock().expect("leases") = leases.to_vec();
    }

    fn fail(&self) {
        *self.fail.lock().expect("fail") = true;
    }

    fn log(&self) -> Vec<String> {
        self.log.lock().expect("log").clone()
    }
}

/// stub 的命令處理：回應格式比照 Kea（單元素陣列、result 0／1）。
async fn handle(State(stub): State<StubKea>, Json(payload): Json<Value>) -> Json<Value> {
    let command = payload
        .get("command")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    stub.log.lock().expect("log").push(command.clone());

    if *stub.fail.lock().expect("fail") {
        return Json(json!([{ "result": 1, "text": "simulated failure" }]));
    }

    let response = match command.as_str() {
        "lease4-get-all" => {
            let leases = stub.leases.lock().expect("leases").clone();
            json!([{ "result": 0, "arguments": { "leases": leases } }])
        }
        other => json!([{ "result": 2, "text": format!("unsupported: {other}") }]),
    };

    Json(response)
}

// ---------- 測試輔助 ----------

/// 建立測試資料庫並套用 migrations。
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

/// 固定時間字串 → epoch 秒（租約 `cltt` 用）。
fn epoch(rfc3339: &str) -> i64 {
    DateTime::parse_from_rfc3339(rfc3339)
        .expect("固定時間")
        .timestamp()
}

/// 一筆租約 JSON；`subnet-id` 預設 1。
fn lease(address: &str, mac: Option<&str>, cltt: Option<&str>, state: Value) -> Value {
    let mut lease = json!({
        "ip-address": address,
        "subnet-id": 1,
        "state": state,
    });
    if let Some(mac) = mac {
        lease["hw-address"] = json!(mac);
    }
    if let Some(cltt) = cltt {
        lease["cltt"] = json!(epoch(cltt));
    }
    lease
}

/// 植入受管網段，回傳 id。
async fn insert_subnet(pool: &SqlitePool, cidr: &str, kea_subnet_id: Option<i64>) -> i64 {
    sqlx::query("INSERT INTO subnets (cidr, kea_subnet_id) VALUES (?, ?)")
        .bind(cidr)
        .bind(kea_subnet_id)
        .execute(pool)
        .await
        .expect("植入網段")
        .last_insert_rowid()
}

/// 植入現況列。
async fn insert_presence(
    pool: &SqlitePool,
    subnet_id: i64,
    address: &str,
    last_seen_at: &str,
    mac: &str,
    source: &str,
) {
    sqlx::query(
        "INSERT INTO ip_presence (subnet_id, address, last_seen_at, last_seen_mac, last_seen_source)
         VALUES (?, ?, ?, ?, ?)",
    )
    .bind(subnet_id)
    .bind(address)
    .bind(last_seen_at)
    .bind(mac)
    .bind(source)
    .execute(pool)
    .await
    .expect("植入現況");
}

/// 讀取現況列（最後可見時間、MAC、來源）；無列為 `None`。
async fn presence(
    pool: &SqlitePool,
    subnet_id: i64,
    address: &str,
) -> Option<(Option<String>, Option<String>, Option<String>)> {
    sqlx::query_as(
        "SELECT last_seen_at, last_seen_mac, last_seen_source
           FROM ip_presence WHERE subnet_id = ? AND address = ?",
    )
    .bind(subnet_id)
    .bind(address)
    .fetch_optional(pool)
    .await
    .expect("讀取現況列")
}

/// 讀取 `observation_event` 的（位址、MAC、種類、來源），依寫入順序。
async fn events(pool: &SqlitePool) -> Vec<(String, String, String, String)> {
    sqlx::query_as(
        "SELECT address, COALESCE(mac, ''), kind, source FROM observation_event ORDER BY id ASC",
    )
    .fetch_all(pool)
    .await
    .expect("讀取觀測事件")
}

// ---------- 測試 ----------

/// 有效租約以 `cltt` 記最後可見；缺 cltt／MAC、非 default／不對應／非 IPv4
/// 一律略過；回傳寫入筆數。
#[tokio::test]
async fn default_leases_record_cltt_and_skip_invalid() {
    let pool = test_pool().await;
    let kea = StubKea::default();
    let client = kea.client().await;
    let subnet_id = insert_subnet(&pool, "10.0.0.0/28", Some(1)).await;

    kea.set_leases(&[
        // 有效租約：帶 MAC，cltt 記為最後可見、MAC 正規化。
        lease(
            "10.0.0.5",
            Some("AA:BB:CC:DD:EE:05"),
            Some("2026-10-05T12:00:00Z"),
            json!("default"),
        ),
        // 有效租約但缺 hw-address：記時間與來源、不寫事件、MAC 保持 NULL。
        lease(
            "10.0.0.3",
            None,
            Some("2026-10-05T11:00:00Z"),
            json!("default"),
        ),
        // 有效租約但缺 cltt：無可信觀測時間，不記。
        lease(
            "10.0.0.10",
            Some("aa:bb:cc:dd:ee:10"),
            None,
            json!("default"),
        ),
        // 以下皆非目前有效租約，不得留下現況。
        lease(
            "10.0.0.4",
            Some("aa:bb:cc:dd:ee:04"),
            Some("2026-10-05T12:00:00Z"),
            json!(1),
        ),
        lease(
            "10.0.0.6",
            Some("aa:bb:cc:dd:ee:06"),
            Some("2026-10-05T12:00:00Z"),
            json!(2),
        ),
        lease(
            "10.0.0.7",
            Some("aa:bb:cc:dd:ee:07"),
            Some("2026-10-05T12:00:00Z"),
            json!(3),
        ),
        // 缺 state：非 default。
        lease(
            "10.0.0.8",
            Some("aa:bb:cc:dd:ee:08"),
            Some("2026-10-05T12:00:00Z"),
            Value::Null,
        ),
        // 非 IPv4。
        lease(
            "fd42::5",
            Some("aa:bb:cc:dd:ee:09"),
            Some("2026-10-05T12:00:00Z"),
            json!("default"),
        ),
    ]);

    let recorded = observation::record_lease_observations(&pool, Some(&client))
        .await
        .expect("租約觀測成功");
    assert_eq!(recorded, 2, "只記有 cltt 的 default 租約");

    // 帶 MAC 的有效租約：cltt 記為最後可見、來源 kea_lease、MAC 正規化。
    assert_eq!(
        presence(&pool, subnet_id, "10.0.0.5").await,
        Some((
            Some("2026-10-05T12:00:00Z".to_string()),
            Some("aa:bb:cc:dd:ee:05".to_string()),
            Some("kea_lease".to_string()),
        ))
    );

    // 缺 MAC 的有效租約：仍記時間與來源，MAC 為 NULL。
    assert_eq!(
        presence(&pool, subnet_id, "10.0.0.3").await,
        Some((
            Some("2026-10-05T11:00:00Z".to_string()),
            None,
            Some("kea_lease".to_string()),
        ))
    );

    // 缺 cltt／非 default／非 IPv4：完全不得留下現況列。
    for ignored in [
        "10.0.0.10",
        "10.0.0.4",
        "10.0.0.6",
        "10.0.0.7",
        "10.0.0.8",
        "fd42::5",
    ] {
        assert!(
            presence(&pool, subnet_id, ignored).await.is_none(),
            "{ignored} 不得留下現況列"
        );
    }

    // 事件：只有帶 MAC 的有效租約寫 first_seen（來源 kea_lease）。
    assert_eq!(
        events(&pool).await,
        vec![(
            "10.0.0.5".to_string(),
            "aa:bb:cc:dd:ee:05".to_string(),
            "first_seen".to_string(),
            "kea_lease".to_string(),
        )]
    );
}

/// 多網段各依自己的 `kea_subnet_id` 篩選租約；未帶 `kea_subnet_id` 的網段略過。
#[tokio::test]
async fn leases_are_matched_per_subnet_kea_id() {
    let pool = test_pool().await;
    let kea = StubKea::default();
    let client = kea.client().await;
    let first = insert_subnet(&pool, "10.0.0.0/28", Some(1)).await;
    let second = insert_subnet(&pool, "10.1.0.0/28", Some(2)).await;
    let untracked = insert_subnet(&pool, "10.2.0.0/28", None).await;

    kea.set_leases(&[
        lease(
            "10.0.0.5",
            Some("aa:bb:cc:dd:ee:05"),
            Some("2026-10-05T12:00:00Z"),
            json!("default"),
        ),
        {
            let mut other = lease(
                "10.1.0.5",
                Some("aa:bb:cc:dd:ee:15"),
                Some("2026-10-05T13:00:00Z"),
                json!("default"),
            );
            other["subnet-id"] = json!(2);
            other
        },
        {
            let mut outside = lease(
                "10.9.9.9",
                Some("aa:bb:cc:dd:ee:99"),
                Some("2026-10-05T14:00:00Z"),
                json!("default"),
            );
            outside["subnet-id"] = json!(9);
            outside
        },
    ]);

    let recorded = observation::record_lease_observations(&pool, Some(&client))
        .await
        .expect("租約觀測成功");

    assert_eq!(recorded, 2, "只記對應網段的租約");
    assert!(presence(&pool, first, "10.0.0.5").await.is_some());
    assert!(presence(&pool, second, "10.1.0.5").await.is_some());
    assert!(
        presence(&pool, untracked, "10.9.9.9").await.is_none(),
        "無 kea_subnet_id 的網段不受租約影響"
    );
}

/// 同一位址的 ARP 與租約取最近者；較新的訊號連來源一起勝出。
#[tokio::test]
async fn latest_signal_wins_between_arp_and_lease() {
    let pool = test_pool().await;
    let kea = StubKea::default();
    let client = kea.client().await;
    let subnet_id = insert_subnet(&pool, "10.0.0.0/28", Some(1)).await;

    // 現況為較新的 ARP（10-05），租約較舊（10-04）→ 租約不得覆寫。
    insert_presence(
        &pool,
        subnet_id,
        "10.0.0.5",
        "2026-10-05T00:00:00Z",
        "aa:bb:cc:dd:ee:05",
        "arp",
    )
    .await;
    kea.set_leases(&[lease(
        "10.0.0.5",
        Some("aa:bb:cc:dd:ee:99"),
        Some("2026-10-04T00:00:00Z"),
        json!("default"),
    )]);

    observation::record_lease_observations(&pool, Some(&client))
        .await
        .expect("租約觀測成功");

    assert_eq!(
        presence(&pool, subnet_id, "10.0.0.5").await,
        Some((
            Some("2026-10-05T00:00:00Z".to_string()),
            Some("aa:bb:cc:dd:ee:05".to_string()),
            Some("arp".to_string()),
        )),
        "較舊租約不得覆寫較新的 ARP"
    );
    assert!(events(&pool).await.is_empty(), "回溯租約不得寫事件");

    // 租約較新（10-06）且換 MAC → 租約勝出、時間為 cltt、寫 mac_changed。
    kea.set_leases(&[lease(
        "10.0.0.5",
        Some("AA:BB:CC:DD:EE:99"),
        Some("2026-10-06T00:00:00Z"),
        json!("default"),
    )]);

    observation::record_lease_observations(&pool, Some(&client))
        .await
        .expect("租約觀測成功");

    assert_eq!(
        presence(&pool, subnet_id, "10.0.0.5").await,
        Some((
            Some("2026-10-06T00:00:00Z".to_string()),
            Some("aa:bb:cc:dd:ee:99".to_string()),
            Some("kea_lease".to_string()),
        )),
        "較新的租約 cltt 勝過舊 ARP，MAC 正規化"
    );
    assert_eq!(
        events(&pool).await,
        vec![(
            "10.0.0.5".to_string(),
            "aa:bb:cc:dd:ee:99".to_string(),
            "mac_changed".to_string(),
            "kea_lease".to_string(),
        )]
    );
}

/// 較舊租約的 MAC 不得每輪以回溯時間重複寫事件（複查修正語意保留）。
#[tokio::test]
async fn stale_lease_mac_does_not_rewrite_events_on_repeated_runs() {
    let pool = test_pool().await;
    let kea = StubKea::default();
    let client = kea.client().await;
    let subnet_id = insert_subnet(&pool, "10.0.0.0/28", Some(1)).await;

    insert_presence(
        &pool,
        subnet_id,
        "10.0.0.5",
        "2026-10-05T00:00:00Z",
        "aa:bb:cc:dd:ee:99",
        "arp",
    )
    .await;
    kea.set_leases(&[lease(
        "10.0.0.5",
        Some("aa:bb:cc:dd:ee:05"),
        Some("2026-10-04T00:00:00Z"),
        json!("default"),
    )]);

    for _ in 0..2 {
        observation::record_lease_observations(&pool, Some(&client))
            .await
            .expect("租約觀測成功");
    }

    assert!(events(&pool).await.is_empty(), "舊租約不得每輪重寫事件");
    assert_eq!(
        presence(&pool, subnet_id, "10.0.0.5").await,
        Some((
            Some("2026-10-05T00:00:00Z".to_string()),
            Some("aa:bb:cc:dd:ee:99".to_string()),
            Some("arp".to_string()),
        )),
        "現況仍為 ARP 的 MAC"
    );
}

/// Kea 未設定：不發送任何命令、不留下租約現況，回 0。
#[tokio::test]
async fn without_kea_no_command_is_sent_and_nothing_is_recorded() {
    let pool = test_pool().await;
    let kea = StubKea::default();
    let _client = kea.client().await;
    insert_subnet(&pool, "10.0.0.0/28", Some(1)).await;
    kea.set_leases(&[lease(
        "10.0.0.5",
        Some("aa:bb:cc:dd:ee:05"),
        Some("2026-10-05T12:00:00Z"),
        json!("default"),
    )]);

    let recorded = observation::record_lease_observations(&pool, None)
        .await
        .expect("未設定 Kea 視為無事可做");

    assert_eq!(recorded, 0);
    assert!(kea.log().is_empty(), "未設定時不得發送任何命令");
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM ip_presence")
        .fetch_one(&pool)
        .await
        .expect("讀取現況筆數");
    assert_eq!(count, 0, "不得留下租約現況列");
}

/// Kea 讀取失敗：回 0、記警告，既有資料不受影響。
#[tokio::test]
async fn kea_failure_returns_zero_and_keeps_existing_data() {
    let pool = test_pool().await;
    let kea = StubKea::default();
    let client = kea.client().await;
    let subnet_id = insert_subnet(&pool, "10.0.0.0/28", Some(1)).await;
    insert_presence(
        &pool,
        subnet_id,
        "10.0.0.1",
        "2026-10-05T00:00:00Z",
        "aa:bb:cc:dd:ee:01",
        "arp",
    )
    .await;
    kea.fail();

    let recorded = observation::record_lease_observations(&pool, Some(&client))
        .await
        .expect("Kea 失敗不讓週期任務失敗");

    assert_eq!(recorded, 0, "讀取失敗不記任何租約");
    assert_eq!(
        presence(&pool, subnet_id, "10.0.0.1").await,
        Some((
            Some("2026-10-05T00:00:00Z".to_string()),
            Some("aa:bb:cc:dd:ee:01".to_string()),
            Some("arp".to_string()),
        )),
        "既有現況不受影響"
    );
    assert_eq!(
        kea.log(),
        vec!["lease4-get-all".to_string()],
        "仍嘗試讀取租約"
    );
}
