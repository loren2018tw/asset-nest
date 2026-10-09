import type {
  InventoryAuditKind,
  InventoryAuditMissingInListRow,
  InventoryAuditMissingInSystemRow
} from "@/api/inventoryAudit";
import { saveBlob } from "@/api/client";

/** 種類標籤：財產／物品（見 spec §2）。 */
export function kindLabel(kind: InventoryAuditKind): string {
  return kind === "item" ? "物品" : "財產";
}

/** 「清冊有、系統沒有」CSV 欄位＝表格欄位（見 spec §6）。 */
const MISSING_IN_SYSTEM_HEADERS = [
  "來源",
  "檔案",
  "列號",
  "財產編號",
  "財產名稱",
  "形式品名",
  "存放地點",
  "保管人員",
  "備註",
  "同鍵重複"
];

/** 「系統有、清冊沒有」CSV 欄位＝表格欄位（「操作」欄不入 CSV；見 spec §6）。 */
const MISSING_IN_LIST_HEADERS = [
  "財產編號",
  "描述",
  "位置",
  "備註",
  "同鍵重複"
];

/** RFC 4180：含逗號、引號或換行者以雙引號包住，內部引號加倍。 */
function csvCell(value: string): string {
  return /[",\r\n]/.test(value) ? `"${value.replace(/"/g, '""')}"` : value;
}

/** 可空字串：null 輸出空字串（原文不清洗、不折疊；見 spec §3）。 */
function cellText(value: string | null): string {
  return value ?? "";
}

/** 同鍵重複標示：是／空。 */
function duplicateText(duplicate: boolean): string {
  return duplicate ? "是" : "";
}

/** 由「清冊有、系統沒有」列產生 CSV（不含 BOM；見 spec §6）。 */
export function buildMissingInSystemCsv(
  rows: InventoryAuditMissingInSystemRow[]
): string {
  const lines = [MISSING_IN_SYSTEM_HEADERS.join(",")];

  for (const row of rows) {
    const cells = [
      kindLabel(row.kind),
      row.file,
      String(row.row_number),
      row.property_no,
      cellText(row.name),
      cellText(row.form),
      cellText(row.location),
      cellText(row.custodian),
      cellText(row.note),
      duplicateText(row.duplicate)
    ];
    lines.push(cells.map(csvCell).join(","));
  }

  return `${lines.join("\r\n")}\r\n`;
}

/** 由「系統有、清冊沒有」列產生 CSV（不含 BOM；見 spec §6）。 */
export function buildMissingInListCsv(
  rows: InventoryAuditMissingInListRow[]
): string {
  const lines = [MISSING_IN_LIST_HEADERS.join(",")];

  for (const row of rows) {
    const cells = [
      row.property_no,
      row.description,
      row.location,
      cellText(row.note),
      duplicateText(row.duplicate)
    ];
    lines.push(cells.map(csvCell).join(","));
  }

  return `${lines.join("\r\n")}\r\n`;
}

/** 檔名日期戳（本機日期，格式 `YYYYMMDD`）。 */
function dateStamp(date: Date): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}${month}${day}`;
}

/** 「清冊有、系統沒有」CSV 檔名（見 spec §6）。 */
export function missingInSystemFilename(date = new Date()): string {
  return `清冊稽核_系統缺少_${dateStamp(date)}.csv`;
}

/** 「系統有、清冊沒有」CSV 檔名（見 spec §6）。 */
export function missingInListFilename(date = new Date()): string {
  return `清冊稽核_清冊缺少_${dateStamp(date)}.csv`;
}

/** 下載 UTF-8 BOM 文字檔；內容已有 BOM 時不重複加。 */
function downloadCsv(filename: string, content: string): void {
  const text = content.startsWith("\uFEFF") ? content : `\uFEFF${content}`;
  saveBlob(new Blob([text], { type: "text/csv;charset=utf-8" }), filename);
}

/** 產生並下載「清冊有、系統沒有」CSV（UTF-8 BOM、CRLF；見 spec §6）。 */
export function downloadMissingInSystemCsv(
  rows: InventoryAuditMissingInSystemRow[]
): void {
  downloadCsv(missingInSystemFilename(), buildMissingInSystemCsv(rows));
}

/** 產生並下載「系統有、清冊沒有」CSV（UTF-8 BOM、CRLF；見 spec §6）。 */
export function downloadMissingInListCsv(
  rows: InventoryAuditMissingInListRow[]
): void {
  downloadCsv(missingInListFilename(), buildMissingInListCsv(rows));
}
