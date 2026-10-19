//! Kea 保留推送：宣告異動的推送規則收斂於一處（見 `docs/adr/0011`、`docs/adr/0027`）。
//!
//! route 只剩三步：本地異動前 [`capture_assignment`]、本地異動、異動後
//! [`commit_assignment`]；刪除家族同型（[`capture_asset_deletion`]／
//! [`capture_interface_deletion`] → [`commit_deletions`]）、MAC 編輯家族亦同
//! （[`capture_interface_change`] → [`commit_interface_change`]）。觸發規則、
//! 捕捉查詢與 [`KeaSync`] 成形只存在本檔。
//!
//! 觸發規則（逐字沿用舊 `kea::sync::after_assignment_change`）：
//! - 受管網段（v4＋`kea_subnet_id`）且已設 client，缺一不互動；
//! - `after` 為保留→upsert（介面無 MAC→`KeaError::Data`→failed sync）；
//! - `before` 為保留、`after` 非保留（轉 static／取消）→delete；
//! - 兩者皆非保留→不產生同步結果。
//!
//! Kea 失敗僅回警示、不阻擋本地儲存，由完整同步（[`super::sync`]）修復。

use std::collections::HashMap;
use std::net::IpAddr;

use serde::Serialize;
use sqlx::SqlitePool;

use super::KeaError;
use super::http::{Client, ReservationRecord};
use crate::api::ApiError;
use crate::assignments::{self, Assignment};
use crate::interfaces;
use crate::subnets::{self, Subnet};

/// 單筆推送結果（隨指派回應附帶；僅在應同步時出現）。
#[derive(Debug, Clone, Serialize)]
pub struct KeaSync {
    /// `ok` 或 `failed`。
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl KeaSync {
    /// 成功結果；供 [`super::sync`] 的舊推送函式共用（票 04 前暫留）。
    pub(crate) fn ok() -> Self {
        Self {
            status: "ok",
            message: None,
        }
    }

    /// 失敗結果（訊息＝[`KeaError::to_string`]）。
    pub(crate) fn failed(error: &KeaError) -> Self {
        Self {
            status: "failed",
            message: Some(error.to_string()),
        }
    }
}

/// Kea 推送的內部 port（私有；供 module 測試注入 fake，不對外公開）。
#[allow(async_fn_in_trait)]
trait KeaPush {
    /// 冪等 upsert：先刪除再新增，最後 `config-write`。
    async fn upsert(&self, subnet_id: i64, record: &ReservationRecord) -> Result<(), KeaError>;

    /// 刪除保留；僅實際刪除（回 `true`）才 `config-write`。
    async fn delete(&self, subnet_id: i64, address: &str) -> Result<(), KeaError>;
}

/// 正式 adapter：包 [`Client`]，語意逐字沿用舊 `kea::sync` 的推送路徑。
struct HttpPush<'a>(&'a Client);

impl KeaPush for HttpPush<'_> {
    async fn upsert(&self, subnet_id: i64, record: &ReservationRecord) -> Result<(), KeaError> {
        // 先刪除（結果忽略、不存在視同已刪）再新增，變更期間僅毫秒級空窗。
        self.0
            .reservation_del(subnet_id, &record.ip_address)
            .await?;
        self.0.reservation_add(subnet_id, record).await?;
        self.0.config_write().await
    }

    async fn delete(&self, subnet_id: i64, address: &str) -> Result<(), KeaError> {
        if self.0.reservation_del(subnet_id, address).await? {
            self.0.config_write().await
        } else {
            Ok(())
        }
    }
}

/// 指派異動前的捕捉狀態；由 [`capture_assignment`] 產生、餵給
/// [`commit_assignment`]（欄位私有，route 僅持有）。
pub struct PendingAssignment {
    /// 位址所屬網段；`None`＝不在任何網段內（永不推送）。
    subnet: Option<Subnet>,
    /// 路徑位址的字串正規形式。
    address: String,
    /// 異動前的指派（`assignments::find` 的當下值）。
    before: Option<Assignment>,
}

/// 捕捉指派異動前的狀態（本地異動前呼叫；永不 404）。
pub async fn capture_assignment(
    db: &SqlitePool,
    address: IpAddr,
) -> Result<PendingAssignment, ApiError> {
    let subnet = subnets::find_by_address(db, address).await?;
    let before = match &subnet {
        Some(subnet) => assignments::find(db, subnet.id, &address.to_string())
            .await
            .map_err(|error| ApiError::internal("讀取指派失敗", error))?,
        None => None,
    };

    Ok(PendingAssignment {
        subnet,
        address: address.to_string(),
        before,
    })
}

/// 指派異動後推送單筆保留；回傳 `None`＝此異動不涉及 Kea。
///
/// 受管網段＋已設 client 時自行重讀現況（`after`）；Kea 失敗僅回警示
/// （見 ADR-0011）。
pub async fn commit_assignment(
    db: &SqlitePool,
    kea: Option<&Client>,
    pending: PendingAssignment,
) -> Result<Option<KeaSync>, ApiError> {
    let push = kea.map(HttpPush);
    commit_assignment_with(db, push.as_ref(), pending).await
}

