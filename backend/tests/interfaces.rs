//! 介面管理整合測試：CRUD、MAC 正規化、名稱門檻、重複 MAC 警示與連動刪除（見票 02）。

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use sqlx::sqlite::SqlitePoolOptions;
use tower::ServiceExt;

use asset_nest::{AppState, app};

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

/// 以 `oneshot` 發送請求；回傳狀態碼與 JSON（204 等空內容為 `Value::Null`）。
async fn send(
    pool: &SqlitePool,
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

    let response = app(AppState::new(
        pool.clone(),
        std::env::temp_dir().join("asset-nest-test-no-dist"),
    ))
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

/// 新增資產並斷言成功，回傳回應 JSON。
async fn create_asset(pool: &SqlitePool, body: Value) -> Value {
    let (status, json) = send(pool, Method::POST, "/api/v1/assets", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "新增資產應成功：{json}");
    json
}

/// 對資產新增介面（不檢查狀態碼，供各測試自行斷言）。
async fn post_interface(pool: &SqlitePool, asset_id: i64, body: Value) -> (StatusCode, Value) {
    send(
        pool,
        Method::POST,
        &format!("/api/v1/assets/{asset_id}/interfaces"),
        Some(body),
    )
    .await
}

/// 讀取資產詳情（含介面清單）並斷言成功。
async fn get_detail(pool: &SqlitePool, asset_id: i64) -> Value {
    let (status, json) = send(
        pool,
        Method::GET,
        &format!("/api/v1/assets/{asset_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "讀取資產詳情應成功：{json}");
    json
}

#[tokio::test]
async fn interface_crud_lifecycle() {
    let pool = test_pool().await;
    let asset = create_asset(
        &pool,
        json!({ "description": "測試伺服器", "location": "機房 A" }),
    )
    .await;
    let asset_id = asset["id"].as_i64().expect("回應含 id");

    // 新增資產尚無介面
    let detail = get_detail(&pool, asset_id).await;
    assert_eq!(detail["interfaces"], json!([]));
    assert_eq!(detail["description"], "測試伺服器", "詳情仍含資產欄位");

    // 新增介面
    let (status, created) = post_interface(
        &pool,
        asset_id,
        json!({ "name": "eth0", "mac": "AA-BB-CC-DD-EE-FF", "note": "主機板內建" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let interface_id = created["id"].as_i64().expect("回應含 id");
    assert!(interface_id > 0, "id 為資料庫自增");
    assert_eq!(created["asset_id"], asset_id);
    assert_eq!(created["name"], "eth0");
    assert_eq!(created["mac"], "aa:bb:cc:dd:ee:ff", "MAC 已正規化");
    assert_eq!(created["note"], "主機板內建");
    assert_eq!(created["warnings"], json!([]), "唯一 MAC 無警示");
    assert!(
        created["created_at"]
            .as_str()
            .is_some_and(|value| !value.is_empty()),
        "回應含建立時間"
    );

    // 詳情包含介面
    let detail = get_detail(&pool, asset_id).await;
    let interfaces = detail["interfaces"].as_array().expect("interfaces 為陣列");
    assert_eq!(interfaces.len(), 1);
    assert_eq!(interfaces[0]["id"], interface_id);
    assert_eq!(interfaces[0]["name"], "eth0");
    assert_eq!(interfaces[0]["mac"], "aa:bb:cc:dd:ee:ff");

    // 編輯介面：改名、換 MAC、清除備註
    let (status, updated) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/interfaces/{interface_id}"),
        Some(json!({ "name": "eth1", "mac": "001122334455", "note": null })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["name"], "eth1");
    assert_eq!(updated["mac"], "00:11:22:33:44:55");
    assert!(updated["note"].is_null(), "顯式 null 清除備註");
    assert_eq!(updated["warnings"], json!([]));

    // 未提供的欄位維持原值
    let (status, kept) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/interfaces/{interface_id}"),
        Some(json!({ "name": "eth2" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(kept["name"], "eth2");
    assert_eq!(kept["mac"], "00:11:22:33:44:55", "未提供 MAC 維持原值");

    // 刪除介面
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/v1/interfaces/{interface_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);
    assert_eq!(get_detail(&pool, asset_id).await["interfaces"], json!([]));

    // 刪除不存在的介面
    let (status, body) = send(
        &pool,
        Method::DELETE,
        &format!("/api/v1/interfaces/{interface_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "not_found");

    // 編輯不存在的介面
    let (status, _) = send(
        &pool,
        Method::PATCH,
        "/api/v1/interfaces/9999",
        Some(json!({ "name": "eth9" })),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 對不存在的資產新增介面
    let (status, body) = post_interface(&pool, 9999, json!({ "name": "eth9" })).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["message"], "找不到資產");
}

#[tokio::test]
async fn blank_mac_requires_name() {
    let pool = test_pool().await;
    let asset = create_asset(
        &pool,
        json!({ "description": "測試伺服器", "location": "機房 A" }),
    )
    .await;
    let asset_id = asset["id"].as_i64().expect("回應含 id");

    // MAC 空白且無名稱 → 阻擋
    let (status, body) = post_interface(&pool, asset_id, json!({ "mac": "" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["error"], "validation_error");
    assert_eq!(body["message"], "MAC 空白時名稱為必填");
    assert_eq!(body["details"]["field"], "name");

    // MAC 未提供且名稱全空白 → 阻擋
    let (status, body) = post_interface(&pool, asset_id, json!({ "name": "   " })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "name");

    // MAC 空白視為未填；有名稱即可建立
    let (status, created) = post_interface(
        &pool,
        asset_id,
        json!({ "name": " eth0 ", "mac": "   ", "note": "無 MAC" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["name"], "eth0", "名稱去除前後空白");
    assert!(created["mac"].is_null(), "空白 MAC 儲存為 NULL");
    let no_mac_id = created["id"].as_i64().expect("回應含 id");

    // 有 MAC 時名稱選填
    let (status, with_mac) =
        post_interface(&pool, asset_id, json!({ "mac": "aabbccddeeff" })).await;
    assert_eq!(status, StatusCode::CREATED);
    assert!(with_mac["name"].is_null());
    let with_mac_id = with_mac["id"].as_i64().expect("回應含 id");

    // 編輯清除 MAC 後無名稱 → 以合併後的最終狀態阻擋
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/interfaces/{with_mac_id}"),
        Some(json!({ "mac": null })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["message"], "MAC 空白時名稱為必填");

    // 有 MAC 時可清空名稱
    let (status, updated) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/interfaces/{with_mac_id}"),
        Some(json!({ "name": "" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(updated["name"].is_null());
    assert_eq!(updated["mac"], "aa:bb:cc:dd:ee:ff");

    // 無 MAC 的介面不可清空名稱
    let (status, body) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/interfaces/{no_mac_id}"),
        Some(json!({ "name": null })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["details"]["field"], "name");

    // 驗證失敗不寫入任何資料
    let detail = get_detail(&pool, asset_id).await;
    assert_eq!(
        detail["interfaces"]
            .as_array()
            .expect("interfaces 為陣列")
            .len(),
        2,
        "僅存在兩筆成功建立的介面"
    );

    // 資料庫層 CHECK 亦擋下兩者皆空的介面（結構規則雙保險）
    let direct = sqlx::query("INSERT INTO interfaces (asset_id, name, mac) VALUES (?, NULL, NULL)")
        .bind(asset_id)
        .execute(&pool)
        .await;
    assert!(direct.is_err(), "資料庫 CHECK 應擋下兩者皆空的介面");
}

#[tokio::test]
async fn mac_is_normalized_to_lowercase_colon() {
    let pool = test_pool().await;
    let asset = create_asset(
        &pool,
        json!({ "description": "測試伺服器", "location": "機房 A" }),
    )
    .await;
    let asset_id = asset["id"].as_i64().expect("回應含 id");

    // 常見輸入格式皆正規化為小寫冒號格式
    let cases = [
        ("AA:BB:CC:DD:EE:01", "aa:bb:cc:dd:ee:01"),
        ("aa-bb-cc-dd-ee-02", "aa:bb:cc:dd:ee:02"),
        ("AABBCCDDEE03", "aa:bb:cc:dd:ee:03"),
        ("AA.BB.CC.DD.EE.04", "aa:bb:cc:dd:ee:04"),
        ("  aa:bb:cc:dd:ee:05  ", "aa:bb:cc:dd:ee:05"),
    ];
    for (input, expected) in cases {
        let (status, created) =
            post_interface(&pool, asset_id, json!({ "name": "eth0", "mac": input })).await;
        assert_eq!(status, StatusCode::CREATED, "輸入 {input} 應可建立");
        assert_eq!(created["mac"], expected, "輸入 {input}");
    }

    // 非 12 位十六進位一律阻擋
    for input in [
        "aa:bb:cc:dd:ee",
        "aa:bb:cc:dd:ee:ff:00",
        "gg:hh:ii:jj:kk:ll",
        "not-a-mac",
    ] {
        let (status, body) =
            post_interface(&pool, asset_id, json!({ "name": "eth0", "mac": input })).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "輸入 {input} 應被阻擋");
        assert_eq!(body["details"]["field"], "mac", "輸入 {input}");
    }

    // 編輯時同樣正規化
    let detail = get_detail(&pool, asset_id).await;
    let first_id = detail["interfaces"][0]["id"].as_i64().expect("介面 id");
    let (status, updated) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/interfaces/{first_id}"),
        Some(json!({ "mac": "FF-EE-DD-CC-BB-AA" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["mac"], "ff:ee:dd:cc:bb:aa");
}

#[tokio::test]
async fn duplicate_mac_only_warns_across_assets() {
    let pool = test_pool().await;
    let first = create_asset(
        &pool,
        json!({ "description": "設備一", "location": "機房" }),
    )
    .await;
    let second = create_asset(
        &pool,
        json!({ "description": "設備二", "location": "機房" }),
    )
    .await;
    let first_id = first["id"].as_i64().expect("回應含 id");
    let second_id = second["id"].as_i64().expect("回應含 id");

    // 第一個介面無重複
    let (status, created) = post_interface(
        &pool,
        first_id,
        json!({ "name": "eth0", "mac": "AA:BB:CC:DD:EE:FF" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(created["warnings"], json!([]));

    // 其他資產以不同格式輸入相同 MAC：不阻擋，但帶警示
    let (status, duplicated) = post_interface(
        &pool,
        second_id,
        json!({ "name": "eth0", "mac": "aabbccddeeff" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "重複 MAC 不阻擋建立");
    assert_eq!(duplicated["mac"], "aa:bb:cc:dd:ee:ff");
    let warnings = duplicated["warnings"].as_array().expect("warnings 為陣列");
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0]["code"], "duplicate_mac");
    assert!(
        warnings[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("aa:bb:cc:dd:ee:ff")),
        "警示訊息含正規化 MAC"
    );
    let interface_id = duplicated["id"].as_i64().expect("回應含 id");
    assert!(interface_id > 0);

    // 另一資產上建立唯一 MAC 的介面，供「排除自身」檢查
    let (status, unique) = post_interface(
        &pool,
        second_id,
        json!({ "name": "eth1", "mac": "00:11:22:33:44:55" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(unique["warnings"], json!([]));
    let unique_id = unique["id"].as_i64().expect("回應含 id");

    // 編輯自身（未改 MAC）不誤報
    let (status, updated) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/interfaces/{unique_id}"),
        Some(json!({ "note": "改備註" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["warnings"], json!([]), "排除自身不誤報");

    // 改為重複 MAC：僅警示
    let (status, updated) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/interfaces/{unique_id}"),
        Some(json!({ "mac": "AA-BB-CC-DD-EE-FF" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["warnings"][0]["code"], "duplicate_mac");

    // 改為唯一 MAC：警示消失
    let (status, updated) = send(
        &pool,
        Method::PATCH,
        &format!("/api/v1/interfaces/{unique_id}"),
        Some(json!({ "mac": "66:77:88:99:AA:BB" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["warnings"], json!([]));
}

#[tokio::test]
async fn deleting_asset_cascades_interfaces() {
    let pool = test_pool().await;
    let first = create_asset(
        &pool,
        json!({ "description": "設備一", "location": "機房" }),
    )
    .await;
    let second = create_asset(
        &pool,
        json!({ "description": "設備二", "location": "機房" }),
    )
    .await;
    let first_id = first["id"].as_i64().expect("回應含 id");
    let second_id = second["id"].as_i64().expect("回應含 id");

    let (_, _) = post_interface(&pool, first_id, json!({ "name": "eth0" })).await;
    let (_, _) = post_interface(&pool, first_id, json!({ "mac": "aabbccddeeff" })).await;
    let (_, kept) = post_interface(&pool, second_id, json!({ "name": "wlan0" })).await;
    let kept_id = kept["id"].as_i64().expect("回應含 id");

    // 刪除資產 → 其介面連動刪除（ON DELETE CASCADE）
    let (status, _) = send(
        &pool,
        Method::DELETE,
        &format!("/api/v1/assets/{first_id}"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let remaining: Vec<i64> = sqlx::query_scalar("SELECT id FROM interfaces ORDER BY id")
        .fetch_all(&pool)
        .await
        .expect("查詢剩餘介面");
    assert_eq!(remaining, vec![kept_id], "僅保留另一資產的介面");

    // 對已刪除的資產新增介面 → 404
    let (status, body) = post_interface(&pool, first_id, json!({ "name": "eth9" })).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["message"], "找不到資產");
}
