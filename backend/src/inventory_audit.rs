//! 清冊稽核（Inventory audit）：以記憶體直讀財管清冊 `.xls／.xlsx`、
//! 以財產編號比對系統資產；純唯讀、不落地。
//!
//! 規則權威為 `docs/adr/0024` 與 `.scratch/inventory-audit/spec.md` §2–§5。
//! 解析與比對集中在本模組；HTTP 形狀見 [`crate::api::inventory_audit`]。

use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::Path;

use calamine::{Data, Range, Reader, open_workbook_auto_from_rs};
use serde::Serialize;
use sqlx::{FromRow, SqlitePool};

use crate::api::ApiError;

/// 檔案大小上限：5 MB（見 spec §4）。
pub(crate) const MAX_FILE_BYTES: usize = 5 * 1024 * 1024;
/// 單檔資料列上限：5,000 列（見 spec §4）。
pub(crate) const MAX_DATA_ROWS: usize = 5_000;
/// 單次檔案數上限：10 檔（見 spec §4）。
pub(crate) const MAX_FILES: usize = 10;

/// 表頭列掃描範圍：工作表前 10 列（見 spec §2）。
const HEADER_SCAN_ROWS: u32 = 10;

/// 表頭文字（去除所有空白後相等；見 spec §2）。
const HEADER_PROPERTY_NO: &str = "財產編號";
const HEADER_NAME: &str = "財產名稱";
const HEADER_FORM: &str = "形式品名";
const HEADER_LOCATION: &str = "存放地點";
const HEADER_CUSTODIAN: &str = "保管人員";
const HEADER_NOTE: &str = "備註";

/// 清冊種類：財產／物品（僅為來源標示；見 spec §2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Property,
    Item,
}

/// 上傳後的單檔內容；`oversized`＝讀取時已確定超過 5 MB（內容不再保留）。
pub(crate) struct UploadedFile {
    pub filename: String,
    pub bytes: Vec<u8>,
    pub oversized: bool,
}

/// 檔級錯誤（整批拒絕時的回應 `details.files` 元素；見 spec §4）。
#[derive(Debug, Serialize)]
pub(crate) struct FileError {
    pub filename: String,
    pub message: String,
}

/// 逐檔摘要（見 spec §5.2）。
#[derive(Debug, Serialize)]
struct FileSummary {
    filename: String,
    kind: Kind,
    total: usize,
    skipped: usize,
}

/// 整體摘要（見 spec §5.2）。
#[derive(Debug, Serialize)]
struct Summary {
    list_total: usize,
    list_property: usize,
    list_item: usize,
    system_total: usize,
    matched: usize,
    missing_in_system: usize,
    missing_in_list: usize,
}

/// 「清冊有、系統沒有」的列；原文保留、不折疊（見 spec §3）。
#[derive(Debug, Serialize)]
struct MissingInSystem {
    file: String,
    kind: Kind,
    row_number: u32,
    property_no: String,
    name: Option<String>,
    form: Option<String>,
    location: Option<String>,
    custodian: Option<String>,
    note: Option<String>,
    duplicate: bool,
}

/// 「系統有、清冊沒有」的資產；原文保留。
#[derive(Debug, Serialize)]
struct MissingInList {
    asset_id: i64,
    property_no: String,
    description: String,
    location: String,
    note: Option<String>,
    duplicate: bool,
}

/// 稽核回應（見 spec §5.2）。
#[derive(Debug, Serialize)]
pub(crate) struct AuditReport {
    files: Vec<FileSummary>,
    summary: Summary,
    missing_in_system: Vec<MissingInSystem>,
    missing_in_list: Vec<MissingInList>,
}

/// 解析後的單列（顯示欄位為空＝`None`；原文保留）。
#[derive(Debug)]
struct ParsedRow {
    row_number: u32,
    property_no: String,
    name: Option<String>,
    form: Option<String>,
    location: Option<String>,
    custodian: Option<String>,
    note: Option<String>,
}

