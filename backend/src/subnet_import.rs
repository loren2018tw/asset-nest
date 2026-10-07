//! 網段 CSV 匯入（Import）：編碼偵測、CSV 解析、逐列驗證、dry_run 預覽與單一交易寫入。
//!
//! 格式權威為 `docs/adr/0009`；解析規則、寫入語意與 API 形狀見
//! `.scratch/csv-export-import/spec.md` §2–§3、§5；兩層驗證見 `docs/adr/0006`
//! （網段匯入僅結構錯誤，無語意警示）。本模組是單一驗證權威：預覽與正式匯入
//! 共用同一條驗證路徑，正式匯入以當下資料重驗後才寫入（見票 02）。
//! 欄位驗證與寫入原語沿用 [`crate::subnets`]（`SubnetInput::validate`／`insert_subnet`）。

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr};

use ipnet::IpNet;
use serde::Serialize;
use sqlx::{SqliteConnection, SqlitePool};

use crate::api::ApiError;
use crate::subnets::{self, ExclusionInput, PoolInput, SubnetInput, ValidSubnet};

/// 檔案大小上限：5 MB（見 spec §2）。
pub(crate) const MAX_FILE_BYTES: usize = 5 * 1024 * 1024;
/// 資料列數上限：5,000 列（見 spec §2）。
pub(crate) const MAX_DATA_ROWS: usize = 5_000;

/// CSV 欄位（7 欄；順序與 ADR-0009／0020 總表一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Name = 0,
    Cidr,
    Gateway,
    KeaSubnetId,
    Pools,
    Exclusions,
    Note,
}

/// 欄位總數（見 ADR-0009／0020）。
const FIELD_COUNT: usize = 7;

impl Field {
    /// 由標題字串（trim、英文已折為小寫）對應欄位；未知標題回傳 `None`。
    fn parse(folded_header: &str) -> Option<Self> {
        Some(match folded_header {
            "名稱" => Self::Name,
            "cidr" => Self::Cidr,
            "gateway" => Self::Gateway,
            "kea subnet-id" => Self::KeaSubnetId,
            "位址池" => Self::Pools,
            "排除範圍" => Self::Exclusions,
            "備註" => Self::Note,
            _ => return None,
        })
    }

