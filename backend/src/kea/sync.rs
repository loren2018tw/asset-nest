//! Kea 保留推送與完整同步（見 `docs/adr/0011`）。
//!
//! 單筆：指派／改用途／改 hostname／取消指派時即時推送該筆保留（僅受管網段＋
//! 保留用途）；Kea 失敗僅警示、不阻擋本地儲存，由完整同步修復。
//! 完整同步：以 asset-nest 為準對齊受管網段的 Kea 保留（新增／更新／刪除）
//! 與網段層設定（pool、gateway；見 `docs/adr/0013`）；先產生計畫（dry-run）
//! 再套用；有衝突標記的指派跳過並列入報告。
//! upsert 以「先刪除（不存在視同已刪）再新增」實作，變更期間僅毫秒級空窗。

use std::collections::{HashMap, HashSet};
use std::net::Ipv4Addr;
use std::sync::Arc;

use ipnet::IpNet;
use serde::Serialize;
use serde_json::{Value, json};
use sqlx::SqlitePool;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use super::KeaError;
use super::http::{Client, Ipv4Range, ReservationRecord, is_routers_option, parse_pool_entry};
use crate::api::ApiError;
use crate::assignments::{self, Assignment};
use crate::conflicts;
use crate::interfaces;
use crate::subnets::Subnet;

/// 完整同步的併發上限。
const SYNC_CONCURRENCY: usize = 6;