/// 解析後的單檔：種類、資料列與略過列數。
#[derive(Debug)]
struct ParsedFile {
    kind: Kind,
    skipped: usize,
    rows: Vec<ParsedRow>,
}

/// 系統側資產（財產編號非空白）。
#[derive(Debug, FromRow)]
struct SystemAsset {
    id: i64,
    property_no: String,
    description: String,
    location: String,
    note: Option<String>,
}

/// 表頭解析後的欄位位置（相對工作表範圍起點；重複表頭取第一個）。
#[derive(Debug, Default)]
struct Columns {
    property_no: Option<usize>,
    name: Option<usize>,
    form: Option<usize>,
    location: Option<usize>,
    custodian: Option<usize>,
    note: Option<usize>,
}

/// 文字化後的工作表網格；`start_row` 為範圍起點的絕對列（0-based），
/// 供輸出 1-based 實際列號（見 spec §2）。
struct SheetGrid {
    start_row: u32,
    rows: Vec<Vec<String>>,
}

impl SheetGrid {
    /// 由 calamine 範圍建立；儲存格一律文字化（見 spec §2）。
    fn from_range(range: &Range<Data>) -> Self {
        let start_row = range.start().map(|(row, _)| row).unwrap_or(0);
        let rows = range
            .rows()
            .map(|row| row.iter().map(cell_text).collect())
            .collect();
        Self { start_row, rows }
    }

    /// 相對列中指定欄的文字；缺欄或超出範圍回傳空字串。
    fn text(&self, rel_row: usize, column: Option<usize>) -> &str {
        column
            .and_then(|col| self.rows.get(rel_row).and_then(|row| row.get(col)))
            .map(String::as_str)
            .unwrap_or("")
    }

    /// 該相對列的實際列號（1-based；見 spec §2）。
    fn row_number(&self, rel_row: usize) -> u32 {
        self.start_row + rel_row as u32 + 1
    }

    /// 表頭列＝前 10 列內第一個含「財產編號」（去空白後相等）儲存格者。
    fn find_header(&self) -> Option<usize> {
        self.rows.iter().enumerate().find_map(|(rel_row, row)| {
            let within_scan = self.start_row + (rel_row as u32) < HEADER_SCAN_ROWS;
            (within_scan
                && row
                    .iter()
                    .any(|cell| strip_whitespace(cell) == HEADER_PROPERTY_NO))
            .then_some(rel_row)
        })
    }

    /// 依表頭文字定位欄位；未知欄忽略、重複表頭取第一個（見 spec §2）。
    fn columns(&self, header: usize) -> Columns {
        let mut columns = Columns::default();
        let Some(row) = self.rows.get(header) else {
            return columns;
        };
        for (index, text) in row.iter().enumerate() {
            let slot = match strip_whitespace(text).as_str() {
                HEADER_PROPERTY_NO => &mut columns.property_no,
                HEADER_NAME => &mut columns.name,
                HEADER_FORM => &mut columns.form,
                HEADER_LOCATION => &mut columns.location,
                HEADER_CUSTODIAN => &mut columns.custodian,
                HEADER_NOTE => &mut columns.note,
                _ => continue,
            };
            if slot.is_none() {
                *slot = Some(index);
            }
        }
        columns
    }

    /// 種類：表頭上方文字（同工作表）含「物品」→ 物品，否則財產（見 spec §2）。
    fn kind(&self, header: usize) -> Kind {
        let has_item_text = self.rows[..header]
            .iter()
            .flatten()
            .any(|cell| cell.contains("物品"));
        if has_item_text {
            Kind::Item
        } else {
            Kind::Property
        }
    }

