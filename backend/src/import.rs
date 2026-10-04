//! 資產 CSV 匯入（Import）：編碼偵測、CSV 解析、逐列驗證、dry_run 預覽與單一交易寫入。
//!
//! 格式權威為 `docs/adr/0008`；解析規則、寫入語意與 API 形狀見
//! `.scratch/asset-csv-import/spec.md` §2–§5；兩層驗證見 `docs/adr/0006`
//! （結構錯誤阻擋、語意警示不擋）。本模組是單一驗證權威：預覽與正式匯入
//! 共用同一條驗證路徑，正式匯入以當下資料重驗後才寫入（見票 02）。

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use chrono::NaiveDate;
use ipnet::IpNet;
use serde::Serialize;
use sqlx::{SqliteConnection, SqlitePool};

use crate::api::ApiError;
use crate::assets::{AssetInput, ValidAsset, normalize_tags};
use crate::assignments::{self, AssignmentTarget};
use crate::interfaces::{self, InterfaceInput};
use crate::ips::HostRange;
use crate::subnets;

/// 檔案大小上限：5 MB（見 spec §2）。
pub(crate) const MAX_FILE_BYTES: usize = 5 * 1024 * 1024;
/// 資料列數上限：5,000 列（見 spec §2）。
pub(crate) const MAX_DATA_ROWS: usize = 5_000;

/// CSV 欄位（14 欄；順序與 ADR-0008 總表一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    PropertyNo = 0,
    Description,
    Location,
    DeviceSerial,
    Brand,
    Model,
    PurchaseDate,
    LifespanYears,
    Note,
    Tags,
    Mac,
    Ipv4,
    Ipv6,
    Hostname,
}

/// 欄位總數（見 ADR-0008「欄位總表（14 欄）」）。
const FIELD_COUNT: usize = 14;

impl Field {
    /// 由標題字串（trim、英文已折為小寫）對應欄位；未知標題回傳 `None`。
    fn parse(folded_header: &str) -> Option<Self> {
        Some(match folded_header {
            "財產編號" => Self::PropertyNo,
            "描述" => Self::Description,
            "位置" => Self::Location,
            "設備序號" => Self::DeviceSerial,
            "廠牌" => Self::Brand,
            "型號" => Self::Model,
            "購置日期" => Self::PurchaseDate,
            "年限" => Self::LifespanYears,
            "備註" => Self::Note,
            "標籤" => Self::Tags,
            "mac" => Self::Mac,
            "ipv4" => Self::Ipv4,
            "ipv6" => Self::Ipv6,
            "hostname" => Self::Hostname,
            _ => return None,
        })
    }