    /// 回應 `data`／問題 `field` 使用的英文欄位名。
    fn key(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Cidr => "cidr",
            Self::Gateway => "gateway",
            Self::KeaSubnetId => "kea_subnet_id",
            Self::Pools => "pools",
            Self::Exclusions => "exclusions",
            Self::Note => "note",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// 解析後的原始資料列；文字已 trim、空白視為未填（見 ADR-0008／0009）。
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

/// 逐列問題（回應 schema 見 spec §5）。
#[derive(Debug, Serialize)]
pub struct Issue {
    /// `warning` 或 `error`（網段匯入無語意警示，實務上僅 `error`）。
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
}

/// 每列 7 欄的正規化值（無法正規化者為 `None`／空陣列）。
#[derive(Debug, Default, Serialize)]
pub struct RowData {
    pub name: Option<String>,
    /// 正規化 CIDR（host bits 收斂）；無法解析者為 `null`。
    pub cidr: Option<String>,
    pub gateway: Option<String>,
    pub kea_subnet_id: Option<i64>,
    /// 正規化位址池：每段 `起點-終點`。
    pub pools: Vec<String>,
    /// 正規化排除範圍：每段 `起-迄` 或 `起-迄#用途說明`。
    pub exclusions: Vec<String>,
    pub note: Option<String>,
}

/// 單列報告。
#[derive(Debug, Serialize)]
pub struct RowReport {
    pub row_number: usize,
    /// `ok`／`error`＝取該列最高嚴重度。
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
    pub subnets: usize,
}

/// 匯入回應（見 spec §5）。
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

/// 分析結果：回應報告＋寫入計畫（`plans` 與 `report.rows` 同序）。
pub(crate) struct Analysis {
    pub(crate) report: ImportReport,
    /// 通過驗證、可寫入的網段；有結構錯誤的列為 `None`。
    pub(crate) plans: Vec<Option<ValidSubnet>>,
}

impl Analysis {
    pub(crate) fn has_errors(&self) -> bool {
        self.report.summary.errors > 0
    }
}

/// 既有網段的比對集合（CIDR 重疊與 Kea subnet-id 重複）。
struct ExistingSubnets {
    subnets: Vec<ExistingSubnet>,
    /// kea_subnet_id →（CIDR 文字、名稱）；僅供錯誤訊息。
    kea_ids: HashMap<i64, (String, Option<String>)>,
}

/// 既有網段（僅匯入比對所需欄位）。
struct ExistingSubnet {
    network: IpNet,
    text: String,
    name: Option<String>,
}

/// 逐列驗證的跨列狀態：既有網段與檔內追蹤（值 → 首次出現列號）。
struct RowContext {
    existing: ExistingSubnets,
    /// 正規化 CIDR 文字 → 首次出現列號（完全相同者）。
    file_cidrs: HashMap<String, usize>,
    /// 檔內已出現的網段（供嵌套／重疊比對）。
    file_nets: Vec<RegisteredNet>,
    /// kea_subnet_id → 首次出現列號。
    file_kea_ids: HashMap<i64, usize>,
}

impl RowContext {
    fn new(existing: ExistingSubnets) -> Self {
        Self {
            existing,
            file_cidrs: HashMap::new(),
            file_nets: Vec::new(),
            file_kea_ids: HashMap::new(),
        }
    }
}

/// 檔內已出現的網段。
struct RegisteredNet {
    network: IpNet,
    row_number: usize,
    text: String,
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
        let (report, plan) = validate_row(raw, &mut context)?;
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

/// 在單一交易內寫入所有列；任何失敗即整批回滾。
pub(crate) async fn commit(
    pool: &SqlitePool,
    plans: Vec<Option<ValidSubnet>>,
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
    plans: Vec<Option<ValidSubnet>>,
    created: &mut Created,
) -> Result<(), ApiError> {
    for valid in plans.into_iter().flatten() {
        subnets::insert_subnet(connection, valid)
            .await
            .map_err(map_write_error)?;
        created.subnets += 1;
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
/// 檔級錯誤：空檔案、缺「CIDR」標題、重複標題、無資料列、超過列數上限。
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
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
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

    if index_of[Field::Cidr.index()].is_none() {
        return Err(file_error("缺少必要標題：CIDR"));
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

/// 逐列驗證：7 欄正規化與結構規則（任一列錯誤即整批不寫入）。
///
/// 欄位驗證重用 [`SubnetInput::validate`]；CIDR 重複／重疊與 Kea subnet-id
/// 重複為匯入特有的跨列規則，於此逐列比對既有資料與檔內較前列。
fn validate_row(
    raw: &RawRow,
    context: &mut RowContext,
) -> Result<(RowReport, Option<ValidSubnet>), ApiError> {
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

    let name = raw.text(Field::Name);
    let note = raw.text(Field::Note);
    let cidr_text = raw.text(Field::Cidr);
    let gateway_text = raw.text(Field::Gateway);
    let kea_text = raw.text(Field::KeaSubnetId);
    let pools_text = raw.text(Field::Pools);
    let exclusions_text = raw.text(Field::Exclusions);

    // CIDR：必填、正規化並收斂 host bits（重用領域原語）；無法解析即無地址族。
    let normalized = match subnets::normalize_cidr(cidr_text.clone()) {
        Ok(network) => Some(network),
        Err(error) => {
            let code = if cidr_text.is_none() {
                "cidr_required"
            } else {
                "invalid_cidr"
            };
            issues.push(Issue::error(
                code,
                Some(Field::Cidr),
                error.message().to_string(),
            ));
            None
        }
    };
    let is_v4 = normalized.map(|network| network.addr().is_ipv4());

    // Gateway：格式自行解析供 data 正規化；是否落在 CIDR 內交由領域驗證。
    let gateway = match gateway_text.as_deref().map(str::parse::<IpAddr>) {
        None => None,
        Some(Ok(address)) => Some(address),
        Some(Err(_)) => {
            issues.push(Issue::error(
                "invalid_gateway",
                Some(Field::Gateway),
                format!(
                    "gateway 格式錯誤：{}",
                    gateway_text.as_deref().unwrap_or("")
                ),
            ));
            None
        }
    };

    // Kea subnet-id：僅 IPv4、整數且 `0 < id < 4294967295`；v6 有值＝錯誤
    //（見 ADR-0009、ADR-0023）。
    let parsed_kea = kea_text
        .as_deref()
        .and_then(|text| text.parse::<i64>().ok());
    let mut kea_subnet_id = None;
    if let Some(text) = &kea_text {
        if is_v4 == Some(false) {
            issues.push(Issue::error(
                "kea_subnet_id_for_v6",
                Some(Field::KeaSubnetId),
                "IPv6 網段不支援 Kea subnet-id（請留空）",
            ));
        } else {
            match parse_kea_subnet_id(text) {
                Ok(id) => kea_subnet_id = Some(id),
                Err(message) => issues.push(Issue::error(
                    "invalid_kea_subnet_id",
                    Some(Field::KeaSubnetId),
                    message,
                )),
            }
        }
    }

    // 位址池：`|` 分隔、每段 `起點-終點`、僅 IPv4；範圍與段間重疊交由領域驗證。
    let mut pool_pairs: Vec<(Ipv4Addr, Ipv4Addr)> = Vec::new();
    let mut pool_inputs: Vec<PoolInput> = Vec::new();
    if let Some(text) = &pools_text {
        if is_v4 == Some(false) {
            issues.push(Issue::error(
                "pools_for_v6",
                Some(Field::Pools),
                "IPv6 網段不支援位址池（請留空）",
            ));
        } else {
            for segment in text.split('|') {
                let segment = segment.trim();
                match parse_pool_segment(segment) {
                    Ok((start, end)) => {
                        pool_inputs.push(PoolInput {
                            start_ip: Some(start.to_string()),
                            end_ip: Some(end.to_string()),
                        });
                        pool_pairs.push((start, end));
                    }
                    Err(message) => {
                        issues.push(Issue::error("invalid_pools", Some(Field::Pools), message))
                    }
                }
            }
        }
    }

    // 排除範圍：`|` 分隔、每段 `起-迄` 或 `起-迄#用途說明`（以第一個 `#`
    // 分割）、僅 IPv4；重疊等跨段規則交由領域驗證（見 ADR-0020）。
    let mut exclusion_pairs: Vec<(Ipv4Addr, Ipv4Addr, Option<String>)> = Vec::new();
    let mut exclusion_inputs: Vec<ExclusionInput> = Vec::new();
    if let Some(text) = &exclusions_text {
        if is_v4 == Some(false) {
            issues.push(Issue::error(
                "exclusions_for_v6",
                Some(Field::Exclusions),
                "IPv6 網段不支援排除範圍（請留空）",
            ));
        } else {
            for segment in text.split('|') {
                let segment = segment.trim();
                match parse_exclusion_segment(segment) {
                    Ok((start, end, note)) => {
                        exclusion_inputs.push(ExclusionInput {
                            start_ip: Some(start.to_string()),
                            end_ip: Some(end.to_string()),
                            note: note.clone(),
                        });
                        exclusion_pairs.push((start, end, note));
                    }
                    Err(message) => issues.push(Issue::error(
                        "invalid_exclusions",
                        Some(Field::Exclusions),
                        message,
                    )),
                }
            }
        }
    }

    // 欄位驗證一律走領域驗證（單一驗證權威）；CIDR 無效時無從續驗。
    let mut valid_subnet = None;
    if normalized.is_some() {
        let input = SubnetInput {
            cidr: cidr_text.clone(),
            name: name.clone(),
            note: note.clone(),
            gateway: gateway.map(|address| address.to_string()),
            kea_subnet_id,
            pools: pool_inputs,
            exclusions: exclusion_inputs,
        };
        match input.validate() {
            Ok(valid) => valid_subnet = Some(valid),
            Err(error) => issues.push(validate_issue(error)),
        }
    }

    // 跨列規則：CIDR 已存在（既有／檔內）或與既有／檔內網段重疊（含嵌套）。
    if let Some(network) = normalized {
        let text = network.to_string();
        for existing in &context.existing.subnets {
            if existing.network == network {
                issues.push(Issue::error(
                    "cidr_duplicate",
                    Some(Field::Cidr),
                    format!(
                        "CIDR {text} 已存在（既有網段 {}）",
                        describe(&existing.text, existing.name.as_deref())
                    ),
                ));
            } else if existing.network.contains(&network) || network.contains(&existing.network) {
                issues.push(Issue::error(
                    "cidr_overlap",
                    Some(Field::Cidr),
                    format!(
                        "與既有網段 {} 重疊（網段不得重疊，含嵌套）",
                        describe(&existing.text, existing.name.as_deref())
                    ),
                ));
            }
        }

        if let Some(previous) = context.file_cidrs.get(&text).copied() {
            issues.push(Issue::error(
                "cidr_duplicate",
                Some(Field::Cidr),
                format!("CIDR {text} 在檔案中重複（首次出現於列 {previous}）"),
            ));
        } else {
            for registered in &context.file_nets {
                if registered.network.contains(&network) || network.contains(&registered.network) {
                    issues.push(Issue::error(
                        "cidr_overlap",
                        Some(Field::Cidr),
                        format!(
                            "與第 {} 列的網段（{}）重疊（含嵌套）",
                            registered.row_number, registered.text
                        ),
                    ));
                }
            }
            context.file_cidrs.insert(text.clone(), raw.row_number);
            context.file_nets.push(RegisteredNet {
                network,
                row_number: raw.row_number,
                text,
            });
        }

        // Kea subnet-id 全系統唯一（僅 v4；v6 已於上方回報）。
        if let Some(id) = kea_subnet_id {
            if let Some((cidr, name)) = context.existing.kea_ids.get(&id) {
                issues.push(Issue::error(
                    "kea_subnet_id_duplicate",
                    Some(Field::KeaSubnetId),
                    format!(
                        "Kea subnet-id {id} 已被既有網段 {} 使用",
                        describe(cidr, name.as_deref())
                    ),
                ));
            }
            if let Some(previous) = context.file_kea_ids.get(&id).copied() {
                issues.push(Issue::error(
                    "kea_subnet_id_duplicate",
                    Some(Field::KeaSubnetId),
                    format!("Kea subnet-id {id} 在檔案中重複（首次出現於列 {previous}）"),
                ));
            } else {
                context.file_kea_ids.insert(id, raw.row_number);
            }
        }
    }

    let status = if issues.iter().any(|issue| issue.severity == "error") {
        "error"
    } else {
        "ok"
    };

    let plan = if status == "error" {
        None
    } else {
        Some(valid_subnet.ok_or_else(|| ApiError::internal("匯入列驗證失敗", "缺少已驗證網段"))?)
    };

    // v6 列的 pool／排除範圍一律以原始段落呈現（規則不允許，供問題列辨識）。
    let data_pools = if is_v4 == Some(false) {
        raw_segments(pools_text)
    } else {
        pool_pairs
            .iter()
            .map(|(start, end)| format!("{start}-{end}"))
            .collect()
    };
    let data_exclusions = if is_v4 == Some(false) {
        raw_segments(exclusions_text)
    } else {
        exclusion_pairs
            .iter()
            .map(|(start, end, note)| match note {
                Some(note) => format!("{start}-{end}#{note}"),
                None => format!("{start}-{end}"),
            })
            .collect()
    };

    let data = RowData {
        name,
        cidr: normalized.map(|network| network.to_string()),
        gateway: gateway.map(|address| address.to_string()),
        kea_subnet_id: parsed_kea,
        pools: data_pools,
        exclusions: data_exclusions,
        note,
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

/// 解析 CSV 的 Kea subnet-id：整數且 `0 < id < 4294967295`（Kea 限制；見 ADR-0023）。
fn parse_kea_subnet_id(text: &str) -> Result<i64, String> {
    match text.parse::<i64>() {
        Ok(id) if (1..subnets::KEA_SUBNET_ID_LIMIT).contains(&id) => Ok(id),
        Ok(id) if id >= 1 => Err(format!(
            "Kea subnet-id 超出範圍（須小於 {}）：{id}",
            subnets::KEA_SUBNET_ID_LIMIT
        )),
        _ => Err(format!("Kea subnet-id 須為正整數：{text}")),
    }
}

/// 單段位址池解析：須為 `起點-終點`（兩端皆 IPv4，不含空白段）。
fn parse_pool_segment(segment: &str) -> Result<(Ipv4Addr, Ipv4Addr), String> {
    if segment.is_empty() {
        return Err("位址池含空段（請以 | 分隔多段，每段 起點-終點）".to_string());
    }

    let Some((start, end)) = segment.split_once('-') else {
        return Err(pool_format_message(segment));
    };
    if end.contains('-') {
        return Err(pool_format_message(segment));
    }

    let start = start.trim().parse::<Ipv4Addr>();
    let end = end.trim().parse::<Ipv4Addr>();
    match (start, end) {
        (Ok(start), Ok(end)) => Ok((start, end)),
        _ => Err(pool_format_message(segment)),
    }
}

/// 位址池段落格式錯誤的訊息（沿用 ADR-0009 範例）。
fn pool_format_message(segment: &str) -> String {
    format!("位址池格式錯誤：{segment}（每段須為 起點-終點，例：10.0.0.100-10.0.0.200）")
}

/// 單段排除範圍解析：`起-迄` 或 `起-迄#用途說明`（以**第一個** `#` 分割；
/// note trim 後空字串視為無；範圍解析重用 [`parse_pool_segment`]）。
fn parse_exclusion_segment(segment: &str) -> Result<(Ipv4Addr, Ipv4Addr, Option<String>), String> {
    if segment.is_empty() {
        return Err("排除範圍含空段（請以 | 分隔多段，每段 起-迄[#用途說明]）".to_string());
    }

    let (range, note) = match segment.split_once('#') {
        Some((range, note)) => (
            range.trim(),
            Some(note.trim()).filter(|text| !text.is_empty()),
        ),
        None => (segment, None),
    };
    let note = note.map(str::to_string);

    match parse_pool_segment(range) {
        Ok((start, end)) => Ok((start, end, note)),
        Err(_) => Err(exclusion_format_message(segment)),
    }
}

/// 排除範圍段落格式錯誤的訊息（格式見 spec §9）。
fn exclusion_format_message(segment: &str) -> String {
    format!(
        "排除範圍格式錯誤：{segment}\
         （每段須為 起-迄 或 起-迄#用途說明，例：10.0.0.100-10.0.0.200 或 \
         10.0.0.100-10.0.0.200#NAT）"
    )
}

/// 將 `SubnetInput::validate` 的錯誤轉為逐列問題（欄位對應代碼）。
fn validate_issue(error: ApiError) -> Issue {
    let code = match error.field_name() {
        Some("cidr") => "invalid_cidr",
        Some("gateway") => "invalid_gateway",
        Some("kea_subnet_id") => "invalid_kea_subnet_id",
        Some(field) if field == "pools" || field.starts_with("pools[") => "invalid_pools",
        Some(field) if field == "exclusions" || field.starts_with("exclusions[") => {
            "invalid_exclusions"
        }
        _ => "invalid_subnet",
    };
    let field = match error.field_name() {
        Some("cidr") => Some("cidr"),
        Some("gateway") => Some("gateway"),
        Some("kea_subnet_id") => Some("kea_subnet_id"),
        Some(field) if field == "pools" || field.starts_with("pools[") => Some("pools"),
        Some(field) if field == "exclusions" || field.starts_with("exclusions[") => {
            Some("exclusions")
        }
        _ => None,
    };
    Issue {
        severity: "error",
        code,
        field: field.map(str::to_string),
        message: error.message().to_string(),
    }
}

/// 衝突訊息中的網段描述：有名稱時附上名稱。
fn describe(cidr: &str, name: Option<&str>) -> String {
    match name {
        Some(name) if !name.trim().is_empty() => format!("「{name}」（{cidr}）"),
        _ => cidr.to_string(),
    }
}

/// 欄位數不一致的列：只填可取得的原始值（僅供問題列報告辨識）。
fn mismatch_data(raw: &RawRow) -> RowData {
    RowData {
        name: raw.text(Field::Name),
        cidr: raw.text(Field::Cidr),
        gateway: raw.text(Field::Gateway),
        kea_subnet_id: raw
            .text(Field::KeaSubnetId)
            .and_then(|value| value.parse::<i64>().ok()),
        pools: raw_segments(raw.text(Field::Pools)),
        exclusions: raw_segments(raw.text(Field::Exclusions)),
        note: raw.text(Field::Note),
    }
}

/// 原始多值欄位文字以 `|` 切段（trim、略過空段；不做格式驗證）。
fn raw_segments(text: Option<String>) -> Vec<String> {
    text.map(|text| {
        text.split('|')
            .map(str::trim)
            .filter(|segment| !segment.is_empty())
            .map(str::to_string)
            .collect()
    })
    .unwrap_or_default()
}

/// 讀取既有網段（CIDR 與 Kea subnet-id 比對用）。
async fn load_existing(pool: &SqlitePool) -> Result<ExistingSubnets, ApiError> {
    let rows: Vec<(String, Option<String>, Option<i64>)> =
        sqlx::query_as("SELECT cidr, name, kea_subnet_id FROM subnets ORDER BY id ASC")
            .fetch_all(pool)
            .await
            .map_err(|error| ApiError::internal("讀取既有網段失敗", error))?;

    let mut subnets = Vec::with_capacity(rows.len());
    let mut kea_ids = HashMap::new();
    for (cidr, name, kea_subnet_id) in rows {
        let network: IpNet = cidr
            .parse()
            .map_err(|error| ApiError::internal("既有網段 CIDR 格式錯誤", error))?;
        if let Some(id) = kea_subnet_id {
            kea_ids.insert(id, (cidr.clone(), name.clone()));
        }
        subnets.push(ExistingSubnet {
            network,
            text: cidr,
            name,
        });
    }

    Ok(ExistingSubnets { subnets, kea_ids })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_file_accepts_utf8_with_and_without_bom() {
        let text = "CIDR,名稱\n10.0.0.0/24,核心\n";
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
        let source = "CIDR,名稱\n10.0.0.0/24,核心網段\n";
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
    fn parse_csv_maps_headers_case_insensitively_in_any_order() {
        let parsed = parse_csv(
            " KEA SUBNET-ID ,名稱,排除範圍,Gateway,備註,CIDR,位址池\n1,核心,10.0.0.30-10.0.0.40#NAT,10.0.0.1,主力,10.0.0.0/24,10.0.0.100-10.0.0.200\n",
        )
        .expect("解析成功");
        assert!(parsed.ignored_headers.is_empty());
        let row = &parsed.rows[0];
        assert_eq!(row.row_number, 2);
        assert!(!row.column_mismatch);
        assert_eq!(row.text(Field::Name).as_deref(), Some("核心"));
        assert_eq!(row.text(Field::Cidr).as_deref(), Some("10.0.0.0/24"));
        assert_eq!(row.text(Field::Gateway).as_deref(), Some("10.0.0.1"));
        assert_eq!(row.text(Field::KeaSubnetId).as_deref(), Some("1"));
        assert_eq!(
            row.text(Field::Pools).as_deref(),
            Some("10.0.0.100-10.0.0.200")
        );
        assert_eq!(
            row.text(Field::Exclusions).as_deref(),
            Some("10.0.0.30-10.0.0.40#NAT")
        );
        assert_eq!(row.text(Field::Note).as_deref(), Some("主力"));
    }

    #[test]
    fn parse_csv_reports_ignored_headers() {
        // 注意：解析層不做欄位驗證，未知標題只列入 ignored_headers。
        let parsed = parse_csv("CIDR,數量,金額\n10.0.0.0/24,3,100\n").expect("解析成功");
        assert_eq!(parsed.ignored_headers, vec!["數量", "金額"]);
    }

    #[test]
    fn parse_csv_rejects_missing_and_duplicate_headers() {
        let error = parse_csv("名稱,Gateway\n核心,10.0.0.1\n").expect_err("缺 CIDR 應拒絕");
        assert!(error.message().contains("缺少必要標題"));
        assert!(error.message().contains("CIDR"));

        let error = parse_csv("CIDR,CIDR,名稱\n10.0.0.0/24,192.168.0.0/24,核心\n")
            .expect_err("重複標題應拒絕");
        assert!(error.message().contains("標題重複"));
        assert!(error.message().contains("CIDR"));

        let error = parse_csv("").expect_err("空檔案應拒絕");
        assert!(error.message().contains("空"));

        let error = parse_csv("CIDR,名稱\n").expect_err("無資料列應拒絕");
        assert!(error.message().contains("沒有資料列"));
    }

    #[test]
    fn parse_csv_skips_blank_lines_and_numbers_rows_sequentially() {
        let parsed = parse_csv("CIDR,名稱\n\n10.0.0.0/24,網段一\n   \n10.0.1.0/24,網段二\n")
            .expect("解析成功");
        let numbers: Vec<usize> = parsed.rows.iter().map(|row| row.row_number).collect();
        assert_eq!(numbers, vec![2, 3], "空行與全空白列忽略");
        assert_eq!(parsed.rows[1].text(Field::Name).as_deref(), Some("網段二"));
    }

    #[test]
    fn parse_csv_flags_column_count_mismatch() {
        let parsed = parse_csv("CIDR,名稱\n只有一欄\n10.0.0.0/24,核心\n10.0.1.0/24,核心,多餘\n")
            .expect("解析成功");
        assert!(parsed.rows[0].column_mismatch);
        assert_eq!(parsed.rows[0].column_count, 1);
        assert!(!parsed.rows[1].column_mismatch);
        assert_eq!(parsed.rows[1].column_count, 2);
        assert!(parsed.rows[2].column_mismatch);
        assert_eq!(parsed.rows[2].column_count, 3);
    }

    #[test]
    fn parse_csv_handles_rfc4180_quotes_and_crlf() {
        let parsed = parse_csv("CIDR,名稱,備註\r\n10.0.0.0/24,\"核心, 第一區\r\n第二行\",備註\r\n")
            .expect("解析成功");
        assert_eq!(parsed.rows.len(), 1);
        let name = parsed.rows[0].text(Field::Name).expect("名稱");
        assert!(name.contains("核心, 第一區"));
        assert!(name.contains("第二行"), "引號內換行屬同一列");
    }

    #[test]
    fn parse_kea_subnet_id_enforces_kea_range() {
        assert_eq!(parse_kea_subnet_id("1"), Ok(1), "下界 1 允許");
        assert_eq!(
            parse_kea_subnet_id("4294967294"),
            Ok(4294967294),
            "上界 4294967294 允許（不含 4294967295）"
        );

        for text in ["4294967295", "4294967296", "9223372036854775807"] {
            let error = parse_kea_subnet_id(text).expect_err("超出上界應拒絕");
            assert!(error.contains("超出範圍"), "訊息說明超出範圍：{error}");
        }
        for text in ["0", "-1", "", "abc"] {
            let error = parse_kea_subnet_id(text).expect_err("非正整數應拒絕");
            assert!(error.contains("正整數"), "訊息：{error}");
        }
    }

    #[test]
    fn parse_pool_segment_accepts_ranges_and_rejects_malformed() {
        assert_eq!(
            parse_pool_segment("10.0.0.100-10.0.0.200").expect("合法"),
            (Ipv4Addr::new(10, 0, 0, 100), Ipv4Addr::new(10, 0, 0, 200))
        );
        for segment in [
            "",
            "10.0.0.100",
            "10.0.0.100-",
            "-10.0.0.200",
            "a-b",
            "fd00::1-fd00::2",
        ] {
            assert!(parse_pool_segment(segment).is_err(), "{segment} 應拒絕");
        }
    }

    #[test]
    fn parse_exclusion_segment_splits_note_at_first_hash() {
        // 無用途說明。
        assert_eq!(
            parse_exclusion_segment("10.0.0.10-10.0.0.20").expect("合法"),
            (
                Ipv4Addr::new(10, 0, 0, 10),
                Ipv4Addr::new(10, 0, 0, 20),
                None
            )
        );

        // 有用途說明：trim；`#` 可再出現於說明，以第一個 `#` 分隔。
        assert_eq!(
            parse_exclusion_segment(" 10.0.0.10 - 10.0.0.20 # NAT #1 ").expect("合法"),
            (
                Ipv4Addr::new(10, 0, 0, 10),
                Ipv4Addr::new(10, 0, 0, 20),
                Some("NAT #1".to_string())
            )
        );

        // 說明為空白視為無。
        assert_eq!(
            parse_exclusion_segment("10.0.0.10-10.0.0.20#  ").expect("合法"),
            (
                Ipv4Addr::new(10, 0, 0, 10),
                Ipv4Addr::new(10, 0, 0, 20),
                None
            )
        );

        for segment in ["", "10.0.0.10", "a-b", "#note", "fd00::1-fd00::2"] {
            assert!(
                parse_exclusion_segment(segment).is_err(),
                "{segment} 應拒絕"
            );
        }
    }
}