    /// 表頭以下的資料列與略過列（見 spec §2）。
    ///
    /// 資料列＝編號欄含 ASCII 數字；略過列＝編號欄不含數字但財產名稱欄有值；
    /// 其餘（全空列、頁尾文字）忽略。
    fn extract_rows(&self, header: usize, columns: &Columns) -> (Vec<ParsedRow>, usize) {
        let mut rows = Vec::new();
        let mut skipped = 0;
        for rel_row in header + 1..self.rows.len() {
            let property_no = self.text(rel_row, columns.property_no).to_string();
            if property_no.chars().any(|ch| ch.is_ascii_digit()) {
                rows.push(ParsedRow {
                    row_number: self.row_number(rel_row),
                    property_no,
                    name: optional_cell(self.text(rel_row, columns.name)),
                    form: optional_cell(self.text(rel_row, columns.form)),
                    location: optional_cell(self.text(rel_row, columns.location)),
                    custodian: optional_cell(self.text(rel_row, columns.custodian)),
                    note: optional_cell(self.text(rel_row, columns.note)),
                });
            } else if !self.text(rel_row, columns.name).trim().is_empty() {
                skipped += 1;
            }
        }
        (rows, skipped)
    }
}

/// 解析單一上傳檔案（記憶體位元組、不落地）；檔級錯誤回傳中文訊息。
fn parse_file(upload: &UploadedFile) -> Result<ParsedFile, String> {
    if upload.oversized || upload.bytes.len() > MAX_FILE_BYTES {
        return Err("檔案超過 5 MB 上限".to_string());
    }
    if !is_excel_extension(&upload.filename) {
        return Err("僅接受 .xls／.xlsx 格式的檔案".to_string());
    }

    let mut workbook = open_workbook_auto_from_rs(Cursor::new(upload.bytes.as_slice()))
        .map_err(|_| "無法解析 Excel 檔案（檔案可能損毀或格式不符）".to_string())?;

    let mut rows = Vec::new();
    let mut skipped = 0;
    let mut kind: Option<Kind> = None;
    for name in workbook.sheet_names() {
        let range = workbook
            .worksheet_range(&name)
            .map_err(|_| "無法解析 Excel 檔案（讀取工作表失敗）".to_string())?;
        let grid = SheetGrid::from_range(&range);
        let Some(header) = grid.find_header() else {
            continue;
        };
        // 多張資料表時以第一張的種類為該檔種類（見 spec §2）。
        if kind.is_none() {
            kind = Some(grid.kind(header));
        }
        let columns = grid.columns(header);
        let (sheet_rows, sheet_skipped) = grid.extract_rows(header, &columns);
        rows.extend(sheet_rows);
        skipped += sheet_skipped;
        if rows.len() > MAX_DATA_ROWS {
            return Err("資料列超過 5,000 列上限".to_string());
        }
    }

    let Some(kind) = kind else {
        return Err("找不到「財產編號」表頭".to_string());
    };
    if rows.is_empty() {
        return Err("沒有資料列".to_string());
    }

    Ok(ParsedFile {
        kind,
        skipped,
        rows,
    })
}

/// 稽核：解析全部檔案、讀取系統側資產、比對；任一檔級錯誤整批回 400（見 spec §4）。
pub(crate) async fn audit(
    pool: &SqlitePool,
    uploads: Vec<UploadedFile>,
) -> Result<AuditReport, ApiError> {
    let mut parsed_files = Vec::with_capacity(uploads.len());
    let mut errors = Vec::new();
    for upload in &uploads {
        match parse_file(upload) {
            Ok(file) => parsed_files.push((upload.filename.clone(), file)),
            Err(message) => errors.push(FileError {
                filename: upload.filename.clone(),
                message,
            }),
        }
    }
    if !errors.is_empty() {
        return Err(file_errors_error(&errors));
    }

    let system = load_system_assets(pool).await?;
    Ok(compare(&parsed_files, &system))
}

/// 檔級錯誤：整批拒絕（400）＋`details.files` 逐檔回報（見 spec §4）。
pub(crate) fn file_errors_error(errors: &[FileError]) -> ApiError {
    let message = errors
        .iter()
        .map(|error| {
            format!(
                "檔案「{}」{}",
                display_filename(&error.filename),
                error.message
            )
        })
        .collect::<Vec<_>>()
        .join("；");
    ApiError::validation(message)
        .field("file")
        .detail("files", errors_json(errors))
}