    /// 回應 `data`／問題 `field` 使用的英文欄位名。
    fn key(self) -> &'static str {
        match self {
            Self::PropertyNo => "property_no",
            Self::Description => "description",
            Self::Location => "location",
            Self::DeviceSerial => "device_serial",
            Self::Brand => "brand",
            Self::Model => "model",
            Self::PurchaseDate => "purchase_date",
            Self::LifespanYears => "lifespan_years",
            Self::Note => "note",
            Self::Tags => "tags",
            Self::Mac => "mac",
            Self::Ipv4 => "ipv4",
            Self::Ipv6 => "ipv6",
            Self::Hostname => "hostname",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// 解析後的原始資料列；文字已 trim、空白視為未填（見 ADR-0008）。
#[derive(Debug)]
struct RawRow {
    /// 列號＝資料記錄序號＋1（標題為第 1 列；空行不計，見 spec §2）。
    row_number: usize,
    /// 此列實際欄位數（供不一致的錯誤訊息）。
    column_count: usize,
    /// 標題欄位數（含未知標題；供不一致的錯誤訊息）。
    expected_columns: usize,
    /// 此列欄位數與標題不一致；只回報此結構錯誤，不做後續驗證。
    column_mismatch: bool,
    fields: [Option<String>; FIELD_COUNT],
}

impl RawRow {
    fn text(&self, field: Field) -> Option<String> {
        self.fields[field.index()].clone()
    }
}

/// 解析結果：資料列與忽略的未知標題。
#[derive(Debug)]
struct ParsedCsv {
    rows: Vec<RawRow>,
    ignored_headers: Vec<String>,
}

/// 逐列問題（回應 schema 見 spec §5.2）。
#[derive(Debug, Serialize)]
pub struct Issue {
    /// `warning` 或 `error`。
    pub severity: &'static str,
    pub code: &'static str,
    /// 對應的 CSV 欄位名；列級錯誤（如欄位數不一致）可為空。
    pub field: Option<String>,
    pub message: String,
}

impl Issue {
    fn error(code: &'static str, field: Option<Field>, message: impl Into<String>) -> Self {
        Self {
            severity: "error",
            code,
            field: field.map(|field| field.key().to_string()),
            message: message.into(),
        }
    }

    fn warning(code: &'static str, field: Option<Field>, message: impl Into<String>) -> Self {
        Self {
            severity: "warning",
            code,
            field: field.map(|field| field.key().to_string()),
            message: message.into(),
        }
    }
}

/// 每列 14 欄的正規化值（無法正規化者為 `None`）。
#[derive(Debug, Default, Serialize)]
pub struct RowData {
    pub property_no: Option<String>,
    pub description: Option<String>,
    pub location: Option<String>,
    pub device_serial: Option<String>,
    pub brand: Option<String>,
    pub model: Option<String>,
    pub purchase_date: Option<String>,
    pub lifespan_years: Option<i64>,
    pub note: Option<String>,
    pub tags: Vec<String>,
    pub mac: Option<String>,
    pub ipv4: Option<String>,
    pub ipv6: Option<String>,
    pub hostname: Option<String>,
}

/// 單列報告。
#[derive(Debug, Serialize)]
pub struct RowReport {
    pub row_number: usize,
    /// `ok`／`warning`／`error`＝取該列最高嚴重度。
    pub status: &'static str,
    pub data: RowData,
    pub issues: Vec<Issue>,
}

/// 匯入摘要。
#[derive(Debug, Serialize)]
pub struct Summary {
    pub total: usize,
    pub ok: usize,
    pub warnings: usize,
    pub errors: usize,
}

/// 正式匯入的建立統計。
#[derive(Debug, Default, Serialize)]
pub struct Created {
    pub assets: usize,
    pub interfaces: usize,
    pub assignments: usize,
}

/// 匯入回應（見 spec §5.2）。
#[derive(Debug, Serialize)]
pub struct ImportReport {
    pub dry_run: bool,
    /// 實際使用的編碼：`utf-8` 或 `big5`。
    pub encoding: &'static str,
    pub ignored_headers: Vec<String>,
    pub summary: Summary,
    pub rows: Vec<RowReport>,
    pub committed: bool,
    pub created: Option<Created>,
}

/// 已排定的指派（僅供交易內寫入，不進回應）。
pub(crate) struct PlannedAssignment {
    pub(crate) subnet_id: i64,
    pub(crate) address: String,
    pub(crate) purpose: &'static str,
    pub(crate) hostname: Option<String>,
}

/// 通過驗證、可寫入的列計畫；有結構錯誤的列為 `None`。
pub(crate) struct RowPlan {
    pub(crate) asset: ValidAsset,
    pub(crate) mac: Option<String>,
    pub(crate) ipv4: Option<PlannedAssignment>,
    pub(crate) ipv6: Option<PlannedAssignment>,
}

/// 分析結果：回應報告＋寫入計畫（`plans` 與 `report.rows` 同序）。
pub(crate) struct Analysis {
    pub(crate) report: ImportReport,
    pub(crate) plans: Vec<Option<RowPlan>>,
}

impl Analysis {
    pub(crate) fn has_errors(&self) -> bool {
        self.report.summary.errors > 0
    }
}

/// 既有資料的比對集合（僅語意警示用；見 spec §4.3）。
struct ExistingValues {
    property_no: HashSet<String>,
    device_serial: HashSet<String>,
    mac: HashSet<String>,
}

/// 逐列驗證的跨列狀態：既有值與檔內重複追蹤（值 → 首次出現列號）。
struct RowContext {
    existing: ExistingValues,
    property_no_seen: HashMap<String, usize>,
    device_serial_seen: HashMap<String, usize>,
    mac_seen: HashMap<String, usize>,
    ipv4_seen: HashMap<String, usize>,
    ipv6_seen: HashMap<String, usize>,
}

impl RowContext {
    fn new(existing: ExistingValues) -> Self {
        Self {
            existing,
            property_no_seen: HashMap::new(),
            device_serial_seen: HashMap::new(),
            mac_seen: HashMap::new(),
            ipv4_seen: HashMap::new(),
            ipv6_seen: HashMap::new(),
        }
    }
}

/// 分析整份匯入檔：解碼、解析、逐列驗證；檔級錯誤回 [`ApiError`] 400。
///
/// 不寫入任何資料；`dry_run=false` 時再由 [`commit`] 以同一份計畫寫入。
pub(crate) async fn analyze(pool: &SqlitePool, bytes: &[u8]) -> Result<Analysis, ApiError> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(file_error(format!(
            "檔案超過 5 MB 上限（目前 {} 位元組）",
            bytes.len()
        )));
    }

    let (text, encoding) = decode_file(bytes)?;
    let parsed = parse_csv(&text)?;
    let existing = load_existing(pool).await?;
    let mut context = RowContext::new(existing);

    let mut rows = Vec::with_capacity(parsed.rows.len());
    let mut plans = Vec::with_capacity(parsed.rows.len());
    for raw in &parsed.rows {
        let (report, plan) = validate_row(pool, raw, &mut context).await?;
        rows.push(report);
        plans.push(plan);
    }

    let summary = Summary {
        total: rows.len(),
        ok: rows.iter().filter(|row| row.status == "ok").count(),
        warnings: rows.iter().filter(|row| row.status == "warning").count(),
        errors: rows.iter().filter(|row| row.status == "error").count(),
    };

    Ok(Analysis {
        report: ImportReport {
            dry_run: false,
            encoding,
            ignored_headers: parsed.ignored_headers,
            summary,
            rows,
            committed: false,
            created: None,
        },
        plans,
    })
}

