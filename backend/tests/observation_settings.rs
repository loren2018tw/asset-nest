//! 觀測設定整合測試：`observed` PATCH、`local` 標示與摘要欄位（見票 01）。

use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex};

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use tower::ServiceExt;

use asset_nest::probe::Prober;
use asset_nest::subnets::Subnet;
use asset_nest::{AppState, app};

/// 可腳本化的探測邊界 stub：清單內的 CIDR 回 `true`，並記錄被查詢的網段。
struct StubProber {
    local_cidrs: Vec<String>,
    seen: Mutex<Vec<String>>,
}

impl StubProber {
    fn new(local_cidrs: &[&str]) -> Self {
        Self {
            local_cidrs: local_cidrs.iter().map(|cidr| cidr.to_string()).collect(),
            seen: Mutex::new(Vec::new()),
        }
    }

    /// 已被查詢的網段 CIDR（依呼叫順序）。
    fn seen(&self) -> Vec<String> {
        self.seen.lock().expect("stub 鎖").clone()
    }
}

impl Prober for StubProber {
    fn is_local(&self, subnet: &Subnet) -> bool {
        self.seen.lock().expect("stub 鎖").push(subnet.cidr.clone());
        self.local_cidrs.iter().any(|cidr| cidr == &subnet.cidr)
    }

    /// 本檔僅測設定與本機判定；探測回應固定為空（掃描行為見 `observation_sweep.rs`）。
    fn probe(&self, _subnet: &Subnet, _targets: &[Ipv4Addr]) -> Vec<(Ipv4Addr, String)> {
        Vec::new()
    }

    /// 本檔不測被動監聽；固定回空。
    fn passive_observe(
        &self,
        _subnet: &Subnet,
        _window: std::time::Duration,
    ) -> Vec<(Ipv4Addr, String)> {
        Vec::new()
    }
}

/// 建立測試資料庫並套用 migrations（領域測試必須先套）。
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

fn dist_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("asset-nest-test-no-dist")
}

/// 建立附掛 stub 探測邊界的 AppState。
fn test_state(pool: &SqlitePool, prober: Arc<StubProber>) -> AppState {
    AppState::new(pool.clone(), dist_dir()).with_prober(prober)
}

