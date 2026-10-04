import { saveBlob } from "@/api/client";
import type { SubnetImportReport, SubnetImportRow } from "@/api/subnets";

/** 問題列報告欄位＝列號＋6 欄＋原因（見 spec §3、ADR-0009）。 */
const ISSUE_REPORT_HEADERS = [
  "列號",
  "名稱",
  "CIDR",
  "Gateway",
  "Kea subnet-id",
  "位址池",
  "備註",
  "原因"
];

/** 問題列＝有 issues 的列（警示或錯誤）。 */
export function issueRowsOf(report: SubnetImportReport): SubnetImportRow[] {
  return report.rows.filter(row => row.issues.length > 0);
}

/** RFC 4180：含逗號、引號或換行者以雙引號包住，內部引號加倍。 */
function csvCell(value: string): string {
  return /[",\r\n]/.test(value) ? `"${value.replace(/"/g, '""')}"` : value;
}

/** 欄位值轉字串：null 空字串、位址池以「|」串接（與匯入格式一致）。 */
function cellText(value: string | number | string[] | null): string {
  if (value === null) {
    return "";
  }
  return Array.isArray(value) ? value.join("|") : String(value);
}

/** 由匯入回應產生問題列報告 CSV（不含 BOM；見 spec §3）。 */
export function buildIssueReportCsv(report: SubnetImportReport): string {
  const lines = [ISSUE_REPORT_HEADERS.join(",")];

  for (const row of issueRowsOf(report)) {
    const data = row.data;
    const cells = [
      String(row.row_number),
      cellText(data.name),
      cellText(data.cidr),
      cellText(data.gateway),
      cellText(data.kea_subnet_id),
      cellText(data.pools),
      cellText(data.note),
      row.issues.map(issue => issue.message).join("；")
    ];
    lines.push(cells.map(csvCell).join(","));
  }

  return `${lines.join("\r\n")}\r\n`;
}

/** 問題列報告檔名（依本機日期，格式 `網段匯入問題報告_YYYYMMDD.csv`）。 */
export function issueReportFilename(date = new Date()): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `網段匯入問題報告_${year}${month}${day}.csv`;
}

/** 下載 UTF-8 BOM 文字檔；內容已有 BOM 時不重複加。 */
function downloadCsv(filename: string, content: string): void {
  const text = content.startsWith("\uFEFF") ? content : `\uFEFF${content}`;
  saveBlob(new Blob([text], { type: "text/csv;charset=utf-8" }), filename);
}

/** 產生並下載問題列報告（UTF-8 BOM；見 spec §3）。 */
export function downloadImportIssueReport(report: SubnetImportReport): void {
  downloadCsv(issueReportFilename(), buildIssueReportCsv(report));
}