/// 檔案數超過上限：整批拒絕；逐檔列出已收到的檔案（見 spec §4）。
pub(crate) fn too_many_files_error(filenames: Vec<String>) -> ApiError {
    let errors: Vec<FileError> = filenames
        .into_iter()
        .map(|filename| FileError {
            filename,
            message: "超過單次 10 個檔案上限".to_string(),
        })
        .collect();
    ApiError::validation("一次最多上傳 10 個檔案，整批不稽核")
        .field("file")
        .detail("files", errors_json(&errors))
}

fn errors_json(errors: &[FileError]) -> serde_json::Value {
    serde_json::to_value(errors).unwrap_or_else(|_| serde_json::json!([]))
}

fn display_filename(filename: &str) -> &str {
    if filename.is_empty() {
        "（未命名）"
    } else {
        filename
    }
}

/// 副檔名檢查：僅接受 `.xls`／`.xlsx`（不分大小寫；見 spec §2）。
fn is_excel_extension(filename: &str) -> bool {
    Path::new(filename)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("xls") || extension.eq_ignore_ascii_case("xlsx")
        })
}

/// 讀取系統側＝財產編號非空白（trim 後）的全部資產；單一查詢（見 spec §3）。
async fn load_system_assets(pool: &SqlitePool) -> Result<Vec<SystemAsset>, ApiError> {
    sqlx::query_as::<_, SystemAsset>(
        "SELECT id, property_no, description, location, note FROM assets
         WHERE property_no IS NOT NULL AND trim(property_no) <> ''
         ORDER BY id",
    )
    .fetch_all(pool)
    .await
    .map_err(|error| ApiError::internal("讀取系統資產失敗", error))
}

/// 比對：相符以鍵計；兩向差異逐列輸出、依鍵升冪穩定排序、同鍵標示重複（見 spec §3）。
fn compare(parsed_files: &[(String, ParsedFile)], system: &[SystemAsset]) -> AuditReport {
    // 清冊側鍵計數（同檔、跨檔合併為一集合；同鍵多列＝重複）。
    let mut list_key_counts: HashMap<String, usize> = HashMap::new();
    for (_, file) in parsed_files {
        for row in &file.rows {
            *list_key_counts
                .entry(normalize_key(&row.property_no))
                .or_default() += 1;
        }
    }

    // 系統側鍵計數。
    let mut system_key_counts: HashMap<String, usize> = HashMap::new();
    for asset in system {
        *system_key_counts
            .entry(normalize_key(&asset.property_no))
            .or_default() += 1;
    }
    let system_keys: HashSet<&str> = system_key_counts.keys().map(String::as_str).collect();

    // 清冊有、系統沒有。
    let mut missing_in_system: Vec<(String, MissingInSystem)> = Vec::new();
    for (filename, file) in parsed_files {
        for row in &file.rows {
            let key = normalize_key(&row.property_no);
            if system_keys.contains(key.as_str()) {
                continue;
            }
            missing_in_system.push((
                key.clone(),
                MissingInSystem {
                    file: filename.clone(),
                    kind: file.kind,
                    row_number: row.row_number,
                    property_no: row.property_no.clone(),
                    name: row.name.clone(),
                    form: row.form.clone(),
                    location: row.location.clone(),
                    custodian: row.custodian.clone(),
                    note: row.note.clone(),
                    duplicate: list_key_counts.get(&key).is_some_and(|count| *count > 1),
                },
            ));
        }
    }
    missing_in_system.sort_by(|left, right| left.0.cmp(&right.0));

    // 系統有、清冊沒有。
    let mut missing_in_list: Vec<(String, MissingInList)> = Vec::new();
    for asset in system {
        let key = normalize_key(&asset.property_no);
        if list_key_counts.contains_key(&key) {
            continue;
        }
        missing_in_list.push((
            key.clone(),
            MissingInList {
                asset_id: asset.id,
                property_no: asset.property_no.clone(),
                description: asset.description.clone(),
                location: asset.location.clone(),
                note: asset.note.clone(),
                duplicate: system_key_counts.get(&key).is_some_and(|count| *count > 1),
            },
        ));
    }
    missing_in_list.sort_by(|left, right| left.0.cmp(&right.0));

    let matched = list_key_counts
        .keys()
        .filter(|key| system_key_counts.contains_key(*key))
        .count();
    let list_total: usize = parsed_files.iter().map(|(_, file)| file.rows.len()).sum();
    let list_property: usize = parsed_files
        .iter()
        .filter(|(_, file)| file.kind == Kind::Property)
        .map(|(_, file)| file.rows.len())
        .sum();
    let list_item: usize = parsed_files
        .iter()
        .filter(|(_, file)| file.kind == Kind::Item)
        .map(|(_, file)| file.rows.len())
        .sum();

    AuditReport {
        files: parsed_files
            .iter()
            .map(|(filename, file)| FileSummary {
                filename: filename.clone(),
                kind: file.kind,
                total: file.rows.len(),
                skipped: file.skipped,
            })
            .collect(),
        summary: Summary {
            list_total,
            list_property,
            list_item,
            system_total: system.len(),
            matched,
            missing_in_system: missing_in_system.len(),
            missing_in_list: missing_in_list.len(),
        },
        missing_in_system: missing_in_system.into_iter().map(|(_, row)| row).collect(),
        missing_in_list: missing_in_list.into_iter().map(|(_, row)| row).collect(),
    }
}