/// 在單一交易內寫入所有列（資產＋`eth0`＋指派）；任何失敗即整批回滾。
pub(crate) async fn commit(
    pool: &SqlitePool,
    plans: Vec<Option<RowPlan>>,
) -> Result<Created, ApiError> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| ApiError::internal("建立匯入交易失敗", error))?;
    let mut created = Created::default();

    match write_plans(&mut transaction, plans, &mut created).await {
        Ok(()) => {
            // 提交失敗即整批不寫入（交易解構時自動回滾）。
            transaction
                .commit()
                .await
                .map_err(|error| ApiError::internal("提交匯入交易失敗", error))?;
            Ok(created)
        }
        Err(error) => {
            let _ = transaction.rollback().await;
            Err(error)
        }
    }
}

/// 逐列寫入；呼叫端負責交易提交與回滾。
async fn write_plans(
    connection: &mut SqliteConnection,
    plans: Vec<Option<RowPlan>>,
    created: &mut Created,
) -> Result<(), ApiError> {
    for plan in plans.into_iter().flatten() {
        let asset_id = crate::assets::insert_asset(connection, plan.asset)
            .await
            .map_err(map_write_error)?;
        created.assets += 1;

        let interface_id = if plan.mac.is_some() || plan.ipv4.is_some() || plan.ipv6.is_some() {
            let valid = InterfaceInput {
                name: Some("eth0".to_string()),
                mac: plan.mac,
                note: None,
            }
            .validate()
            .map_err(|error| ApiError::internal("匯入介面驗證失敗", error.message().to_string()))?;

            let id = interfaces::insert_interface(connection, asset_id, valid)
                .await
                .map_err(map_write_error)?;
            created.interfaces += 1;
            Some(id)
        } else {
            None
        };

        for assignment in [plan.ipv4, plan.ipv6].into_iter().flatten() {
            let interface_id =
                interface_id.ok_or_else(|| ApiError::internal("匯入指派失敗", "缺少介面"))?;

            sqlx::query(
                "INSERT INTO ip_assignments (subnet_id, address, interface_id, purpose, hostname)
                 VALUES (?, ?, ?, ?, ?)",
            )
            .bind(assignment.subnet_id)
            .bind(&assignment.address)
            .bind(interface_id)
            .bind(assignment.purpose)
            .bind(assignment.hostname)
            .execute(&mut *connection)
            .await
            .map_err(map_write_error)?;
            created.assignments += 1;
        }
    }

    Ok(())
}

/// 資料庫唯一性等限制為結構規則的雙保險；預覽後資料被他人異動時整批回 400。
fn map_write_error(error: sqlx::Error) -> ApiError {
    if let sqlx::Error::Database(database_error) = &error {
        if database_error.is_unique_violation() {
            return file_error(
                "匯入時資料已變動（違反唯一性限制），已取消整批匯入；請重新預覽後再試",
            );
        }
    }
    ApiError::internal("匯入寫入失敗", error)
}

/// 編碼偵測：UTF-8 BOM → 嚴格 UTF-8 → Big5（CP950）。
///
/// UTF-16 BOM 或兩者皆無法解讀 → 400，提示另存為 UTF-8 或 Big5（見 spec §2）。
fn decode_file(bytes: &[u8]) -> Result<(String, &'static str), ApiError> {
    if bytes.len() >= 2
        && ((bytes[0] == 0xFF && bytes[1] == 0xFE) || (bytes[0] == 0xFE && bytes[1] == 0xFF))
    {
        return Err(file_error(
            "偵測到 UTF-16 編碼，請以 Excel 另存為 UTF-8 或 Big5（CP950）後重新上傳",
        ));
    }

    let body = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    if let Ok(text) = std::str::from_utf8(body) {
        return Ok((text.to_string(), "utf-8"));
    }

    let (text, _, had_errors) = encoding_rs::BIG5.decode(bytes);
    if had_errors {
        return Err(file_error(
            "檔案編碼無法解讀，請另存為 UTF-8 或 Big5（CP950）後重新上傳",
        ));
    }

    Ok((text.into_owned(), "big5"))
}

