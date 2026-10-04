import type { ImportReport, ImportRow } from "@/api/assets";
import templateCsv from "@/assets/資產匯入範本.csv?raw";

/** 範本檔名（見 spec §6）。 */
export const ASSET_IMPORT_TEMPLATE_FILENAME = "資產匯入範本.csv";

/** 問題列報告欄位＝列號＋14 欄＋原因（見 spec §6、ADR-0008）。 */
const ISSUE_REPORT_HEADERS = [
  "列號",
  "財產編號",
  "描述",
  "位置",
  "設備序號",
  "廠牌",
  "型號",
  "購置日期",
  "年限",
  "備註",
  "標籤",
  "MAC",
  "IPv4",
  "IPv6",
  "hostname",
  "原因"
];

/** 問題列＝有 issues 的列（警示或錯誤）。 */
export function issueRowsOf(report: ImportReport): ImportRow[] {
  return report.rows.filter(row => row.issues.length > 0);
}

/** RFC 4180：含逗號、引號或換行者以雙引號包住，內部引號加倍。 */
function csvCell(value: string): string {
  return /[",\r\n]/.test(value) ? `"${value.replace(/"/g, '""')}"` : value;
}

/** 14 欄值轉字串：null 空字串、標籤以「|」串接（與匯入格式一致）。 */
function cellText(value: string | number | string[] | null): string {
  if (value === null) {
    return "";
  }
  return Array.isArray(value) ? value.join("|") : String(value);
}

/** 由匯入回應產生問題列報告 CSV（不含 BOM；見 spec §6）。 */
export function buildIssueReportCsv(report: ImportReport): string {
  const lines = [ISSUE_REPORT_HEADERS.join(",")];

  for (const row of issueRowsOf(report)) {
    const data = row.data;
    const cells = [
      String(row.row_number),
      cellText(data.property_no),
      cellText(data.description),
      cellText(data.location),
      cellText(data.device_serial),
      cellText(data.brand),
      cellText(data.model),
      cellText(data.purchase_date),
      cellText(data.lifespan_years),
      cellText(data.note),
      cellText(data.tags),
      cellText(data.mac),
      cellText(data.ipv4),
      cellText(data.ipv6),
      cellText(data.hostname),
      row.issues.map(issue => issue.message).join("；")
    ];
    lines.push(cells.map(csvCell).join(","));
  }

  return `${lines.join("\r\n")}\r\n`;
}

/** 問題列報告檔名（依本機日期，格式 `匯入問題報告_YYYYMMDD.csv`）。 */
export function issueReportFilename(date = new Date()): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `匯入問題報告_${year}${month}${day}.csv`;
}

/** 下載 UTF-8 BOM 文字檔；內容已有 BOM 時不重複加。 */
function downloadCsv(filename: string, content: string): void {
  const text = content.startsWith("\uFEFF") ? content : `\uFEFF${content}`;
  const blob = new Blob([text], { type: "text/csv;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  anchor.click();
  URL.revokeObjectURL(url);
}

/** 下載匯入範本（內容與 `docs/資產匯入範本.csv` 一致、UTF-8 BOM）。 */
export function downloadAssetImportTemplate(): void {
  downloadCsv(ASSET_IMPORT_TEMPLATE_FILENAME, templateCsv);
}

/** 產生並下載問題列報告（UTF-8 BOM；見 spec §6）。 */
export function downloadImportIssueReport(report: ImportReport): void {
  downloadCsv(issueReportFilename(), buildIssueReportCsv(report));
}