/// 比對鍵正規化（見 spec §3）：trim、內部空白（換行、連續空白、全形空白
/// U+3000 等）壓縮為單一半形空格、ASCII 英文字母轉小寫；其餘照字面，
/// 不做「(併主機)」等慣例特例。
fn normalize_key(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut pending_space = false;
    for ch in text.trim().chars() {
        if ch.is_whitespace() {
            pending_space = true;
            continue;
        }
        if pending_space && !normalized.is_empty() {
            normalized.push(' ');
        }
        pending_space = false;
        normalized.push(if ch.is_ascii_alphabetic() {
            ch.to_ascii_lowercase()
        } else {
            ch
        });
    }
    normalized
}

/// 儲存格一律視為文字（見 spec §2）：
/// `String`／`Int`／`Float`（整數去除尾端 `.0`）／`Bool`；`Empty`＝空字串。
fn cell_text(cell: &Data) -> String {
    match cell {
        Data::String(text) => text.clone(),
        Data::Int(value) => value.to_string(),
        Data::Float(value) => float_text(*value),
        Data::Bool(value) => value.to_string(),
        Data::DateTimeIso(text) | Data::DurationIso(text) => text.clone(),
        Data::DateTime(value) => float_text(value.as_f64()),
        Data::Error(_) | Data::Empty => String::new(),
    }
}