/// 解析 CSV：標題比對（trim、英文不分大小寫、順序不拘）與資料列收集。
///
/// 檔級錯誤：空檔案、缺「描述」或「位置」標題、重複標題、無資料列、超過列數上限。
/// 空行忽略；欄位數與標題不一致的列保留為錯誤列（見 spec §2）。
fn parse_csv(text: &str) -> Result<ParsedCsv, ApiError> {
    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(text.as_bytes());

    let headers = reader.headers().map_err(map_csv_error)?.clone();
    if headers.is_empty() {
        return Err(file_error("檔案為空或缺少標題列"));
    }
    let expected_columns = headers.len();

    let mut index_of: [Option<usize>; FIELD_COUNT] = [None; FIELD_COUNT];
    let mut seen: HashSet<String> = HashSet::new();
    let mut ignored_headers = Vec::new();
    for (index, header) in headers.iter().enumerate() {
        let trimmed = header.trim();
        if trimmed.is_empty() {
            continue;
        }
        let folded = trimmed.to_ascii_lowercase();
        if !seen.insert(folded.clone()) {
            return Err(file_error(format!("標題重複：「{trimmed}」")));
        }
        match Field::parse(&folded) {
            Some(field) => index_of[field.index()] = Some(index),
            None => ignored_headers.push(trimmed.to_string()),
        }
    }

    let mut missing = Vec::new();
    if index_of[Field::Description.index()].is_none() {
        missing.push("描述");
    }
    if index_of[Field::Location.index()].is_none() {
        missing.push("位置");
    }
    if !missing.is_empty() {
        return Err(file_error(format!("缺少必要標題：{}", missing.join("、"))));
    }

    let mut rows = Vec::new();
    for record in reader.records() {
        let record = record.map_err(map_csv_error)?;
        if is_blank(&record) {
            continue;
        }
        if rows.len() >= MAX_DATA_ROWS {
            return Err(file_error("資料列超過 5,000 列上限"));
        }

        let fields = std::array::from_fn(|field_index| {
            index_of[field_index]
                .and_then(|column| record.get(column))
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        });
        rows.push(RawRow {
            row_number: rows.len() + 2,
            column_count: record.len(),
            expected_columns,
            column_mismatch: record.len() != expected_columns,
            fields,
        });
    }

    if rows.is_empty() {
        return Err(file_error("檔案沒有資料列（僅有標題列）"));
    }

    Ok(ParsedCsv {
        rows,
        ignored_headers,
    })
}

/// 空行（含全空白列）忽略；正規 csv 讀取器已略過真正空行，此處補上全空白列。
fn is_blank(record: &csv::StringRecord) -> bool {
    record.is_empty()
        || (record.len() == 1 && record.get(0).is_some_and(|value| value.trim().is_empty()))
}

fn map_csv_error(error: csv::Error) -> ApiError {
    file_error(format!("CSV 解析失敗：{error}"))
}

fn file_error(message: impl Into<String>) -> ApiError {
    ApiError::validation(message).field("file")
}