/// [`commit_assignment`] 的內部版本：以 [`KeaPush`] port 注入（module 測試以
/// fake 呼叫）；公開函式以 [`HttpPush`] 包 `Option<&Client>` 後呼叫。
///
/// 參數為泛型而非 `&dyn KeaPush`：`async fn` trait 在穩定版 Rust 無 dyn
/// 相容性（E0038；見票 01 實作記錄）。
async fn commit_assignment_with<P: KeaPush + ?Sized>(
    db: &SqlitePool,
    kea: Option<&P>,
    pending: PendingAssignment,
) -> Result<Option<KeaSync>, ApiError> {
    let PendingAssignment {
        subnet,
        address,
        before,
    } = pending;
    let (Some(subnet), Some(kea)) = (subnet, kea) else {
        return Ok(None);
    };
    let Some(kea_subnet_id) = subnet.kea_subnet_id else {
        return Ok(None);
    };

    let after = assignments::find(db, subnet.id, &address)
        .await
        .map_err(|error| ApiError::internal("讀取指派失敗", error))?;

    let desired = after
        .as_ref()
        .filter(|assignment| assignment.purpose == "reservation");
    let before_was_reservation = before
        .as_ref()
        .is_some_and(|assignment| assignment.purpose == "reservation");

    if desired.is_none() && !before_was_reservation {
        return Ok(None);
    }

    let outcome = match desired {
        Some(assignment) => {
            let interface = interfaces::get(db, assignment.interface_id)
                .await
                .map_err(|error| ApiError::internal("讀取介面失敗", error))?;
            match interface.and_then(|interface| interface.mac) {
                Some(mac) => {
                    let record = ReservationRecord {
                        ip_address: assignment.address.clone(),
                        hw_address: mac,
                        hostname: assignment.hostname.clone(),
                    };
                    kea.upsert(kea_subnet_id, &record).await
                }
                None => Err(KeaError::Data(format!(
                    "保留位址 {} 的介面缺少 MAC",
                    assignment.address
                ))),
            }
        }
        None => {
            let assignment = before
                .as_ref()
                .ok_or_else(|| ApiError::internal("Kea 同步失敗", "缺少變更前的指派狀態"))?;
            kea.delete(kea_subnet_id, &assignment.address).await
        }
    };

    Ok(Some(match outcome {
        Ok(()) => KeaSync::ok(),
        Err(error) => KeaSync::failed(&error),
    }))
}

/// 資產／介面刪除前的捕捉狀態；由 [`capture_asset_deletion`]／
/// [`capture_interface_deletion`] 產生、餵給 [`commit_deletions`]（欄位私有，
/// route 僅持有）。
pub struct PendingDeletes {
    /// 待刪除的保留（一律為 `purpose = "reservation"`；順序＝查詢序 `id ASC`）。
    reservations: Vec<Assignment>,
}

/// 捕捉資產刪除前的保留（本地異動前呼叫；asset 不存在＝空清單，永不 404）。
pub async fn capture_asset_deletion(
    db: &SqlitePool,
    asset_id: i64,
) -> Result<PendingDeletes, ApiError> {
    let interfaces = interfaces::list_for_asset(db, asset_id)
        .await
        .map_err(|error| ApiError::internal("讀取介面失敗", error))?;
    let interface_ids: Vec<i64> = interfaces.iter().map(|interface| interface.id).collect();
    let reservations = assignments::list_reservations_for_interfaces(db, &interface_ids)
        .await
        .map_err(|error| ApiError::internal("讀取保留指派失敗", error))?;

    Ok(PendingDeletes { reservations })
}

/// 捕捉介面刪除前的保留（本地異動前呼叫；介面不存在＝空清單，永不 404）。
pub async fn capture_interface_deletion(
    db: &SqlitePool,
    interface_id: i64,
) -> Result<PendingDeletes, ApiError> {
    let reservations = assignments::list_reservations_for_interfaces(db, &[interface_id])
        .await
        .map_err(|error| ApiError::internal("讀取保留指派失敗", error))?;

    Ok(PendingDeletes { reservations })
}

/// 刪除後逐筆推送 Kea 保留刪除；未受管網段或未設 client 者不列入結果
/// （見 ADR-0027）。
pub async fn commit_deletions(
    db: &SqlitePool,
    kea: Option<&Client>,
    pending: PendingDeletes,
) -> Result<Vec<KeaSync>, ApiError> {
    let push = kea.map(HttpPush);
    commit_deletions_with(db, push.as_ref(), pending).await
}