/// 單筆推送結果（隨指派回應附帶；僅在應同步時出現）。
#[derive(Debug, Clone, Serialize)]
pub struct KeaSync {
    /// `ok` 或 `failed`。
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl KeaSync {
    fn ok() -> Self {
        Self {
            status: "ok",
            message: None,
        }
    }

    fn failed(error: &KeaError) -> Self {
        Self {
            status: "failed",
            message: Some(error.to_string()),
        }
    }
}

/// 指派異動後推送單筆保留；回傳 `None`＝此異動不涉及 Kea。
///
/// 觸發條件：受管網段（v4＋`kea_subnet_id`）且（新狀態為保留用途，或舊狀態為
/// 保留用途＝改為 static／取消指派時刪除）。Kea 失敗僅回警示（見 ADR-0011）。
pub async fn after_assignment_change(
    pool: &SqlitePool,
    client: Option<&Client>,
    subnet: &Subnet,
    before: Option<&Assignment>,
    after: Option<&Assignment>,
) -> Result<Option<KeaSync>, ApiError> {
    let (Some(kea_subnet_id), Some(client)) = (subnet.kea_subnet_id, client) else {
        return Ok(None);
    };

    let desired = after.filter(|assignment| assignment.purpose == "reservation");
    let before_was_reservation =
        before.is_some_and(|assignment| assignment.purpose == "reservation");

    if desired.is_none() && !before_was_reservation {
        return Ok(None);
    }

    let outcome = match desired {
        Some(assignment) => {
            let interface = interfaces::get(pool, assignment.interface_id)
                .await
                .map_err(|error| ApiError::internal("讀取介面失敗", error))?;
            match interface.and_then(|interface| interface.mac) {
                Some(mac) => {
                    let record = ReservationRecord {
                        ip_address: assignment.address.clone(),
                        hw_address: mac,
                        hostname: assignment.hostname.clone(),
                    };
                    upsert(client, kea_subnet_id, &record).await
                }
                None => Err(KeaError::Data(format!(
                    "保留位址 {} 的介面缺少 MAC",
                    assignment.address
                ))),
            }
        }
        None => {
            let assignment =
                before.ok_or_else(|| ApiError::internal("Kea 同步失敗", "缺少變更前的指派狀態"))?;
            match client
                .reservation_del(kea_subnet_id, &assignment.address)
                .await
            {
                Ok(changed) => {
                    if changed {
                        client.config_write().await
                    } else {
                        Ok(())
                    }
                }
                Err(error) => Err(error),
            }
        }
    };

    Ok(Some(match outcome {
        Ok(()) => KeaSync::ok(),
        Err(error) => KeaSync::failed(&error),
    }))
}

/// 幂等 upsert：先刪除（不存在視同已刪）再新增，最後 `config-write` 持久化。
async fn upsert(
    client: &Client,
    kea_subnet_id: i64,
    record: &ReservationRecord,
) -> Result<(), KeaError> {
    client
        .reservation_del(kea_subnet_id, &record.ip_address)
        .await?;
    client.reservation_add(kea_subnet_id, record).await?;
    client.config_write().await
}

/// 完整同步計畫（dry-run 回應；見 ADR-0011）。
#[derive(Debug, Serialize)]
pub struct SyncPlan {
    pub subnets: Vec<SyncPlanSubnet>,
    pub totals: SyncTotals,
}

#[derive(Debug, Default, Serialize)]
pub struct SyncTotals {
    pub add: usize,
    pub update: usize,
    pub delete: usize,
    pub skipped: usize,
    /// 要新增的 pool 筆數（見 ADR-0013）。
    pub pool_add: usize,
    /// 要刪除的 pool 筆數。
    pub pool_delete: usize,
    /// 要變更 gateway 的網段數（每網段至多 1）。
    pub gateway: usize,
}

#[derive(Debug, Serialize)]
pub struct SyncPlanSubnet {
    /// asset-nest 的網段 id。
    pub subnet_id: i64,
    pub cidr: String,
    pub name: Option<String>,
    pub kea_subnet_id: i64,
    pub add: Vec<PlanItem>,
    pub update: Vec<PlanItem>,
    pub delete: Vec<PlanItem>,
    pub skipped: Vec<PlanSkip>,
    /// 要新增的 pool 範圍（正規化 `start-end`）。
    pub pool_add: Vec<String>,
    /// 要刪除的 pool 範圍（正規化 `start-end`）。
    pub pool_delete: Vec<String>,
    /// gateway（routers option）變更；相同時省略。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway: Option<GatewayPlanItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// gateway（routers option）變更；`None`＝未設／移除。
#[derive(Debug, Serialize)]
pub struct GatewayPlanItem {
    pub current: Option<String>,
    pub desired: Option<String>,
}

/// 一筆變更；新增＝`desired`、刪除＝`current`、更新＝兩者。
#[derive(Debug, Serialize)]
pub struct PlanItem {
    pub ip_address: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desired: Option<RecordFields>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<RecordFields>,
}

/// 保留的可比對欄位。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RecordFields {
    pub hw_address: String,
    pub hostname: Option<String>,
}

/// 被跳過的項目（語意衝突、非 hw-address 形式的 Kea 保留）。
#[derive(Debug, Serialize)]
pub struct PlanSkip {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip_address: Option<String>,
    pub reason: String,
}

/// 完整同步套用報告。
#[derive(Debug, Serialize)]
pub struct SyncApplyReport {
    pub subnets: Vec<ApplySubnet>,
    /// `ok`／`failed`／`skipped`（無變更時不寫檔）。
    pub config_write: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_write_message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ApplySubnet {
    pub subnet_id: i64,
    pub cidr: String,
    pub name: Option<String>,
    pub kea_subnet_id: i64,
    pub added: usize,
    pub updated: usize,
    pub deleted: usize,
    pub skipped: usize,
    /// 成功新增的 pool 筆數（見 ADR-0013）。
    pub pool_added: usize,
    /// 成功刪除的 pool 筆數。
    pub pool_deleted: usize,
    /// gateway 是否已更新。
    pub gateway_updated: bool,
    /// 網段層（pool／gateway）套用失敗訊息；成功時省略。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settings_error: Option<String>,
    pub failures: Vec<ApplyFailure>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ApplyFailure {
    /// `add`／`update`／`delete`。
    pub action: &'static str,
    pub ip_address: String,
    pub message: String,
}

/// 產生完整同步計畫（唯讀；見 ADR-0011）。
pub async fn plan(pool: &SqlitePool, client: &Client) -> Result<SyncPlan, ApiError> {
    let plans = compute_plans(pool, client).await?;
    Ok(to_api_plan(plans))
}

/// 重算計畫並套用（新增／更新／刪除；見 ADR-0011）。
pub async fn apply(pool: &SqlitePool, client: &Client) -> Result<SyncApplyReport, ApiError> {
    let plans = compute_plans(pool, client).await?;
    let mut counts: Vec<[usize; 3]> = vec![[0; 3]; plans.len()];
    let mut failures: Vec<Vec<ApplyFailure>> = (0..plans.len()).map(|_| Vec::new()).collect();

    for phase in [Action::Delete, Action::Update, Action::Add] {
        let mut ops: Vec<(usize, Op)> = Vec::new();
        for (index, plan) in plans.iter().enumerate() {
            if plan.error.is_some() {
                continue;
            }
            match phase {
                Action::Delete => {
                    for (ip_address, _) in &plan.deletes {
                        ops.push((index, Op::delete(plan.kea_subnet_id, ip_address)));
                    }
                }
                Action::Update => {
                    for (ip_address, desired, _) in &plan.updates {
                        ops.push((
                            index,
                            Op::update(plan.kea_subnet_id, ip_address, desired.clone()),
                        ));
                    }
                }
                Action::Add => {
                    for (ip_address, desired) in &plan.adds {
                        ops.push((
                            index,
                            Op::add(plan.kea_subnet_id, ip_address, desired.clone()),
                        ));
                    }
                }
            }
        }

        for (index, op, result) in run_ops(client, ops).await {
            match result {
                Ok(()) => counts[index][phase.index()] += 1,
                Err(error) => failures[index].push(ApplyFailure {
                    action: phase.label(),
                    ip_address: op.ip_address.clone(),
                    message: error.to_string(),
                }),
            }
        }
    }

    // 網段層設定（pool／gateway）：逐網段序列執行、單一失敗續行（見 ADR-0013）。
    let mut settings: Vec<SettingsOutcome> = vec![SettingsOutcome::default(); plans.len()];
    let mut settings_mutated = false;
    for (index, plan) in plans.iter().enumerate() {
        if plan.error.is_some() || !plan.has_settings_change() {
            continue;
        }
        match apply_settings(client, plan).await {
            Ok(()) => {
                settings[index] = SettingsOutcome {
                    pool_added: plan.pool_adds.len(),
                    pool_deleted: plan.pool_deletes.len(),
                    gateway_updated: plan.gateway_change.is_some(),
                    error: None,
                };
                settings_mutated = true;
            }
            Err(error) => settings[index].error = Some(error.to_string()),
        }
    }

    let mutated: usize = counts
        .iter()
        .map(|counts| counts.iter().sum::<usize>())
        .sum();
    let (config_write, config_write_message) = if mutated == 0 && !settings_mutated {
        ("skipped", None)
    } else {
        match client.config_write().await {
            Ok(()) => ("ok", None),
            Err(error) => ("failed", Some(error.to_string())),
        }
    };

    let subnets = plans
        .into_iter()
        .zip(counts)
        .zip(failures)
        .zip(settings)
        .map(|(((plan, counts), failures), settings)| ApplySubnet {
            subnet_id: plan.subnet_id,
            cidr: plan.cidr,
            name: plan.name,
            kea_subnet_id: plan.kea_subnet_id,
            added: counts[Action::Add.index()],
            updated: counts[Action::Update.index()],
            deleted: counts[Action::Delete.index()],
            skipped: plan.skipped.len(),
            pool_added: settings.pool_added,
            pool_deleted: settings.pool_deleted,
            gateway_updated: settings.gateway_updated,
            settings_error: settings.error,
            failures,
            error: plan.error,
        })
        .collect();

    Ok(SyncApplyReport {
        subnets,
        config_write,
        config_write_message,
    })
}

/// 計算各受管網段的差異（供計畫與套用共用）。
async fn compute_plans(pool: &SqlitePool, client: &Client) -> Result<Vec<SubnetPlan>, ApiError> {
    let subnets = crate::subnets::list_full(pool)
        .await
        .map_err(|error| ApiError::internal("讀取網段失敗", error))?;
    let kea_subnets = client
        .config_get_dhcp4()
        .await
        .map_err(|error| ApiError::kea(format!("讀取 Kea 設定失敗：{error}")))?
        .subnets;

    let mut plans = Vec::new();
    for subnet in subnets {
        let Some(kea_subnet_id) = subnet.kea_subnet_id else {
            continue;
        };

        let mut plan = SubnetPlan {
            subnet_id: subnet.id,
            cidr: subnet.cidr.clone(),
            name: subnet.name.clone(),
            kea_subnet_id,
            adds: Vec::new(),
            updates: Vec::new(),
            deletes: Vec::new(),
            skipped: Vec::new(),
            desired_pools: Vec::new(),
            pool_adds: Vec::new(),
            pool_deletes: Vec::new(),
            gateway_change: None,
            raw_subnet: None,
            error: None,
        };

        let kea_subnet = match kea_subnets.get(&kea_subnet_id) {
            None => {
                plan.error = Some(format!("Kea 端沒有 subnet-id {kea_subnet_id}"));
                plans.push(plan);
                continue;
            }
            Some(kea_subnet) if !same_network(&subnet.cidr, &kea_subnet.cidr) => {
                plan.error = Some(format!(
                    "CIDR 不符：asset-nest {}／Kea {}",
                    subnet.cidr, kea_subnet.cidr
                ));
                plans.push(plan);
                continue;
            }
            Some(kea_subnet) => kea_subnet,
        };

        // 網段層設定差異（pool／gateway；見 ADR-0013）。
        plan.desired_pools = match subnet_pool_ranges(&subnet) {
            Ok(ranges) => ranges,
            Err(message) => {
                plan.error = Some(format!("讀取網段 pool 失敗：{message}"));
                plans.push(plan);
                continue;
            }
        };
        (plan.pool_adds, plan.pool_deletes) = pool_diff(&plan.desired_pools, &kea_subnet.pools);
        if subnet.gateway != kea_subnet.gateway {
            plan.gateway_change = Some(GatewayChange {
                current: kea_subnet.gateway.clone(),
                desired: subnet.gateway.clone(),
            });
        }
        plan.raw_subnet = Some(kea_subnet.raw.clone());

        let assignments = assignments::list_for_subnet(pool, subnet.id)
            .await
            .map_err(|error| ApiError::internal("讀取指派清單失敗", error))?;
        let conflict_map = conflicts::by_address(&subnet, &assignments)?;

        let mut untouchable = HashSet::new();
        let mut desired: Vec<(String, RecordFields)> = Vec::new();
        for assignment in &assignments {
            if assignment.purpose != "reservation" {
                continue;
            }

            if let Some(codes) = conflict_map.get(&assignment.address) {
                plan.skipped.push(PlanSkip {
                    ip_address: Some(assignment.address.clone()),
                    reason: format!("語意衝突（{}），跳過推送", codes.join("、")),
                });
                untouchable.insert(assignment.address.clone());
                continue;
            }

            match assignment.mac.clone() {
                Some(hw_address) => desired.push((
                    assignment.address.clone(),
                    RecordFields {
                        hw_address,
                        hostname: assignment.hostname.clone(),
                    },
                )),
                None => {
                    plan.skipped.push(PlanSkip {
                        ip_address: Some(assignment.address.clone()),
                        reason: "保留的介面缺少 MAC，跳過推送".to_string(),
                    });
                    untouchable.insert(assignment.address.clone());
                }
            }
        }

        let hosts = match client.reservation_get_all(kea_subnet_id).await {
            Ok(hosts) => hosts,
            Err(error) => {
                plan.error = Some(format!("讀取 Kea 保留失敗：{error}"));
                plans.push(plan);
                continue;
            }
        };

        let mut existing: HashMap<String, RecordFields> = HashMap::new();
        for host in hosts {
            match (host.ip_address, host.hw_address) {
                (Some(ip_address), Some(hw_address)) => {
                    existing.insert(
                        ip_address,
                        RecordFields {
                            hw_address,
                            hostname: host.hostname,
                        },
                    );
                }
                (ip_address, hw_address) => plan.skipped.push(PlanSkip {
                    ip_address,
                    reason: format!(
                        "Kea 端保留非 hw-address 形式（hw-address={}），跳過",
                        hw_address.unwrap_or_else(|| "無".to_string())
                    ),
                }),
            }
        }

        let (adds, updates, deletes) = diff(&desired, &existing, &untouchable);
        plan.adds = adds;
        plan.updates = updates;
        plan.deletes = deletes;
        plans.push(plan);
    }

    Ok(plans)
}

/// 單一網段的內部計畫資料。
struct SubnetPlan {
    subnet_id: i64,
    cidr: String,
    name: Option<String>,
    kea_subnet_id: i64,
    adds: Vec<(String, RecordFields)>,
    updates: Vec<(String, RecordFields, RecordFields)>,
    deletes: Vec<(String, RecordFields)>,
    skipped: Vec<PlanSkip>,
    /// 期望 pool（來自 asset-nest；正規化範圍）。
    desired_pools: Vec<Ipv4Range>,
    /// 要新增的 pool 範圍。
    pool_adds: Vec<Ipv4Range>,
    /// 要刪除的 pool 範圍。
    pool_deletes: Vec<Ipv4Range>,
    /// gateway 變更；相同時為 `None`。
    gateway_change: Option<GatewayChange>,
    /// 取自 `config-get` 的原始 `subnet4` 物件（供 `subnet4-update`）。
    raw_subnet: Option<Value>,
    error: Option<String>,
}

impl SubnetPlan {
    /// 是否有任何網段層（pool／gateway）變更。
    fn has_settings_change(&self) -> bool {
        !self.pool_adds.is_empty() || !self.pool_deletes.is_empty() || self.gateway_change.is_some()
    }
}

/// 單一網段的 gateway 變更。
struct GatewayChange {
    current: Option<String>,
    desired: Option<String>,
}

/// 單一網段網段層套用結果（內部彙總）。
#[derive(Debug, Clone, Default)]
struct SettingsOutcome {
    pool_added: usize,
    pool_deleted: usize,
    gateway_updated: bool,
    error: Option<String>,
}

/// 由 asset-nest 網段取出期望 pool 範圍（資料庫內容經結構驗證；異常視為內部錯誤）。
fn subnet_pool_ranges(subnet: &Subnet) -> Result<Vec<Ipv4Range>, String> {
    subnet
        .pools
        .iter()
        .map(|pool| {
            let start: Ipv4Addr = pool
                .start_ip
                .parse()
                .map_err(|_| format!("pool 起點格式異常：{}", pool.start_ip))?;
            let end: Ipv4Addr = pool
                .end_ip
                .parse()
                .map_err(|_| format!("pool 終點格式異常：{}", pool.end_ip))?;
            if start > end {
                return Err(format!("pool 範圍顛倒：{}-{}", pool.start_ip, pool.end_ip));
            }
            Ok(Ipv4Range { start, end })
        })
        .collect()
}

/// pool 差異：期望與 Kea 現值以「正規化範圍」比較（順序無關）。
///
/// 同範圍但 Kea 端帶額外屬性（如 client-classes）視為相同、不重建（見 ADR-0013）。
fn pool_diff(desired: &[Ipv4Range], current: &[Ipv4Range]) -> (Vec<Ipv4Range>, Vec<Ipv4Range>) {
    let mut adds: Vec<Ipv4Range> = desired
        .iter()
        .copied()
        .filter(|range| !current.contains(range))
        .collect();
    adds.sort();
    let mut deletes: Vec<Ipv4Range> = current
        .iter()
        .copied()
        .filter(|range| !desired.contains(range))
        .collect();
    deletes.sort();
    (adds, deletes)
}

/// 以 `subnet4-update` 套用單一網段的 pool／gateway（整段回寫；見 ADR-0013）。
///
/// 由 `config-get` 取得的原始網段物件複製後只改 `pools` 與 routers 條目；
/// 同範圍的既有 pool 條目原樣保留（屬性不遺失）。
async fn apply_settings(client: &Client, plan: &SubnetPlan) -> Result<(), KeaError> {
    let mut subnet = plan
        .raw_subnet
        .clone()
        .ok_or_else(|| KeaError::Data("缺少 Kea 網段原始設定".to_string()))?;

    let desired: HashSet<Ipv4Range> = plan.desired_pools.iter().copied().collect();
    let mut pools: Vec<Value> = Vec::new();
    if let Some(existing) = subnet.get("pools").and_then(Value::as_array) {
        for entry in existing {
            if parse_pool_entry(entry).is_some_and(|range| desired.contains(&range)) {
                pools.push(entry.clone());
            }
        }
    }
    let kept: HashSet<Ipv4Range> = pools.iter().filter_map(parse_pool_entry).collect();
    for range in &plan.desired_pools {
        if !kept.contains(range) {
            pools.push(json!({ "pool": range.to_kea_string() }));
        }
    }
    subnet["pools"] = Value::Array(pools);

    if let Some(change) = &plan.gateway_change {
        set_gateway_option(&mut subnet, change.desired.as_deref());
    }

    client.subnet4_update(&subnet).await
}

/// 設定或移除 `subnet4` 的 routers option（gateway）；其他 option 原樣保留。
fn set_gateway_option(subnet: &mut Value, gateway: Option<&str>) {
    let options = subnet.get_mut("option-data").and_then(Value::as_array_mut);
    match (options, gateway) {
        (Some(options), Some(gateway)) => {
            match options.iter_mut().find(|option| is_routers_option(option)) {
                Some(entry) => entry["data"] = json!(gateway),
                None => options.push(json!({
                    "name": "routers",
                    "code": 3,
                    "space": "dhcp4",
                    "data": gateway,
                })),
            }
        }
        (Some(options), None) => options.retain(|option| !is_routers_option(option)),
        (None, Some(gateway)) => {
            subnet["option-data"] = json!([{
                "name": "routers",
                "code": 3,
                "space": "dhcp4",
                "data": gateway,
            }]);
        }
        (None, None) => {}
    }
}

/// 差異計算：期望 vs Kea 現值；`untouchable`（衝突跳過者）不刪除也不更新。
fn diff(
    desired: &[(String, RecordFields)],
    existing: &HashMap<String, RecordFields>,
    untouchable: &HashSet<String>,
) -> (
    Vec<(String, RecordFields)>,
    Vec<(String, RecordFields, RecordFields)>,
    Vec<(String, RecordFields)>,
) {
    let mut adds = Vec::new();
    let mut updates = Vec::new();
    for (ip_address, fields) in desired {
        match existing.get(ip_address) {
            None => adds.push((ip_address.clone(), fields.clone())),
            Some(current) if current != fields => {
                updates.push((ip_address.clone(), fields.clone(), current.clone()));
            }
            Some(_) => {}
        }
    }

    let desired_ips: HashSet<&str> = desired
        .iter()
        .map(|(ip_address, _)| ip_address.as_str())
        .collect();
    let mut deletes = Vec::new();
    for (ip_address, current) in existing {
        if desired_ips.contains(ip_address.as_str()) || untouchable.contains(ip_address) {
            continue;
        }
        deletes.push((ip_address.clone(), current.clone()));
    }

    let key = |(ip_address, _): &(String, RecordFields)| assignments::address_sort_key(ip_address);
    adds.sort_by_key(key);
    updates.sort_by_key(|(ip_address, _, _)| assignments::address_sort_key(ip_address));
    deletes.sort_by_key(key);

    (adds, updates, deletes)
}

/// 內部計畫轉 API 回應（含總計）。
fn to_api_plan(plans: Vec<SubnetPlan>) -> SyncPlan {
    let mut totals = SyncTotals::default();
    let subnets = plans
        .into_iter()
        .map(|plan| {
            let add: Vec<PlanItem> = plan
                .adds
                .into_iter()
                .map(|(ip_address, desired)| PlanItem {
                    ip_address,
                    desired: Some(desired),
                    current: None,
                })
                .collect();
            let update: Vec<PlanItem> = plan
                .updates
                .into_iter()
                .map(|(ip_address, desired, current)| PlanItem {
                    ip_address,
                    desired: Some(desired),
                    current: Some(current),
                })
                .collect();
            let delete: Vec<PlanItem> = plan
                .deletes
                .into_iter()
                .map(|(ip_address, current)| PlanItem {
                    ip_address,
                    desired: None,
                    current: Some(current),
                })
                .collect();

            let pool_add: Vec<String> = plan
                .pool_adds
                .iter()
                .map(|range| range.to_compact_string())
                .collect();
            let pool_delete: Vec<String> = plan
                .pool_deletes
                .iter()
                .map(|range| range.to_compact_string())
                .collect();
            let gateway = plan.gateway_change.map(|change| GatewayPlanItem {
                current: change.current,
                desired: change.desired,
            });

            totals.add += add.len();
            totals.update += update.len();
            totals.delete += delete.len();
            totals.skipped += plan.skipped.len();
            totals.pool_add += pool_add.len();
            totals.pool_delete += pool_delete.len();
            totals.gateway += usize::from(gateway.is_some());

            SyncPlanSubnet {
                subnet_id: plan.subnet_id,
                cidr: plan.cidr,
                name: plan.name,
                kea_subnet_id: plan.kea_subnet_id,
                add,
                update,
                delete,
                skipped: plan.skipped,
                pool_add,
                pool_delete,
                gateway,
                error: plan.error,
            }
        })
        .collect();

    SyncPlan { subnets, totals }
}

/// 要执行的動作（刪除→更新→新增，分相執行）。
#[derive(Debug, Clone, Copy)]
enum Action {
    Add,
    Update,
    Delete,
}

impl Action {
    fn index(self) -> usize {
        match self {
            Action::Delete => 0,
            Action::Update => 1,
            Action::Add => 2,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Action::Add => "add",
            Action::Update => "update",
            Action::Delete => "delete",
        }
    }
}

/// 單一推送操作。
#[derive(Debug, Clone)]
struct Op {
    kea_subnet_id: i64,
    action: Action,
    ip_address: String,
    desired: Option<RecordFields>,
}

impl Op {
    fn add(kea_subnet_id: i64, ip_address: &str, desired: RecordFields) -> Self {
        Self {
            kea_subnet_id,
            action: Action::Add,
            ip_address: ip_address.to_string(),
            desired: Some(desired),
        }
    }

    fn update(kea_subnet_id: i64, ip_address: &str, desired: RecordFields) -> Self {
        Self {
            kea_subnet_id,
            action: Action::Update,
            ip_address: ip_address.to_string(),
            desired: Some(desired),
        }
    }

    fn delete(kea_subnet_id: i64, ip_address: &str) -> Self {
        Self {
            kea_subnet_id,
            action: Action::Delete,
            ip_address: ip_address.to_string(),
            desired: None,
        }
    }

    async fn run(&self, client: &Client) -> Result<(), KeaError> {
        let record = || ReservationRecord {
            ip_address: self.ip_address.clone(),
            hw_address: self
                .desired
                .as_ref()
                .map(|fields| fields.hw_address.clone())
                .unwrap_or_default(),
            hostname: self
                .desired
                .as_ref()
                .and_then(|fields| fields.hostname.clone()),
        };

        match self.action {
            Action::Add => client.reservation_add(self.kea_subnet_id, &record()).await,
            Action::Update => {
                client
                    .reservation_del(self.kea_subnet_id, &self.ip_address)
                    .await?;
                client.reservation_add(self.kea_subnet_id, &record()).await
            }
            Action::Delete => client
                .reservation_del(self.kea_subnet_id, &self.ip_address)
                .await
                .map(|_| ()),
        }
    }
}

/// 以受限併發執行操作；逐一收集結果（單項失敗不影響其他項）。
async fn run_ops(client: &Client, ops: Vec<(usize, Op)>) -> Vec<(usize, Op, Result<(), KeaError>)> {
    let semaphore = Arc::new(Semaphore::new(SYNC_CONCURRENCY));
    let mut set: JoinSet<(usize, Op, Result<(), KeaError>)> = JoinSet::new();

    for (index, op) in ops {
        let permit = semaphore
            .clone()
            .acquire_owned()
            .await
            .expect("取得同步併發許可");
        let client = client.clone();
        set.spawn(async move {
            let _permit = permit;
            let result = op.run(&client).await;
            (index, op, result)
        });
    }

    let mut results = Vec::new();
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok(item) => results.push(item),
            Err(error) => tracing::error!(error = %error, "Kea 同步任務異常結束"),
        }
    }