/// 逐列驗證：14 欄正規化、結構規則（阻擋）與語意警示（不擋）。
///
/// 無結構錯誤時回傳可寫入的 [`RowPlan`]（見 spec §4）。
async fn validate_row(
    pool: &SqlitePool,
    raw: &RawRow,
    context: &mut RowContext,
) -> Result<(RowReport, Option<RowPlan>), ApiError> {
    let mut issues = Vec::new();

    if raw.column_mismatch {
        issues.push(Issue::error(
            "column_count_mismatch",
            None,
            format!(
                "此列 {} 欄與標題 {} 欄不一致",
                raw.column_count, raw.expected_columns
            ),
        ));
        return Ok((
            RowReport {
                row_number: raw.row_number,
                status: "error",
                data: mismatch_data(raw),
                issues,
            },
            None,
        ));
    }

    let property_no = raw.text(Field::PropertyNo);
    let description = raw.text(Field::Description);
    let location = raw.text(Field::Location);
    let device_serial = raw.text(Field::DeviceSerial);
    let brand = raw.text(Field::Brand);
    let model = raw.text(Field::Model);
    let note = raw.text(Field::Note);
    let hostname = raw.text(Field::Hostname);

    // 購置日期：三格式正規化為 YYYY-MM-DD（見 ADR-0008）。
    let purchase_date = match raw.text(Field::PurchaseDate) {
        Some(value) => match normalize_purchase_date(&value) {
            Some(normalized) => Some(normalized),
            None => {
                issues.push(Issue::error(
                    "invalid_purchase_date",
                    Some(Field::PurchaseDate),
                    format!("購置日期格式錯誤：{value}（須為 YYYY-MM-DD、YYYY/M/D 或 YYYY.M.D）"),
                ));
                None
            }
        },
        None => None,
    };

    // 年限：非負整數；負值交由 AssetInput::validate 統一回報。
    let lifespan_years = match raw.text(Field::LifespanYears) {
        Some(value) => match value.parse::<i64>() {
            Ok(years) => Some(years),
            Err(_) => {
                issues.push(Issue::error(
                    "invalid_lifespan",
                    Some(Field::LifespanYears),
                    format!("年限須為非負整數：{value}"),
                ));
                None
            }
        },
        None => None,
    };

    let tags = normalize_tags(
        raw.text(Field::Tags)
            .map(|value| value.split('|').map(str::to_string).collect())
            .unwrap_or_default(),
    );

    // MAC：正規化為小寫冒號（重用介面驗證）。
    let mac = match interfaces::normalize_mac(raw.text(Field::Mac)) {
        Ok(mac) => mac,
        Err(error) => {
            issues.push(Issue::error(
                "invalid_mac",
                Some(Field::Mac),
                error.message().to_string(),
            ));
            None
        }
    };

    // IPv4：格式、所屬 v4 網段、host／非 pool、未被指派、檔內不重複。
    let ipv4_address = raw.text(Field::Ipv4);
    let mut parsed_ipv4: Option<Ipv4Addr> = None;
    let mut planned_ipv4: Option<PlannedAssignment> = None;
    if let Some(value) = &ipv4_address {
        match value.parse::<Ipv4Addr>() {
            Ok(address) => {
                parsed_ipv4 = Some(address);
                let address_text = address.to_string();
                match subnets::find_by_address(pool, IpAddr::V4(address)).await? {
                    None => issues.push(Issue::error(
                        "ipv4_out_of_subnet",
                        Some(Field::Ipv4),
                        format!("IPv4 {address_text} 不在任何既有 v4 網段內"),
                    )),
                    Some(subnet) => {
                        // 用途推導：有 MAC → 保留；無 MAC → 手動（見 ADR-0008）。
                        let purpose = if mac.is_some() {
                            "reservation"
                        } else {
                            "static"
                        };
                        match assignments::validate_address(
                            &subnet,
                            IpAddr::V4(address),
                            purpose,
                            false,
                        ) {
                            Err(error) => {
                                let code = ipv4_rule_code(&subnet, address)?;
                                issues.push(Issue::error(
                                    code,
                                    Some(Field::Ipv4),
                                    error.message().to_string(),
                                ));
                            }
                            Ok(()) => {
                                match assignments::target_for_address(
                                    pool,
                                    subnet.id,
                                    &address_text,
                                )
                                .await
                                .map_err(|error| {
                                    ApiError::internal("讀取指派對象失敗", error)
                                })? {
                                    Some(target) => issues.push(Issue::error(
                                        "ipv4_assigned",
                                        Some(Field::Ipv4),
                                        format!(
                                            "IPv4 {address_text} 已被指派給{}；請先取消指派或改用其他位址",
                                            describe_target(&target)
                                        ),
                                    )),
                                    None => {
                                        planned_ipv4 = Some(PlannedAssignment {
                                            subnet_id: subnet.id,
                                            address: address_text.clone(),
                                            purpose,
                                            hostname: if purpose == "reservation" {
                                                hostname.clone()
                                            } else {
                                                None
                                            },
                                        });
                                    }
                                }
                            }
                        }
                    }
                }

                if let Some(previous) = context.ipv4_seen.get(&address_text).copied() {
                    issues.push(Issue::error(
                        "ipv4_duplicate",
                        Some(Field::Ipv4),
                        format!("IPv4 {address_text} 在檔案中重複（首次出現於列 {previous}）"),
                    ));
                } else {
                    context.ipv4_seen.insert(address_text, raw.row_number);
                }
            }
            Err(_) => issues.push(Issue::error(
                "invalid_ipv4",
                Some(Field::Ipv4),
                format!("IPv4 格式錯誤：{value}（須為合法 IPv4 位址）"),
            )),
        }
    }

    // IPv6：格式、所屬 v6 網段（登錄制、含 network 位址）、未被登錄、檔內不重複；用途恆 static。
    let ipv6_address = raw.text(Field::Ipv6);
    let mut parsed_ipv6: Option<Ipv6Addr> = None;
    let mut planned_ipv6: Option<PlannedAssignment> = None;
    if let Some(value) = &ipv6_address {
        match value.parse::<Ipv6Addr>() {
            Ok(address) => {
                parsed_ipv6 = Some(address);
                let address_text = address.to_string();
                match subnets::find_by_address(pool, IpAddr::V6(address)).await? {
                    None => issues.push(Issue::error(
                        "ipv6_out_of_subnet",
                        Some(Field::Ipv6),
                        format!("IPv6 {address_text} 不在任何既有 v6 網段內"),
                    )),
                    Some(subnet) => {
                        match assignments::target_for_address(pool, subnet.id, &address_text)
                            .await
                            .map_err(|error| ApiError::internal("讀取指派對象失敗", error))?
                        {
                            Some(target) => issues.push(Issue::error(
                                "ipv6_registered",
                                Some(Field::Ipv6),
                                format!(
                                    "IPv6 {address_text} 已登錄於{}；請改用其他位址",
                                    describe_target(&target)
                                ),
                            )),
                            None => {
                                planned_ipv6 = Some(PlannedAssignment {
                                    subnet_id: subnet.id,
                                    address: address_text.clone(),
                                    purpose: "static",
                                    hostname: None,
                                });
                            }
                        }
                    }
                }

                if let Some(previous) = context.ipv6_seen.get(&address_text).copied() {
                    issues.push(Issue::error(
                        "ipv6_duplicate",
                        Some(Field::Ipv6),
                        format!("IPv6 {address_text} 在檔案中重複（首次出現於列 {previous}）"),
                    ));
                } else {
                    context.ipv6_seen.insert(address_text, raw.row_number);
                }
            }
            Err(_) => issues.push(Issue::error(
                "invalid_ipv6",
                Some(Field::Ipv6),
                format!("IPv6 格式錯誤：{value}（須為合法 IPv6 位址）"),
            )),
        }
    }

    // hostname 僅「有 MAC＋IPv4」（保留）的列可填（見 ADR-0008）。
    if hostname.is_some() && !(mac.is_some() && parsed_ipv4.is_some()) {
        issues.push(Issue::error(
            "hostname_not_allowed",
            Some(Field::Hostname),
            "hostname 僅能在同時填寫有效 MAC 與 IPv4 的列使用",
        ));
    }

    // 資產欄位以正規化後的值交給 AssetInput::validate（單一驗證權威；見票 02）。
    let valid_asset = AssetInput {
        property_no: property_no.clone(),
        description: description.clone(),
        location: location.clone(),
        device_serial: device_serial.clone(),
        brand: brand.clone(),
        model: model.clone(),
        purchase_date: purchase_date.clone(),
        lifespan_years,
        note: note.clone(),
        tags: Some(tags.clone()),
    }
    .validate()
    .map_err(|error| {
        issues.push(asset_issue(error));
    })
    .ok();

    // 語意警示：重複財產編號／設備序號／MAC（檔內或既有；僅比對非空值、不擋）。
    warn_duplicates(
        &mut issues,
        context,
        &property_no,
        &device_serial,
        &mac,
        raw.row_number,
    );

    let status = if issues.iter().any(|issue| issue.severity == "error") {
        "error"
    } else if issues.is_empty() {
        "ok"
    } else {
        "warning"
    };

    let plan = if status == "error" {
        None
    } else {
        Some(RowPlan {
            asset: valid_asset
                .ok_or_else(|| ApiError::internal("匯入列驗證失敗", "缺少已驗證資產"))?,
            mac: mac.clone(),
            ipv4: planned_ipv4,
            ipv6: planned_ipv6,
        })
    };

    let data = RowData {
        property_no,
        description,
        location,
        device_serial,
        brand,
        model,
        purchase_date,
        lifespan_years,
        note,
        tags,
        mac,
        ipv4: parsed_ipv4.map(|address| address.to_string()),
        ipv6: parsed_ipv6.map(|address| address.to_string()),
        hostname,
    };

    Ok((
        RowReport {
            row_number: raw.row_number,
            status,
            data,
            issues,
        },
        plan,
    ))
}

