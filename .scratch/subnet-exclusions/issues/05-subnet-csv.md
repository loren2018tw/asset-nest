# 05 — 網段 CSV 匯出／匯入（排除範圍欄）

Status: ready-for-agent
Blocked by: 01

## 目標

實作 `spec §9`：匯出 7 欄；匯入新增選填「排除範圍」欄，支援 `起-迄[#用途說明]`。

## 範圍

- `backend/src/subnets.rs`：
  - `export_csv`：標題列改 `名稱, CIDR, Gateway, Kea subnet-id, 位址池, 排除範圍, 備註`；排除範圍段＝`起-迄`，有 note 時 `起-迄#note`，多段以 `|` 分隔；v6 一律空字串；列排序不變。
  - 單元測試更新：7 欄、含 note 的排除範圍、v6 兩欄皆空。
- `backend/src/subnet_import.rs`：
  - `Field` 插入 `Exclusions`（`Pools` 與 `Note` 之間）：`parse("排除範圍")`、`key() = "exclusions"`；`FIELD_COUNT` 7。
  - `validate_row`：解析排除範圍文字（`|` 分段；每段以**第一個** `#` 分為範圍與 note；範圍重用既有段解析器或等價新函式；note trim、空字串視為無）。
    - v6 有內容 → `exclusions_for_v6`（field `exclusions`）。
    - 段格式錯誤 → `invalid_exclusions`。
    - 建 `ExclusionInput` 併入 `SubnetInput`（領域驗證處理重疊等跨段規則）。
  - `validate_issue`：`exclusions`／`exclusions[...]` → `invalid_exclusions`、field `exclusions`。
  - `RowData.exclusions: Vec<String>`（正規化：`起-迄` 或 `起-迄#note`；v6 錯誤列以原始段呈現）；`mismatch_data` 一併填入。
- 測試：
  - `backend/tests/subnets.rs`：匯出 CSV 7 欄與排除範圍儲存格格式（含 note）斷言。
  - `backend/tests/subnet_import.rs`：
    - 7 欄標題（不分大小寫、順序不拘）解析；含 note 的段正確拆出。
    - `invalid_exclusions`、`exclusions_for_v6` 錯誤碼與 field。
    - 舊 6 欄檔（無「排除範圍」標題）仍可匯入（既有測試即為相容性測試，確認不被破壞）。
    - 匯出→匯入 round trip：含多段與 note（note 可含 `#`，以第一個 `#` 分隔）。

## 驗收

- `cargo test --manifest-path backend/Cargo.toml --lib subnets`、`--test subnet_import`、`--test subnets`。
- `pnpm test` 全綠；`cargo fmt --check`。

## 注意

- 前端顯示（匯入對話框欄位、問題列報告）是票 06，不在本票。
- `Field` 索引位移要同步 `RawRow.fields` 陣列長度與所有 match。
- 不要 `git commit`。
