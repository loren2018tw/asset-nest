# 06 — 前端：網段表單與匯入呈現

Status: ready-for-agent
Blocked by: 01、05

## 目標

實作 `spec §10.1、§10.4`：網段表單的排除範圍編輯；匯入預覽與問題列報告新增欄位。

## 範圍

- `frontend/src/api/subnets.ts`：
  - `SubnetExclusion { id, start_ip, end_ip, note: string | null }`；`Subnet.exclusions`。
  - `SubnetExclusionInput { start_ip, end_ip, note: string | null }`；`SubnetInput.exclusions`。
  - `SubnetImportRowData.exclusions: string[]`。
- `frontend/src/components/SubnetFormDialog.vue`：
  - v4 新增「排除範圍」區塊（pool 之後）：每列起點／終點／用途說明（選填）＋刪除；「新增排除範圍」按鈕；空清單提示文案「排除範圍內位址不可指派（例：NAT 對外）；不得與 pool 或彼此重疊」。
  - 即時檢查（後端仍權威）：IPv4、端點必填、起點 ≤ 終點、端點在 CIDR 內、彼此不重疊、與 pool 不重疊、用途說明不含 `|`；`LiveIssue.field` 增加 `"exclusions"`，與 pool 相同的 banner 呈現。
  - `prepare()` 載入 `detail.exclusions`（含 note）；`toInput()` v4 帶排除範圍、v6 帶空陣列。
- `frontend/src/components/SubnetImportDialog.vue`：預覽表新增「排除範圍」欄（`row.data.exclusions.join("|")`、空為「—」），位置在「位址池」與「備註」之間。
- `frontend/src/utils/subnetImport.ts`：`ISSUE_REPORT_HEADERS` 插入「排除範圍」；`buildIssueReportCsv` 對應插入 `cellText(data.exclusions)`。

## 驗收

- `pnpm --filter frontend typecheck`、`pnpm lint:check`（必要時先 `pnpm lint` 自動格式化）。
- 人工檢核（`pnpm dev`）：新增 v4 網段、加兩段排除範圍（含用途說明）→ 儲存；重新開啟確認值；輸入與 pool 重疊的範圍會即時顯示錯誤且無法送出；編輯 v6 網段不顯示此區塊。
- 匯入含「排除範圍」欄的 CSV，預覽與問題列報告欄位正確。

## 注意

- 不要 `git commit`；不要動 `IpListPage.vue`／`AssignIpDialog.vue`（票 07、08）。