/// 語意警示：重複財產編號／設備序號／MAC（檔內＋既有，皆只比對非空值）。
fn warn_duplicates(
    issues: &mut Vec<Issue>,
    context: &mut RowContext,
    property_no: &Option<String>,
    device_serial: &Option<String>,
    mac: &Option<String>,
    row_number: usize,
) {
    if let Some(value) = property_no {
        let folded = fold(value);
        if context.existing.property_no.contains(&folded) {
            issues.push(Issue::warning(
                "duplicate_property_no",
                Some(Field::PropertyNo),
                format!("財產編號 {value} 已存在於既有資產（僅提示，不阻擋匯入）"),
            ));
        }
        if let Some(previous) = context.property_no_seen.get(&folded).copied() {
            issues.push(Issue::warning(
                "duplicate_property_no",
                Some(Field::PropertyNo),
                format!("財產編號 {value} 在檔案中重複（首次出現於列 {previous}）"),
            ));
        } else {
            context.property_no_seen.insert(folded, row_number);
        }
    }

    if let Some(value) = device_serial {
        let folded = fold(value);
        if context.existing.device_serial.contains(&folded) {
            issues.push(Issue::warning(
                "duplicate_device_serial",
                Some(Field::DeviceSerial),
                format!("設備序號 {value} 已存在於既有資產（僅提示，不阻擋匯入）"),
            ));
        }
        if let Some(previous) = context.device_serial_seen.get(&folded).copied() {
            issues.push(Issue::warning(
                "duplicate_device_serial",
                Some(Field::DeviceSerial),
                format!("設備序號 {value} 在檔案中重複（首次出現於列 {previous}）"),
            ));
        } else {
            context.device_serial_seen.insert(folded, row_number);
        }
    }

    if let Some(value) = mac {
        let folded = fold(value);
        if context.existing.mac.contains(&folded) {
            issues.push(Issue::warning(
                "duplicate_mac",
                Some(Field::Mac),
                format!("MAC {value} 已被其他介面使用（僅提示，不阻擋匯入）"),
            ));
        }
        if let Some(previous) = context.mac_seen.get(&folded).copied() {
            issues.push(Issue::warning(
                "duplicate_mac",
                Some(Field::Mac),
                format!("MAC {value} 在檔案中重複（首次出現於列 {previous}）"),
            ));
        } else {
            context.mac_seen.insert(folded, row_number);
        }
    }
}

/// 將 `AssetInput::validate` 的錯誤轉為逐列問題（依欄位對應代碼）。
fn asset_issue(error: ApiError) -> Issue {
    let code = match error.field_name() {
        Some("description") => "description_required",
        Some("location") => "location_required",
        Some("purchase_date") => "invalid_purchase_date",
        Some("lifespan_years") => "invalid_lifespan",
        _ => "invalid_asset",
    };
    Issue {
        severity: "error",
        code,
        field: error.field_name().map(str::to_string),
        message: error.message().to_string(),
    }
}

