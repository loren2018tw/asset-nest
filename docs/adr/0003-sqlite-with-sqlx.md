# ADR-0003：持久化採 SQLite＋sqlx

- 狀態：已接受
- 日期：2026-10-04

## 背景

asset-nest 需要持久化資產、Interface、IP 與同步狀態（Kealight 沒有資料庫）。目標部署環境為單機內網、少數管理員使用。

## 決策

採用 SQLite（以 sqlx 存取，bundled 方式，不需系統安裝 sqlite3）。啟動時自動套用內嵌 migrations；資料存取經 repository 邊界隔離，未來可換 PostgreSQL。

## 理由

- 零系統依賴、單檔備份、維運單純。
- 對單機、少數管理員的規模，PostgreSQL 是過度設計。
- repository 邊界讓換庫成本可控。

## 後果

- 寫入併發有限（以 WAL 模式因應）；若未來多站部署或高併發，需重新評估。
- 備份策略以複製資料庫檔案為主（細節於功能階段訂定）。