/// [`commit_deletions`] 的內部版本：以 [`KeaPush`] port 注入（module 測試以
/// fake 呼叫）；公開函式以 [`HttpPush`] 包 `Option<&Client>` 後呼叫。
///
/// 逐筆（輸入序）刪除；網段以 `subnet_id` 快取（不存在→`ApiError`）；Kea
/// 失敗逐筆記 `failed`、續行。
async fn commit_deletions_with<P: KeaPush + ?Sized>(
    db: &SqlitePool,
    kea: Option<&P>,
    pending: PendingDeletes,
) -> Result<Vec<KeaSync>, ApiError> {
    let mut subnet_cache: HashMap<i64, Subnet> = HashMap::new();
    let mut syncs = Vec::new();
    for assignment in pending.reservations {
        if !subnet_cache.contains_key(&assignment.subnet_id) {
            let subnet = subnets::get(db, assignment.subnet_id)
                .await
                .map_err(|error| ApiError::internal("讀取網段失敗", error))?
                .ok_or_else(|| ApiError::internal("讀取網段失敗", "網段不存在"))?;
            subnet_cache.insert(subnet.id, subnet);
        }
        let subnet = &subnet_cache[&assignment.subnet_id];
        let Some(kea_subnet_id) = subnet.kea_subnet_id else {
            continue;
        };
        let Some(kea) = kea else {
            continue;
        };

        syncs.push(match kea.delete(kea_subnet_id, &assignment.address).await {
            Ok(()) => KeaSync::ok(),
            Err(error) => KeaSync::failed(&error),
        });
    }

    Ok(syncs)
}

/// 介面 MAC 編輯前的捕捉狀態；由 [`capture_interface_change`] 產生、餵給
/// [`commit_interface_change`]（欄位私有，route 僅持有）。
pub struct PendingInterfaceChange {
    /// 要比較 MAC 的介面 id。
    interface_id: i64,
    /// 異動前的 MAC；`None`＝介面不存在或原本即無 MAC。
    mac_before: Option<String>,
    /// 異動前該介面上的保留（順序＝查詢序 `id ASC`）。
    reservations: Vec<Assignment>,
}

/// 捕捉介面 MAC 編輯前的狀態（本地異動前呼叫；介面不存在＝MAC `None`、
/// 保留空，永不 404）。
pub async fn capture_interface_change(
    db: &SqlitePool,
    interface_id: i64,
) -> Result<PendingInterfaceChange, ApiError> {
    let mac_before = interfaces::get(db, interface_id)
        .await
        .map_err(|error| ApiError::internal("讀取介面失敗", error))?
        .and_then(|interface| interface.mac);
    let reservations = assignments::list_reservations_for_interfaces(db, &[interface_id])
        .await
        .map_err(|error| ApiError::internal("讀取保留指派失敗", error))?;

    Ok(PendingInterfaceChange {
        interface_id,
        mac_before,
        reservations,
    })
}

/// 介面 MAC 編輯後以新 MAC 重新推送該介面的保留（見 ADR-0027）；回傳 `None`
/// ＝此異動不涉及 Kea（介面不存在、MAC 未變或無可推送保留）。
pub async fn commit_interface_change(
    db: &SqlitePool,
    kea: Option<&Client>,
    pending: PendingInterfaceChange,
) -> Result<Option<Vec<KeaSync>>, ApiError> {
    let push = kea.map(HttpPush);
    commit_interface_change_with(db, push.as_ref(), pending).await
}