/// IPv4 結構錯誤的細分代碼：host 範圍／pool／其他（重用 [`HostRange`] 與 pool 判定）。
fn ipv4_rule_code(subnet: &subnets::Subnet, address: Ipv4Addr) -> Result<&'static str, ApiError> {
    let network: IpNet = subnet
        .cidr
        .parse()
        .map_err(|error| ApiError::internal("網段 CIDR 格式錯誤", error))?;
    if let IpNet::V4(network) = network {
        if !HostRange::of(&network).contains(address) {
            return Ok("ipv4_not_host");
        }
    }
    if assignments::is_in_pool(subnet, address)? {
        return Ok("ipv4_in_pool");
    }
    Ok("invalid_ipv4")
}

/// 「位址已被指派／已登錄」訊息中的目前指派對象（比照 ADR-0007 提示資訊樣式）。
fn describe_target(target: &AssignmentTarget) -> String {
    let interface = match (&target.interface_name, &target.mac) {
        (Some(name), Some(mac)) => format!("介面 {name}（MAC {mac}）"),
        (Some(name), None) => format!("介面 {name}"),
        (None, Some(mac)) => format!("未命名介面（MAC {mac}）"),
        (None, None) => "未命名介面".to_string(),
    };
    format!(
        "資產「{}」（位置：{}）的{interface}",
        target.asset_description, target.asset_location
    )
}

/// 購置日期：`YYYY-MM-DD`／`YYYY/M/D`／`YYYY.M.D` → `YYYY-MM-DD`；無法解析回傳 `None`。
///
/// 破折號格式要求月／日補零（`YYYY-MM-DD`）；斜線與點號容忍不補零。
fn normalize_purchase_date(text: &str) -> Option<String> {
    if let Ok(date) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        let bytes = text.as_bytes();
        if text.len() == 10 && bytes[4] == b'-' && bytes[7] == b'-' {
            return Some(date.format("%Y-%m-%d").to_string());
        }
    }
    for format in ["%Y/%m/%d", "%Y.%m.%d"] {
        if let Ok(date) = NaiveDate::parse_from_str(text, format) {
            return Some(date.format("%Y-%m-%d").to_string());
        }
    }
    None
}

/// 欄位數不一致的列：只填可取得的原始值（僅供問題列報告辨識）。
fn mismatch_data(raw: &RawRow) -> RowData {
    let tags = normalize_tags(
        raw.text(Field::Tags)
            .map(|value| value.split('|').map(str::to_string).collect())
            .unwrap_or_default(),
    );
    RowData {
        property_no: raw.text(Field::PropertyNo),
        description: raw.text(Field::Description),
        location: raw.text(Field::Location),
        device_serial: raw.text(Field::DeviceSerial),
        brand: raw.text(Field::Brand),
        model: raw.text(Field::Model),
        purchase_date: raw.text(Field::PurchaseDate),
        lifespan_years: raw
            .text(Field::LifespanYears)
            .and_then(|value| value.parse::<i64>().ok()),
        note: raw.text(Field::Note),
        tags,
        mac: raw.text(Field::Mac),
        ipv4: raw.text(Field::Ipv4),
        ipv6: raw.text(Field::Ipv6),
        hostname: raw.text(Field::Hostname),
    }
}

/// 讀取既有資料的比對集合；一律排除空值（見 spec §4.3）。
async fn load_existing(pool: &SqlitePool) -> Result<ExistingValues, ApiError> {
    Ok(ExistingValues {
        property_no: existing_values(
            pool,
            "SELECT property_no FROM assets
              WHERE property_no IS NOT NULL AND trim(property_no) <> ''",
        )
        .await?,
        device_serial: existing_values(
            pool,
            "SELECT device_serial FROM assets
              WHERE device_serial IS NOT NULL AND trim(device_serial) <> ''",
        )
        .await?,
        mac: existing_values(
            pool,
            "SELECT mac FROM interfaces WHERE mac IS NOT NULL AND trim(mac) <> ''",
        )
        .await?,
    })
}

async fn existing_values(pool: &SqlitePool, sql: &str) -> Result<HashSet<String>, ApiError> {
    let values: Vec<String> = sqlx::query_scalar(sql)
        .fetch_all(pool)
        .await
        .map_err(|error| ApiError::internal("讀取既有資料失敗", error))?;
    Ok(values.into_iter().map(|value| fold(&value)).collect())
}