    results
}

/// 兩段 CIDR 是否為同一網路（正規化後比較）。
fn same_network(a: &str, b: &str) -> bool {
    match (a.parse::<IpNet>(), b.parse::<IpNet>()) {
        (Ok(a), Ok(b)) => a.trunc() == b.trunc(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(hw_address: &str, hostname: Option<&str>) -> RecordFields {
        RecordFields {
            hw_address: hw_address.to_string(),
            hostname: hostname.map(str::to_string),
        }
    }

    #[test]
    fn diff_classifies_add_update_delete_and_leaves_untouchable() {
        let desired = vec![
            ("10.0.0.2".to_string(), fields("aa:aa:aa:aa:aa:01", None)),
            (
                "10.0.0.3".to_string(),
                fields("aa:aa:aa:aa:aa:02", Some("pc-3")),
            ),
        ];
        let existing = HashMap::from([
            ("10.0.0.2".to_string(), fields("aa:aa:aa:aa:aa:01", None)),
            ("10.0.0.3".to_string(), fields("aa:aa:aa:aa:aa:02", None)),
            ("10.0.0.9".to_string(), fields("aa:aa:aa:aa:aa:09", None)),
            ("10.0.0.10".to_string(), fields("aa:aa:aa:aa:aa:10", None)),
            ("10.0.0.20".to_string(), fields("aa:aa:aa:aa:aa:20", None)),
        ]);
        let untouchable = HashSet::from(["10.0.0.20".to_string()]);

        let (adds, updates, deletes) = diff(&desired, &existing, &untouchable);

        assert!(adds.is_empty(), "同值不新增");
        assert_eq!(updates.len(), 1, "hostname 不同視為更新");
        assert_eq!(updates[0].0, "10.0.0.3");
        assert_eq!(updates[0].2.hostname, None, "附上 Kea 現值");
        assert_eq!(
            deletes
                .iter()
                .map(|(ip, _)| ip.as_str())
                .collect::<Vec<_>>(),
            vec!["10.0.0.9", "10.0.0.10"],
            "多餘保留刪除；untouchable（10.0.0.20）不動"
        );
    }

    #[test]
    fn diff_sorts_by_address_numerically() {
        let desired = vec![
            ("10.0.0.10".to_string(), fields("aa:aa:aa:aa:aa:10", None)),
            ("10.0.0.2".to_string(), fields("aa:aa:aa:aa:aa:02", None)),
        ];
        let (adds, _, _) = diff(&desired, &HashMap::new(), &HashSet::new());
        assert_eq!(
            adds.iter().map(|(ip, _)| ip.as_str()).collect::<Vec<_>>(),
            vec!["10.0.0.2", "10.0.0.10"],
            "依數值排序而非字串"
        );
    }

    #[test]
    fn pool_diff_adds_and_removes_by_normalized_range() {
        let range = |start: &str, end: &str| Ipv4Range {
            start: start.parse().expect("起點"),
            end: end.parse().expect("終點"),
        };
        let desired = vec![
            range("10.0.0.30", "10.0.0.40"),
            range("10.0.0.10", "10.0.0.20"),
        ];
        let current = vec![
            range("10.0.0.50", "10.0.0.60"),
            range("10.0.0.30", "10.0.0.40"),
        ];

        let (adds, deletes) = pool_diff(&desired, &current);

        assert_eq!(
            adds.iter()
                .map(|range| range.to_compact_string())
                .collect::<Vec<_>>(),
            vec!["10.0.0.10-10.0.0.20"],
            "缺少的補上、數值排序"
        );
        assert_eq!(
            deletes
                .iter()
                .map(|range| range.to_compact_string())
                .collect::<Vec<_>>(),
            vec!["10.0.0.50-10.0.0.60"],
            "同範圍不重建、多餘刪除"
        );
    }

    #[test]
    fn set_gateway_option_adds_replaces_and_removes_routers() {
        let mut subnet = json!({
            "option-data": [
                { "name": "domain-name-servers", "code": 6, "space": "dhcp4", "data": "10.0.0.53" },
                { "name": "routers", "code": 3, "space": "dhcp4", "data": "10.0.0.254" }
            ]
        });

        set_gateway_option(&mut subnet, Some("10.0.0.1"));
        assert_eq!(subnet["option-data"][1]["data"], "10.0.0.1", "改值");
        assert_eq!(
            subnet["option-data"][0]["name"], "domain-name-servers",
            "其他 option 保留"
        );

        let mut subnet = json!({});
        set_gateway_option(&mut subnet, Some("10.0.0.1"));
        assert_eq!(
            subnet["option-data"][0]["name"], "routers",
            "無 option-data 時新增"
        );

        set_gateway_option(&mut subnet, None);
        assert!(
            subnet["option-data"].as_array().expect("陣列").is_empty(),
            "gateway 未設＝移除 routers"
        );
    }
}