/// 以 `oneshot` 發送請求；回傳狀態碼與 JSON（204 等空內容為 `Value::Null`）。
async fn send(
    state: &AppState,
    method: Method,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder().method(method).uri(uri);
    let body = match body {
        Some(value) => {
            builder = builder.header(header::CONTENT_TYPE, "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };

    let response = app(state.clone())
        .oneshot(builder.body(body).expect("建立請求"))
        .await
        .expect("執行請求");

    let status = response.status();
    let bytes = response
        .into_body()
        .collect()
        .await
        .expect("讀取回應內容")
        .to_bytes();

    let json = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("回應為 JSON")
    };

    (status, json)
}

/// 新增網段並斷言成功，回傳回應 JSON。
async fn create_subnet(state: &AppState, body: Value) -> Value {
    let (status, json) = send(state, Method::POST, "/api/v1/subnets", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "新增網段應成功：{json}");
    json
}

/// 讀取單一網段 id。
fn id_of(created: &Value) -> i64 {
    created["id"].as_i64().expect("回應含 id")
}

/// 讀取資料庫中的 `observed` 值。
async fn stored_observed(pool: &SqlitePool, id: i64) -> i64 {
    sqlx::query_scalar("SELECT observed FROM subnets WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("查詢觀測開關")
}

#[tokio::test]
async fn v4_observed_patch_roundtrips_and_carries_local() {
    let pool = test_pool().await;
    let stub = Arc::new(StubProber::new(&["10.20.1.0/24"]));
    let state = test_state(&pool, stub.clone());

    // 建立預設未觀測；詳情與 PATCH 皆帶 local
    let created = create_subnet(&state, json!({ "cidr": "10.20.1.0/24", "name": "觀測區" })).await;
    let id = id_of(&created);
    assert_eq!(created["observed"], false, "建立預設未觀測");
    assert_eq!(created["local"], true, "本機同 L2 由注入的探測邊界判定");

    let (status, detail) = send(&state, Method::GET, &format!("/api/v1/subnets/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["observed"], false);
    assert_eq!(detail["local"], true);

    // 開啟觀測：PATCH 回應即時反映，其他欄位不受影響
    let (status, enabled) = send(
        &state,
        Method::PATCH,
        &format!("/api/v1/subnets/{id}"),
        Some(json!({ "observed": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "開啟觀測應成功：{enabled}");
    assert_eq!(enabled["observed"], true);
    assert_eq!(enabled["local"], true);
    assert_eq!(enabled["cidr"], "10.20.1.0/24");
    assert_eq!(enabled["name"], "觀測區");
    assert_eq!(stored_observed(&pool, id).await, 1, "開啟持久化為 1");

    // 詳情與列表皆反映開啟狀態
    let (status, detail) = send(&state, Method::GET, &format!("/api/v1/subnets/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["observed"], true);
    assert_eq!(detail["local"], true);

    let (status, page) = send(&state, Method::GET, "/api/v1/subnets", None).await;
    assert_eq!(status, StatusCode::OK);
    let item = &page["items"][0];
    assert_eq!(item["observed"], true);
    assert_eq!(item["local"], true);

    // 關閉觀測：往返一致；宣告資料不受影響（見 ADR-0014）
    let (status, disabled) = send(
        &state,
        Method::PATCH,
        &format!("/api/v1/subnets/{id}"),
        Some(json!({ "observed": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(disabled["observed"], false);
    assert_eq!(disabled["local"], true, "local 與 observed 無關");
    assert_eq!(stored_observed(&pool, id).await, 0, "關閉持久化為 0");

    assert!(
        stub.seen().iter().any(|cidr| cidr == "10.20.1.0/24"),
        "local 須經注入的探測邊界判定：{:?}",
        stub.seen()
    );
}

#[tokio::test]
async fn v6_observed_enable_is_rejected_and_stays_off() {
    let pool = test_pool().await;
    let stub = Arc::new(StubProber::new(&[]));
    let state = test_state(&pool, stub);

    let created = create_subnet(&state, json!({ "cidr": "fd42::/64", "name": "v6 區" })).await;
    let id = id_of(&created);
    assert_eq!(created["observed"], false);
    assert_eq!(created["local"], false, "v6 恆非同 L2");

    // 對 v6 開啟 → 400 結構錯誤（見 ADR-0006）
    let (status, body) = send(
        &state,
        Method::PATCH,
        &format!("/api/v1/subnets/{id}"),
        Some(json!({ "observed": true })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
    assert_eq!(body["details"]["field"], "observed");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("IPv6")),
        "訊息說明 v6 不支援觀測：{}",
        body["message"]
    );

    let (_, detail) = send(&state, Method::GET, &format!("/api/v1/subnets/{id}"), None).await;
    assert_eq!(detail["observed"], false, "維持 0");
    assert_eq!(stored_observed(&pool, id).await, 0, "v6 不得持久化為 1");

    // 對 v6 關閉（false）為合法 no-op
    let (status, disabled) = send(
        &state,
        Method::PATCH,
        &format!("/api/v1/subnets/{id}"),
        Some(json!({ "observed": false })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(disabled["observed"], false);
}

#[tokio::test]
async fn converting_observed_v4_to_v6_is_rejected() {
    let pool = test_pool().await;
    let stub = Arc::new(StubProber::new(&["10.30.1.0/24"]));
    let state = test_state(&pool, stub);

    let created = create_subnet(&state, json!({ "cidr": "10.30.1.0/24" })).await;
    let id = id_of(&created);

    let (status, enabled) = send(
        &state,
        Method::PATCH,
        &format!("/api/v1/subnets/{id}"),
        Some(json!({ "observed": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(enabled["observed"], true);

    // 已開啟觀測的 v4 改為 v6：合併後狀態不合法（殘留 observed）
    let (status, body) = send(
        &state,
        Method::PATCH,
        &format!("/api/v1/subnets/{id}"),
        Some(json!({ "cidr": "fd43::/64" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "observed");

    let (_, detail) = send(&state, Method::GET, &format!("/api/v1/subnets/{id}"), None).await;
    assert_eq!(detail["cidr"], "10.30.1.0/24", "拒絕後維持原值");
    assert_eq!(detail["observed"], true);
}

#[tokio::test]
async fn summaries_carry_observed_and_stub_local() {
    let pool = test_pool().await;
    let stub = Arc::new(StubProber::new(&["10.50.1.0/24"]));
    let state = test_state(&pool, stub.clone());

    let local_created = create_subnet(&state, json!({ "cidr": "10.50.1.0/24" })).await;
    let local_id = id_of(&local_created);
    create_subnet(&state, json!({ "cidr": "10.50.2.0/24" })).await;

    let (status, enabled) = send(
        &state,
        Method::PATCH,
        &format!("/api/v1/subnets/{local_id}"),
        Some(json!({ "observed": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(enabled["observed"], true);

    // 摘要：observed 來自資料庫；local 由 stub 的判定決定
    let (status, page) = send(&state, Method::GET, "/api/v1/subnets", None).await;
    assert_eq!(status, StatusCode::OK);
    let items = page["items"].as_array().expect("items 為陣列");
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["observed"], true);
    assert_eq!(items[0]["local"], true);
    assert_eq!(items[1]["observed"], false);
    assert_eq!(items[1]["local"], false, "非 stub 同 L2 清單");

    // 詳情 local=false：stub 說非同 L2
    let remote_id = items[1]["id"].as_i64().expect("回應含 id");
    let (status, detail) = send(
        &state,
        Method::GET,
        &format!("/api/v1/subnets/{remote_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["local"], false);
    assert_eq!(detail["observed"], false);

    let seen = stub.seen();
    assert!(
        seen.iter().any(|cidr| cidr == "10.50.1.0/24")
            && seen.iter().any(|cidr| cidr == "10.50.2.0/24"),
        "兩個網段皆須經探測邊界判定：{seen:?}"
    );
}