/// [`commit_interface_change`] 的內部版本：以 [`KeaPush`] port 注入（module
/// 測試以 fake 呼叫）；公開函式以 [`HttpPush`] 包 `Option<&Client>` 後呼叫。
///
/// 重讀介面（不存在→`None`）；MAC 未變→`None`；變更→對保留逐筆（輸入序）
/// 以新 MAC upsert；網段以 `subnet_id` 快取、未受管或無 client 跳過；介面
/// 無 MAC 記 `failed`；結果空→`None`。
async fn commit_interface_change_with<P: KeaPush + ?Sized>(
    db: &SqlitePool,
    kea: Option<&P>,
    pending: PendingInterfaceChange,
) -> Result<Option<Vec<KeaSync>>, ApiError> {
    let PendingInterfaceChange {
        interface_id,
        mac_before,
        reservations,
    } = pending;

    let interface = interfaces::get(db, interface_id)
        .await
        .map_err(|error| ApiError::internal("讀取介面失敗", error))?;
    let Some(interface) = interface else {
        return Ok(None);
    };
    if interface.mac == mac_before {
        return Ok(None);
    }

    let mut subnet_cache: HashMap<i64, Subnet> = HashMap::new();
    let mut syncs = Vec::new();
    for assignment in reservations {
        if !subnet_cache.contains_key(&assignment.subnet_id) {
            let subnet = subnets::get(db, assignment.subnet_id)
                .await
                .map_err(|error| ApiError::internal("讀取網段失敗", error))?
                .ok_or_else(|| ApiError::internal("讀取網段失敗", "網段不存在"))?;
            subnet_cache.insert(subnet.id, subnet);
        }
        let subnet = &subnet_cache[&assignment.subnet_id];
        let Some(kea_subnet_id) = subnet.kea_subnet_id else {
            continue;
        };
        let Some(kea) = kea else {
            continue;
        };

        let outcome = match &interface.mac {
            Some(mac) => {
                let record = ReservationRecord {
                    ip_address: assignment.address.clone(),
                    hw_address: mac.clone(),
                    hostname: assignment.hostname.clone(),
                };
                kea.upsert(kea_subnet_id, &record).await
            }
            None => Err(KeaError::Data(format!(
                "保留位址 {} 的介面缺少 MAC",
                assignment.address
            ))),
        };
        syncs.push(match outcome {
            Ok(()) => KeaSync::ok(),
            Err(error) => KeaSync::failed(&error),
        });
    }

    if syncs.is_empty() {
        return Ok(None);
    }
    Ok(Some(syncs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use crate::assets;
    use crate::assignments::AssignmentInput;
    use crate::interfaces::InterfaceInput;

    /// 建立測試資料庫並套用 migrations（比照 `disposed_assets.rs`）。
    async fn test_pool() -> SqlitePool {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
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

    /// 建立網段；`kea_subnet_id` 為 `Some` 即受管網段。
    async fn create_subnet(pool: &SqlitePool, cidr: &str, kea_subnet_id: Option<i64>) -> Subnet {
        subnets::create(
            pool,
            subnets::SubnetInput {
                cidr: Some(cidr.to_string()),
                kea_subnet_id,
                ..Default::default()
            }
            .validate()
            .expect("網段輸入"),
        )
        .await
        .expect("建立網段")
    }

    /// 建立資產，回傳 id。
    async fn create_asset(pool: &SqlitePool) -> i64 {
        sqlx::query("INSERT INTO assets (description, location) VALUES (?, ?)")
            .bind("測試機")
            .bind("機房 A")
            .execute(pool)
            .await
            .expect("新增資產")
            .last_insert_rowid()
    }

    /// 在指定資產建立介面（`mac = None`＝無 MAC 介面），回傳介面 id。
    async fn create_interface_on(pool: &SqlitePool, asset_id: i64, mac: Option<&str>) -> i64 {
        let valid = InterfaceInput {
            name: Some("eth0".to_string()),
            mac: mac.map(str::to_string),
            note: None,
        }
        .validate()
        .expect("介面輸入");
        interfaces::create(pool, asset_id, valid)
            .await
            .expect("新增介面")
            .id
    }

    /// 建立資產與介面（`mac = None`＝無 MAC 介面），回傳介面 id。
    async fn create_interface(pool: &SqlitePool, mac: Option<&str>) -> i64 {
        create_interface_on(pool, create_asset(pool).await, mac).await
    }

    /// 建立保留指派（hostname `pc-1`）；經領域驗證。
    async fn assign_reservation(
        pool: &SqlitePool,
        subnet: &Subnet,
        interface_id: i64,
        address: &str,
    ) {
        let valid = AssignmentInput {
            interface_id: Some(interface_id),
            purpose: Some("reservation".to_string()),
            hostname: Some("pc-1".to_string()),
        }
        .validate()
        .expect("保留輸入");
        assignments::assign(pool, subnet, address.parse().expect("位址"), valid)
            .await
            .expect("建立保留指派");
    }

    /// 建立 static 指派；經領域驗證。
    async fn assign_static(pool: &SqlitePool, subnet: &Subnet, interface_id: i64, address: &str) {
        let valid = AssignmentInput {
            interface_id: Some(interface_id),
            purpose: Some("static".to_string()),
            hostname: None,
        }
        .validate()
        .expect("static 輸入");
        assignments::assign(pool, subnet, address.parse().expect("位址"), valid)
            .await
            .expect("建立 static 指派");
    }

    /// 記錄的推送命令（供斷言順序與內容）。
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Command {
        Upsert {
            subnet_id: i64,
            record: ReservationRecord,
        },
        Delete {
            subnet_id: i64,
            address: String,
        },
    }

    /// 測試用推送 port：記錄命令序列、可注入失敗。
    #[derive(Default)]
    struct FakePush {
        commands: Mutex<Vec<Command>>,
        /// 若非 `None`：每個命令都回此失敗訊息。
        failure: Option<String>,
        /// 若非 `None`：僅此位址的 delete 回失敗（其餘成功）。
        fail_delete_address: Option<String>,
    }

    impl FakePush {
        /// 注入失敗：每個命令都先記錄再回失敗。
        fn failing(message: &str) -> Self {
            Self {
                commands: Mutex::new(Vec::new()),
                failure: Some(message.to_string()),
                fail_delete_address: None,
            }
        }

        /// 注入單筆失敗：只讓指定位址的 delete 失敗，其餘命令成功。
        fn failing_delete(address: &str) -> Self {
            Self {
                commands: Mutex::new(Vec::new()),
                failure: None,
                fail_delete_address: Some(address.to_string()),
            }
        }

        /// 已記錄的命令序列。
        fn commands(&self) -> Vec<Command> {
            self.commands.lock().expect("命令序列").clone()
        }
    }

    impl KeaPush for FakePush {
        async fn upsert(&self, subnet_id: i64, record: &ReservationRecord) -> Result<(), KeaError> {
            self.commands
                .lock()
                .expect("命令序列")
                .push(Command::Upsert {
                    subnet_id,
                    record: record.clone(),
                });
            match &self.failure {
                Some(message) => Err(KeaError::Data(message.clone())),
                None => Ok(()),
            }
        }

        async fn delete(&self, subnet_id: i64, address: &str) -> Result<(), KeaError> {
            self.commands
                .lock()
                .expect("命令序列")
                .push(Command::Delete {
                    subnet_id,
                    address: address.to_string(),
                });
            if self.fail_delete_address.as_deref() == Some(address) {
                return Err(KeaError::Data(format!("刪除失敗：{address}")));
            }
            match &self.failure {
                Some(message) => Err(KeaError::Data(message.clone())),
                None => Ok(()),
            }
        }
    }

    #[tokio::test]
    async fn reservation_assignment_upserts_once_with_fields() {
        let pool = test_pool().await;
        let subnet = create_subnet(&pool, "10.0.0.0/24", Some(7)).await;
        let interface_id = create_interface(&pool, Some("aa:bb:cc:dd:ee:01")).await;

        let pending = capture_assignment(&pool, "10.0.0.5".parse().expect("位址"))
            .await
            .expect("捕捉");
        assign_reservation(&pool, &subnet, interface_id, "10.0.0.5").await;

        let fake = FakePush::default();
        let sync = commit_assignment_with(&pool, Some(&fake), pending)
            .await
            .expect("推送")
            .expect("受管網段的保留應同步");

        assert_eq!(sync.status, "ok");
        assert_eq!(sync.message, None);
        assert_eq!(
            fake.commands(),
            vec![Command::Upsert {
                subnet_id: 7,
                record: ReservationRecord {
                    ip_address: "10.0.0.5".to_string(),
                    hw_address: "aa:bb:cc:dd:ee:01".to_string(),
                    hostname: Some("pc-1".to_string()),
                },
            }],
            "恰好一次 upsert、欄位取自 DB 現值"
        );
    }

    #[tokio::test]
    async fn reservation_to_static_and_cancel_delete_once() {
        let pool = test_pool().await;
        let subnet = create_subnet(&pool, "10.0.0.0/24", Some(7)).await;
        let interface_id = create_interface(&pool, Some("aa:bb:cc:dd:ee:01")).await;

        // 保留→static：刪除 Kea 保留一次。
        assign_reservation(&pool, &subnet, interface_id, "10.0.0.5").await;
        let pending = capture_assignment(&pool, "10.0.0.5".parse().expect("位址"))
            .await
            .expect("捕捉");
        assign_static(&pool, &subnet, interface_id, "10.0.0.5").await;
        let fake = FakePush::default();
        let sync = commit_assignment_with(&pool, Some(&fake), pending)
            .await
            .expect("推送")
            .expect("轉 static 應同步");
        assert_eq!(sync.status, "ok");
        assert_eq!(
            fake.commands(),
            vec![Command::Delete {
                subnet_id: 7,
                address: "10.0.0.5".to_string(),
            }]
        );

        // 取消保留：刪除 Kea 保留一次。
        assign_reservation(&pool, &subnet, interface_id, "10.0.0.5").await;
        let pending = capture_assignment(&pool, "10.0.0.5".parse().expect("位址"))
            .await
            .expect("捕捉");
        assert!(
            assignments::cancel(&pool, subnet.id, "10.0.0.5".parse().expect("位址"))
                .await
                .expect("取消指派")
        );
        let fake = FakePush::default();
        let sync = commit_assignment_with(&pool, Some(&fake), pending)
            .await
            .expect("推送")
            .expect("取消保留應同步");
        assert_eq!(sync.status, "ok");
        assert_eq!(
            fake.commands(),
            vec![Command::Delete {
                subnet_id: 7,
                address: "10.0.0.5".to_string(),
            }]
        );
    }

    #[tokio::test]
    async fn unmanaged_or_clientless_or_non_reservation_pushes_nothing() {
        let pool = test_pool().await;
        let managed = create_subnet(&pool, "10.0.0.0/24", Some(7)).await;
        let unmanaged = create_subnet(&pool, "10.1.0.0/24", None).await;
        let reserved_interface = create_interface(&pool, Some("aa:bb:cc:dd:ee:01")).await;
        let static_interface = create_interface(&pool, Some("aa:bb:cc:dd:ee:02")).await;

        // 未受管網段：即使指派保留也不互動。
        let pending = capture_assignment(&pool, "10.1.0.5".parse().expect("位址"))
            .await
            .expect("捕捉");
        assign_reservation(&pool, &unmanaged, reserved_interface, "10.1.0.5").await;
        let fake = FakePush::default();
        assert!(
            commit_assignment_with(&pool, Some(&fake), pending)
                .await
                .expect("推送")
                .is_none()
        );
        assert!(fake.commands().is_empty(), "未受管不推送");

        // 未設 client：受管網段亦不互動。
        let pending = capture_assignment(&pool, "10.0.0.5".parse().expect("位址"))
            .await
            .expect("捕捉");
        assign_reservation(&pool, &managed, reserved_interface, "10.0.0.5").await;
        assert!(
            commit_assignment_with(&pool, None::<&FakePush>, pending)
                .await
                .expect("推送")
                .is_none()
        );

        // 兩態皆非保留（static→static）：零命令、None。
        assign_static(&pool, &managed, static_interface, "10.0.0.6").await;
        let pending = capture_assignment(&pool, "10.0.0.6".parse().expect("位址"))
            .await
            .expect("捕捉");
        assign_static(&pool, &managed, static_interface, "10.0.0.6").await;
        assert!(
            commit_assignment_with(&pool, Some(&fake), pending)
                .await
                .expect("推送")
                .is_none()
        );

        // 兩態皆非保留（static→取消）：零命令、None。
        let pending = capture_assignment(&pool, "10.0.0.6".parse().expect("位址"))
            .await
            .expect("捕捉");
        assert!(
            assignments::cancel(&pool, managed.id, "10.0.0.6".parse().expect("位址"))
                .await
                .expect("取消指派")
        );
        assert!(
            commit_assignment_with(&pool, Some(&fake), pending)
                .await
                .expect("推送")
                .is_none()
        );
        assert!(fake.commands().is_empty(), "非保留不推送");
    }

    #[tokio::test]
    async fn reservation_without_mac_fails_without_upsert() {
        let pool = test_pool().await;
        let subnet = create_subnet(&pool, "10.0.0.0/24", Some(7)).await;
        let interface_id = create_interface(&pool, None).await;

        let pending = capture_assignment(&pool, "10.0.0.5".parse().expect("位址"))
            .await
            .expect("捕捉");
        // 直接入庫：指派領域會阻擋無 MAC 介面的保留。
        sqlx::query(
            "INSERT INTO ip_assignments (subnet_id, address, interface_id, purpose)
             VALUES (?, ?, ?, 'reservation')",
        )
        .bind(subnet.id)
        .bind("10.0.0.5")
        .bind(interface_id)
        .execute(&pool)
        .await
        .expect("新增保留指派");

        let fake = FakePush::default();
        let sync = commit_assignment_with(&pool, Some(&fake), pending)
            .await
            .expect("推送")
            .expect("保留缺 MAC 應回報 failed sync");

        assert_eq!(sync.status, "failed");
        assert_eq!(
            sync.message.as_deref(),
            Some("Kea 推送資料異常：保留位址 10.0.0.5 的介面缺少 MAC")
        );
        assert!(fake.commands().is_empty(), "無 MAC 不推送");
    }

    #[tokio::test]
    async fn kea_failure_reports_failed_and_keeps_local_state() {
        let pool = test_pool().await;
        let subnet = create_subnet(&pool, "10.0.0.0/24", Some(7)).await;
        let interface_id = create_interface(&pool, Some("aa:bb:cc:dd:ee:01")).await;

        let pending = capture_assignment(&pool, "10.0.0.5".parse().expect("位址"))
            .await
            .expect("捕捉");
        assign_reservation(&pool, &subnet, interface_id, "10.0.0.5").await;

        let fake = FakePush::failing("Kea 連線失敗");
        let sync = commit_assignment_with(&pool, Some(&fake), pending)
            .await
            .expect("推送")
            .expect("Kea 失敗仍回報同步結果");

        assert_eq!(sync.status, "failed");
        assert_eq!(
            sync.message.as_deref(),
            Some("Kea 推送資料異常：Kea 連線失敗")
        );
        assert_eq!(fake.commands().len(), 1, "失敗仍嘗試推送一次");

        let local = assignments::find(&pool, subnet.id, "10.0.0.5")
            .await
            .expect("讀取指派");
        assert_eq!(
            local.map(|assignment| assignment.purpose),
            Some("reservation".to_string()),
            "Kea 失敗不影響本地儲存"
        );
    }

    #[tokio::test]
    async fn asset_deletion_across_subnets_deletes_in_input_order() {
        let pool = test_pool().await;
        let subnet_second = create_subnet(&pool, "10.1.0.0/24", Some(8)).await;
        let subnet_first = create_subnet(&pool, "10.0.0.0/24", Some(7)).await;
        let asset_id = create_asset(&pool).await;
        let first_interface = create_interface_on(&pool, asset_id, Some("aa:bb:cc:dd:ee:01")).await;
        let second_interface =
            create_interface_on(&pool, asset_id, Some("aa:bb:cc:dd:ee:02")).await;

        // 先建的網段後建立保留：查詢序（id ASC）與網段 id 序相反。
        assign_reservation(&pool, &subnet_second, first_interface, "10.1.0.5").await;
        assign_reservation(&pool, &subnet_first, second_interface, "10.0.0.5").await;

        let pending = capture_asset_deletion(&pool, asset_id).await.expect("捕捉");
        assert!(
            assets::delete(&pool, asset_id).await.expect("刪除資產"),
            "本地刪除成功"
        );

        let fake = FakePush::default();
        let syncs = commit_deletions_with(&pool, Some(&fake), pending)
            .await
            .expect("推送");

        assert_eq!(syncs.len(), 2, "跨多網段逐筆推送");
        assert!(syncs.iter().all(|sync| sync.status == "ok"));
        assert_eq!(
            fake.commands(),
            vec![
                Command::Delete {
                    subnet_id: 8,
                    address: "10.1.0.5".to_string(),
                },
                Command::Delete {
                    subnet_id: 7,
                    address: "10.0.0.5".to_string(),
                },
            ],
            "順序＝捕捉查詢序"
        );
    }

    #[tokio::test]
    async fn deletion_skips_unmanaged_subnet_and_missing_client() {
        let pool = test_pool().await;
        let managed = create_subnet(&pool, "10.0.0.0/24", Some(7)).await;
        let unmanaged = create_subnet(&pool, "10.1.0.0/24", None).await;
        let asset_id = create_asset(&pool).await;
        let unmanaged_interface =
            create_interface_on(&pool, asset_id, Some("aa:bb:cc:dd:ee:01")).await;
        let managed_interface =
            create_interface_on(&pool, asset_id, Some("aa:bb:cc:dd:ee:02")).await;

        assign_reservation(&pool, &unmanaged, unmanaged_interface, "10.1.0.5").await;
        assign_reservation(&pool, &managed, managed_interface, "10.0.0.5").await;

        // 未受管網段：跳過、不產生同步結果；受管者照刪。
        let pending = capture_asset_deletion(&pool, asset_id).await.expect("捕捉");
        let fake = FakePush::default();
        let syncs = commit_deletions_with(&pool, Some(&fake), pending)
            .await
            .expect("推送");

        assert_eq!(syncs.len(), 1, "未受管不產生同步結果");
        assert_eq!(
            fake.commands(),
            vec![Command::Delete {
                subnet_id: 7,
                address: "10.0.0.5".to_string(),
            }]
        );

        // 未設 client：受管網段亦不互動、零命令。
        let pending = capture_interface_deletion(&pool, managed_interface)
            .await
            .expect("捕捉");
        let syncs = commit_deletions_with(&pool, None::<&FakePush>, pending)
            .await
            .expect("推送");
        assert!(syncs.is_empty(), "無 client 不產生同步結果");
        assert_eq!(fake.commands().len(), 1, "無 client 不推送");
    }

    #[tokio::test]
    async fn deletion_failure_reports_failed_and_continues() {
        let pool = test_pool().await;
        let subnet = create_subnet(&pool, "10.0.0.0/24", Some(7)).await;
        let asset_id = create_asset(&pool).await;
        let first_interface = create_interface_on(&pool, asset_id, Some("aa:bb:cc:dd:ee:01")).await;
        let second_interface =
            create_interface_on(&pool, asset_id, Some("aa:bb:cc:dd:ee:02")).await;

        assign_reservation(&pool, &subnet, first_interface, "10.0.0.5").await;
        assign_reservation(&pool, &subnet, second_interface, "10.0.0.6").await;

        let pending = capture_asset_deletion(&pool, asset_id).await.expect("捕捉");
        let fake = FakePush::failing_delete("10.0.0.5");
        let syncs = commit_deletions_with(&pool, Some(&fake), pending)
            .await
            .expect("推送");

        assert_eq!(syncs.len(), 2, "單筆失敗仍續行");
        assert_eq!(syncs[0].status, "failed");
        assert_eq!(
            syncs[0].message.as_deref(),
            Some("Kea 推送資料異常：刪除失敗：10.0.0.5")
        );
        assert_eq!(syncs[1].status, "ok");
        assert_eq!(fake.commands().len(), 2, "失敗後仍推送下一筆");
    }

    #[tokio::test]
    async fn missing_asset_capture_is_empty_not_404() {
        let pool = test_pool().await;

        let pending = capture_asset_deletion(&pool, 999)
            .await
            .expect("捕捉不 404");
        let fake = FakePush::default();
        let syncs = commit_deletions_with(&pool, Some(&fake), pending)
            .await
            .expect("推送");

        assert!(syncs.is_empty());
        assert!(fake.commands().is_empty());
    }

    /// 更新介面 MAC（經領域驗證；供 MAC 編輯系列測試）。
    async fn update_interface_mac(pool: &SqlitePool, interface_id: i64, mac: &str) {
        let valid = interfaces::ValidInterface {
            name: Some("eth0".to_string()),
            mac: Some(mac.to_string()),
            note: None,
        };
        interfaces::update(pool, interface_id, valid)
            .await
            .expect("更新介面")
            .expect("介面存在");
    }

    #[tokio::test]
    async fn interface_mac_change_upserts_reservations_in_input_order() {
        let pool = test_pool().await;
        let subnet_second = create_subnet(&pool, "10.1.0.0/24", Some(8)).await;
        let subnet_first = create_subnet(&pool, "10.0.0.0/24", Some(7)).await;
        let interface_id = create_interface(&pool, Some("aa:bb:cc:dd:ee:01")).await;

        // 先建的網段後建立保留：查詢序（id ASC）與網段 id 序相反。
        assign_reservation(&pool, &subnet_second, interface_id, "10.1.0.5").await;
        assign_reservation(&pool, &subnet_first, interface_id, "10.0.0.5").await;

        let pending = capture_interface_change(&pool, interface_id)
            .await
            .expect("捕捉");
        update_interface_mac(&pool, interface_id, "aa:bb:cc:dd:ee:02").await;

        let fake = FakePush::default();
        let syncs = commit_interface_change_with(&pool, Some(&fake), pending)
            .await
            .expect("推送")
            .expect("MAC 變更應同步");

        assert_eq!(syncs.len(), 2, "逐筆推送每筆保留");
        assert!(syncs.iter().all(|sync| sync.status == "ok"));
        assert_eq!(
            fake.commands(),
            vec![
                Command::Upsert {
                    subnet_id: 8,
                    record: ReservationRecord {
                        ip_address: "10.1.0.5".to_string(),
                        hw_address: "aa:bb:cc:dd:ee:02".to_string(),
                        hostname: Some("pc-1".to_string()),
                    },
                },
                Command::Upsert {
                    subnet_id: 7,
                    record: ReservationRecord {
                        ip_address: "10.0.0.5".to_string(),
                        hw_address: "aa:bb:cc:dd:ee:02".to_string(),
                        hostname: Some("pc-1".to_string()),
                    },
                },
            ],
            "順序＝捕捉查詢序、欄位＝新 MAC"
        );
    }

    #[tokio::test]
    async fn interface_mac_unchanged_pushes_nothing() {
        let pool = test_pool().await;
        let subnet = create_subnet(&pool, "10.0.0.0/24", Some(7)).await;
        let interface_id = create_interface(&pool, Some("aa:bb:cc:dd:ee:01")).await;
        assign_reservation(&pool, &subnet, interface_id, "10.0.0.5").await;

        let pending = capture_interface_change(&pool, interface_id)
            .await
            .expect("捕捉");
        // 只改備註：MAC 未變。
        let valid = interfaces::ValidInterface {
            name: Some("eth0".to_string()),
            mac: Some("aa:bb:cc:dd:ee:01".to_string()),
            note: Some("改備註".to_string()),
        };
        interfaces::update(&pool, interface_id, valid)
            .await
            .expect("更新介面")
            .expect("介面存在");

        let fake = FakePush::default();
        assert!(
            commit_interface_change_with(&pool, Some(&fake), pending)
                .await
                .expect("推送")
                .is_none(),
            "MAC 未變不產生同步結果"
        );
        assert!(fake.commands().is_empty(), "MAC 未變不推送");
    }

    #[tokio::test]
    async fn interface_mac_change_without_reservations_pushes_nothing() {
        let pool = test_pool().await;
        let interface_id = create_interface(&pool, Some("aa:bb:cc:dd:ee:01")).await;

        let pending = capture_interface_change(&pool, interface_id)
            .await
            .expect("捕捉");
        update_interface_mac(&pool, interface_id, "aa:bb:cc:dd:ee:02").await;

        let fake = FakePush::default();
        assert!(
            commit_interface_change_with(&pool, Some(&fake), pending)
                .await
                .expect("推送")
                .is_none(),
            "無保留不產生同步結果"
        );
        assert!(fake.commands().is_empty(), "無保留不推送");

        // 介面不存在：捕捉與推送皆不 404、不推送。
        let pending = capture_interface_change(&pool, 999)
            .await
            .expect("捕捉不 404");
        assert!(
            commit_interface_change_with(&pool, Some(&fake), pending)
                .await
                .expect("推送")
                .is_none()
        );
        assert!(fake.commands().is_empty(), "介面不存在不推送");
    }

    #[tokio::test]
    async fn interface_mac_change_skips_unmanaged_subnet_and_missing_client() {
        let pool = test_pool().await;
        let managed = create_subnet(&pool, "10.0.0.0/24", Some(7)).await;
        let unmanaged = create_subnet(&pool, "10.1.0.0/24", None).await;
        let interface_id = create_interface(&pool, Some("aa:bb:cc:dd:ee:01")).await;

        assign_reservation(&pool, &unmanaged, interface_id, "10.1.0.5").await;
        assign_reservation(&pool, &managed, interface_id, "10.0.0.5").await;

        // 未受管網段：跳過、不產生同步結果；受管者以新 MAC 重推。
        let pending = capture_interface_change(&pool, interface_id)
            .await
            .expect("捕捉");
        update_interface_mac(&pool, interface_id, "aa:bb:cc:dd:ee:02").await;
        let fake = FakePush::default();
        let syncs = commit_interface_change_with(&pool, Some(&fake), pending)
            .await
            .expect("推送")
            .expect("受管保留應同步");

        assert_eq!(syncs.len(), 1, "未受管不產生同步結果");
        assert_eq!(
            fake.commands(),
            vec![Command::Upsert {
                subnet_id: 7,
                record: ReservationRecord {
                    ip_address: "10.0.0.5".to_string(),
                    hw_address: "aa:bb:cc:dd:ee:02".to_string(),
                    hostname: Some("pc-1".to_string()),
                },
            }]
        );

        // 未設 client：受管網段亦不互動、零命令、None。
        let pending = capture_interface_change(&pool, interface_id)
            .await
            .expect("捕捉");
        update_interface_mac(&pool, interface_id, "aa:bb:cc:dd:ee:03").await;
        assert!(
            commit_interface_change_with(&pool, None::<&FakePush>, pending)
                .await
                .expect("推送")
                .is_none(),
            "無 client 不產生同步結果"
        );
        assert_eq!(fake.commands().len(), 1, "無 client 不推送");
    }
}