/// 重複比對用的折疊：trim＋ASCII 不分大小寫（與系統既有 `COLLATE NOCASE` 語意一致）。
fn fold(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_file_accepts_utf8_with_and_without_bom() {
        let text = "描述,位置\n機台,機房\n";
        let (decoded, encoding) = decode_file(text.as_bytes()).expect("UTF-8");
        assert_eq!(encoding, "utf-8");
        assert_eq!(decoded, text);

        let mut with_bom = vec![0xEF, 0xBB, 0xBF];
        with_bom.extend_from_slice(text.as_bytes());
        let (decoded, encoding) = decode_file(&with_bom).expect("UTF-8 BOM");
        assert_eq!(encoding, "utf-8");
        assert_eq!(decoded, text, "BOM 須去除");
    }

    #[test]
    fn decode_file_falls_back_to_big5() {
        let source = "描述,位置\n測試機,機房A\n";
        let (bytes, _, _) = encoding_rs::BIG5.encode(source);
        let (decoded, encoding) = decode_file(&bytes).expect("Big5");
        assert_eq!(encoding, "big5");
        assert_eq!(decoded, source);
    }

    #[test]
    fn decode_file_rejects_utf16_and_undecodable_bytes() {
        let utf16 = [0xFF, 0xFE, 0x41, 0x00];
        let error = decode_file(&utf16).expect_err("UTF-16 應拒絕");
        assert!(error.message().contains("UTF-16"));

        // 0xFF 0xFF 不是合法 Big5，也不是合法 UTF-8。
        let broken = [0xFF, 0xFF, 0xFF, 0xFF];
        let error = decode_file(&broken).expect_err("無法解讀應拒絕");
        assert!(error.message().contains("Big5"));
    }

    #[test]
    fn normalize_purchase_date_accepts_three_formats_and_rejects_invalid() {
        for (input, expected) in [
            ("2024-01-15", "2024-01-15"),
            ("2024/1/15", "2024-01-15"),
            ("2024.1.5", "2024-01-05"),
            ("2024/12/31", "2024-12-31"),
        ] {
            assert_eq!(
                normalize_purchase_date(input).as_deref(),
                Some(expected),
                "{input}"
            );
        }
        for input in ["2024-1-5", "2024/13/01", "2024-02-30", "abc", ""] {
            assert_eq!(normalize_purchase_date(input), None, "{input} 應拒絕");
        }
    }

    #[test]
    fn parse_csv_maps_headers_case_insensitively_in_any_order() {
        let parsed = parse_csv("位置,描述,MAC,IPV4\n機房A,機台,AA-BB-CC-DD-EE-FF,10.0.0.5\n")
            .expect("解析成功");
        assert!(parsed.ignored_headers.is_empty());
        let row = &parsed.rows[0];
        assert_eq!(row.row_number, 2);
        assert!(!row.column_mismatch);
        assert_eq!(row.text(Field::Location).as_deref(), Some("機房A"));
        assert_eq!(row.text(Field::Description).as_deref(), Some("機台"));
        assert_eq!(row.text(Field::Mac).as_deref(), Some("AA-BB-CC-DD-EE-FF"));
        assert_eq!(row.text(Field::Ipv4).as_deref(), Some("10.0.0.5"));
    }

    #[test]
    fn parse_csv_reports_ignored_headers() {
        // 注意：解析層不做欄位驗證，未知標題只列入 ignored_headers。
        let parsed = parse_csv("描述,位置,數量,金額\n機台,機房,3,100\n").expect("解析成功");
        assert_eq!(parsed.ignored_headers, vec!["數量", "金額"]);
    }

    #[test]
    fn parse_csv_rejects_missing_and_duplicate_headers() {
        let error = parse_csv("描述,數量\n機台,3\n").expect_err("缺位置應拒絕");
        assert!(error.message().contains("缺少必要標題"));
        assert!(error.message().contains("位置"));

        let error = parse_csv("描述,描述,位置\n機台,機台,機房\n").expect_err("重複標題應拒絕");
        assert!(error.message().contains("標題重複"));
        assert!(error.message().contains("描述"));

        let error = parse_csv("").expect_err("空檔案應拒絕");
        assert!(error.message().contains("空"));

        let error = parse_csv("描述,位置\n").expect_err("無資料列應拒絕");
        assert!(error.message().contains("沒有資料列"));
    }

    #[test]
    fn parse_csv_skips_blank_lines_and_numbers_rows_sequentially() {
        let parsed = parse_csv("描述,位置\n\n機台一,機房A\n   \n機台二,機房B\n").expect("解析成功");
        let numbers: Vec<usize> = parsed.rows.iter().map(|row| row.row_number).collect();
        assert_eq!(numbers, vec![2, 3], "空行與全空白列忽略");
        assert_eq!(
            parsed.rows[1].text(Field::Description).as_deref(),
            Some("機台二")
        );
    }

    #[test]
    fn parse_csv_flags_column_count_mismatch() {
        let parsed =
            parse_csv("描述,位置\n只有一欄\n機台,機房\n機台,機房,多餘\n").expect("解析成功");
        assert!(parsed.rows[0].column_mismatch);
        assert_eq!(parsed.rows[0].column_count, 1);
        assert!(!parsed.rows[1].column_mismatch);
        assert_eq!(parsed.rows[1].column_count, 2);
        assert!(parsed.rows[2].column_mismatch);
        assert_eq!(parsed.rows[2].column_count, 3);
    }

    #[test]
    fn parse_csv_handles_rfc4180_quotes_and_crlf() {
        let parsed = parse_csv("描述,位置,備註\r\n\"伺服器, 第一台\r\n第二行\",機房A,備註\r\n")
            .expect("解析成功");
        assert_eq!(parsed.rows.len(), 1);
        let description = parsed.rows[0].text(Field::Description).expect("描述");
        assert!(description.contains("伺服器, 第一台"));
        assert!(description.contains("第二行"), "引號內換行屬同一列");
    }
}