fn float_text(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

/// 顯示欄位：空白視為未填（`None`）；非空保留原文、不折疊。
fn optional_cell(text: &str) -> Option<String> {
    if text.trim().is_empty() {
        None
    } else {
        Some(text.to_string())
    }
}

/// 去除所有空白（含全形空白）；表頭比對用（見 spec §2）。
fn strip_whitespace(text: &str) -> String {
    text.chars().filter(|ch| !ch.is_whitespace()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(rows: &[&[&str]]) -> SheetGrid {
        SheetGrid {
            start_row: 0,
            rows: rows
                .iter()
                .map(|row| row.iter().map(|cell| cell.to_string()).collect())
                .collect(),
        }
    }

    fn parsed_row(row_number: u32, property_no: &str) -> ParsedRow {
        ParsedRow {
            row_number,
            property_no: property_no.to_string(),
            name: None,
            form: None,
            location: None,
            custodian: None,
            note: None,
        }
    }

    fn system_asset(id: i64, property_no: &str) -> SystemAsset {
        SystemAsset {
            id,
            property_no: property_no.to_string(),
            description: format!("描述{id}"),
            location: format!("位置{id}"),
            note: None,
        }
    }

    #[test]
    fn normalize_key_trims_collapses_whitespace_and_lowercases_ascii() {
        assert_eq!(
            normalize_key("  3140101-03\n\n3000380  "),
            "3140101-03 3000380"
        );
        assert_eq!(normalize_key("A\t\tB"), "a b");
        assert_eq!(normalize_key("\u{3000}Ｘ\u{3000} Y\u{3000}"), "Ｘ y");
        assert_eq!(normalize_key("X3009999"), "x3009999");
        assert_eq!(normalize_key("無帳"), "無帳");
        assert_eq!(normalize_key("(併主機)"), "(併主機)");
        assert_eq!(normalize_key(""), "");
    }

    #[test]
    fn extracts_data_rows_and_counts_skipped_rows() {
        let grid = grid(&[
            &["財產清冊"],
            &["財產編號", "財產名稱", "備註"],
            &["3001", "電腦", "A"],
            &["無帳", "待報廢", ""],
            &["保管人：王", ""],
            &["說明：", ""],
        ]);
        let header = grid.find_header().expect("表頭");
        assert_eq!(header, 1);
        let columns = grid.columns(header);
        assert_eq!(columns.property_no, Some(0));
        assert_eq!(columns.name, Some(1));
        assert_eq!(columns.note, Some(2));

        let (rows, skipped) = grid.extract_rows(header, &columns);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].row_number, 3);
        assert_eq!(rows[0].property_no, "3001");
        assert_eq!(rows[0].name.as_deref(), Some("電腦"));
        assert_eq!(skipped, 1, "「無帳」列有名稱、無數字＝略過");
    }

    #[test]
    fn header_resolution_removes_all_whitespace_and_takes_first_duplicate() {
        let grid = grid(&[
            &["形式\n品名", "財產編號", "財產  編號", "形式品名"],
            &["x", "3001", "y", "z"],
        ]);
        let header = grid.find_header().expect("表頭");
        assert_eq!(header, 0);
        let columns = grid.columns(header);
        assert_eq!(columns.property_no, Some(1), "重複表頭取第一個");
        assert_eq!(columns.form, Some(0), "換行表頭去空白後仍定位");
        assert_eq!(columns.location, None);
    }

    #[test]
    fn header_multiline_property_no_is_preserved() {
        let grid = grid(&[
            &["財產編號", "財產名稱"],
            &["3140101-03\n\n3000380", "伺服器"],
        ]);
        let header = grid.find_header().expect("表頭");
        let columns = grid.columns(header);
        let (rows, _) = grid.extract_rows(header, &columns);
        assert_eq!(rows[0].property_no, "3140101-03\n\n3000380");
        assert_eq!(normalize_key(&rows[0].property_no), "3140101-03 3000380");
    }

    #[test]
    fn kind_detection_reads_text_above_header() {
        let item = grid(&[&["資訊組物品盤點清冊"], &["財產編號"], &["3001"]]);
        let header = item.find_header().expect("表頭");
        assert_eq!(item.kind(header), Kind::Item);

        let property = grid(&[&["資訊組財產盤點清冊"], &["財產編號"], &["3001"]]);
        let header = property.find_header().expect("表頭");
        assert_eq!(property.kind(header), Kind::Property);
    }

    #[test]
    fn header_scan_is_limited_to_first_ten_worksheet_rows() {
        let mut rows: Vec<Vec<String>> = (0..10).map(|_| vec!["頁首".to_string()]).collect();
        rows.push(vec!["財產編號".to_string()]);
        let grid = SheetGrid { start_row: 0, rows };
        assert_eq!(grid.find_header(), None, "第 11 列的表頭不算");
    }

    #[test]
    fn row_numbers_are_absolute_worksheet_rows() {
        let grid = SheetGrid {
            start_row: 4,
            rows: vec![
                vec!["財產編號".to_string(), "財產名稱".to_string()],
                vec!["3001".to_string(), "電腦".to_string()],
            ],
        };
        let header = grid.find_header().expect("表頭");
        let columns = grid.columns(header);
        let (rows, _) = grid.extract_rows(header, &columns);
        assert_eq!(rows[0].row_number, 6, "範圍起點為第 5 列，資料列＝第 6 列");
    }

    #[test]
    fn cell_text_stringifies_and_strips_trailing_zero() {
        assert_eq!(
            cell_text(&Data::String("3140101-03\n\n3000380".to_string())),
            "3140101-03\n\n3000380"
        );
        assert_eq!(cell_text(&Data::Int(42)), "42");
        assert_eq!(cell_text(&Data::Float(3.0)), "3");
        assert_eq!(cell_text(&Data::Float(3.5)), "3.5");
        assert_eq!(cell_text(&Data::Bool(true)), "true");
        assert_eq!(cell_text(&Data::Empty), "");
    }

    #[test]
    fn parse_file_rejects_extension_oversize_and_broken_bytes() {
        let upload = |filename: &str, bytes: Vec<u8>, oversized: bool| UploadedFile {
            filename: filename.to_string(),
            bytes,
            oversized,
        };

        let error = parse_file(&upload("report.csv", Vec::new(), false)).expect_err("非 Excel");
        assert!(error.contains(".xls"), "{error}");

        let error = parse_file(&upload("big.xlsx", vec![0; 16], true)).expect_err("超限");
        assert!(error.contains("5 MB"), "{error}");

        let error = parse_file(&upload("broken.xlsx", b"not a workbook".to_vec(), false))
            .expect_err("無法解析");
        assert!(error.contains("無法解析"), "{error}");
    }

    #[test]
    fn compare_matches_by_key_and_flags_duplicates() {
        let file = ParsedFile {
            kind: Kind::Property,
            skipped: 1,
            rows: vec![
                parsed_row(4, "PC-2"),
                parsed_row(5, "pc-1"),
                parsed_row(6, " pc-1\n"),
            ],
        };
        let system = vec![
            system_asset(1, "PC-2"),
            system_asset(2, "DUP"),
            system_asset(3, "dup"),
        ];

        let report = compare(&[("a.xlsx".to_string(), file)], &system);

        assert_eq!(report.summary.list_total, 3);
        assert_eq!(report.summary.list_property, 3);
        assert_eq!(report.summary.list_item, 0);
        assert_eq!(report.summary.system_total, 3);
        assert_eq!(report.summary.matched, 1, "PC-2 兩側同鍵");
        assert_eq!(report.summary.missing_in_system, 2);
        assert_eq!(report.summary.missing_in_list, 2);

        assert_eq!(report.files.len(), 1);
        assert_eq!(report.files[0].filename, "a.xlsx");
        assert_eq!(report.files[0].kind, Kind::Property);
        assert_eq!(report.files[0].total, 3);
        assert_eq!(report.files[0].skipped, 1);

        // 同鍵兩列皆輸出、皆標示重複；原文保留。
        assert_eq!(report.missing_in_system.len(), 2);
        assert_eq!(report.missing_in_system[0].property_no, "pc-1");
        assert_eq!(report.missing_in_system[1].property_no, " pc-1\n");
        assert!(report.missing_in_system.iter().all(|row| row.duplicate));

        // 系統側同鍵（大小寫不同）亦標示重複。
        assert_eq!(report.missing_in_list.len(), 2);
        assert!(report.missing_in_list.iter().all(|row| row.duplicate));
        assert_eq!(report.missing_in_list[0].description, "描述2");
    }

    #[test]
    fn diffs_sort_ascending_by_normalized_key_stably() {
        let file = ParsedFile {
            kind: Kind::Item,
            skipped: 0,
            rows: vec![
                parsed_row(2, "B 2"),
                parsed_row(3, "a1"),
                parsed_row(4, "b2"),
                parsed_row(5, "b2\n"),
            ],
        };

        let report = compare(&[("f.xlsx".to_string(), file)], &[]);
        let keys: Vec<&str> = report
            .missing_in_system
            .iter()
            .map(|row| row.property_no.as_str())
            .collect();
        assert_eq!(keys, vec!["a1", "B 2", "b2", "b2\n"], "依正規化鍵升冪");
        let duplicates: Vec<bool> = report
            .missing_in_system
            .iter()
            .map(|row| row.duplicate)
            .collect();
        assert_eq!(duplicates, vec![false, false, true, true]);
        assert_eq!(report.summary.missing_in_system, 4);
    }
}
