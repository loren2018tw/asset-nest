# 08 — 前端：指派輸入候選與提示

Status: ready-for-agent
Blocked by: 04

## 目標

實作 `spec §10.3`：指派 IP 對話框輸入前綴即時篩選可用位址；完整位址不可用時顯示原因。

## 範圍

- `frontend/src/utils/cidr.ts`：新增 `parseIpv4Prefix(text)`：
  - 語意與後端 `spec §8` 相同：`.` 分段、結尾點＝全為完整 octet；完整 octet 須 0–255、前綴段 1–3 位數字；超過 4 段、空段、非數字回 `null`。
  - 至少 2 個完整 octet 才回值（否則 `null`，代表尚不可查詢）；回傳足以供查詢使用的結構（自訂，例如 `{ completeOctets, query }`）。
- 新增 `frontend/src/api/ipCandidates.ts`：
  - `IpCandidate { address, subnet_id, subnet_cidr, subnet_name: string | null }`
  - `IpQueryStatus { address, status: "available" | "in_pool" | "excluded" | "static" | "reservation" | "out_of_subnet", subnet_id: number | null, subnet_cidr: string | null, subnet_name: string | null }`
  - `findIpCandidates(q: string, limit = 20): Promise<{ items: IpCandidate[]; query_status: IpQueryStatus | null }>` → `GET /api/v1/ip-candidates`。
- `frontend/src/components/AssignIpDialog.vue`：
  - 位址欄改 `q-select`：`use-input`、`hide-selected`、`fill-input`、`input-debounce="300"`；選項文字為位址、次要文字「網段名｜CIDR」；**仍可自由輸入**任何位址（移轉既有指派不會出現在候選）。
  - 查詢：`parseIpv4Prefix` 不為 `null` 時呼叫 `findIpCandidates`；以 token 丟棄過期回應；請求失敗靜默（不得阻擋輸入／送出）。
  - 查無結果：顯示提示「此範圍無可用位址」。
  - `query_status` 提示（完整位址）：
    - `in_pool` →「此位址在 DHCP 位址池內，不可指派」（負向）
    - `excluded` →「此位址在排除範圍內，不可指派」（負向）
    - `static`／`reservation` →「此位址已指派；送出後可確認移轉」（中性）
    - `out_of_subnet` →「此位址不在任何網段可指派的範圍內」（負向）
    - `available` → 不顯示
  - v6 輸入（`parseAddress` family 為 ipv6）不查候選、不顯示候選提示。
  - 既有提交邏輯（`submit`／`assignFromAsset`／移轉確認）不得改行為；`address` 值的取得方式需確保「未選候選、直接輸入完整位址按指派」與「鍵入後未按 Enter 直接送出」都能取得目前文字。

## 驗收

- `pnpm --filter frontend typecheck`、`pnpm lint:check`（必要時先 `pnpm lint`）。
- 人工檢核（`pnpm dev`）：
  - 輸入 `10.0.1.` 顯示可用位址（不含 pool／排除／已指派）；`10.0.1`（未打點）符合前綴語意。
  - 輸入完整排除位址顯示「在排除範圍內」提示；輸入已指派位址顯示可移轉提示；送出流程與原行為一致（含移轉確認）。
  - 輸入 IPv6 不觸發候選查詢。

## 注意

- 不要 `git commit`；不要改 `AssignmentDialog.vue`（v6 登錄／v4 編輯對話框）。
