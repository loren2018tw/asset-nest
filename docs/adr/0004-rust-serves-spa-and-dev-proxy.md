# ADR-0004：Rust 同源服務前端；開發以 Vite proxy 連後端

- 狀態：已接受
- 日期：2026-10-04

## 背景

前端是 Quasar（SPA），後端是 Rust API。需要一致的開發與部署連線方式，並避免環境差異與 CORS 設定。

## 決策

- 前端程式一律使用相對路徑 `/api`。
- 開發：Quasar dev server（9000）透過 Vite proxy 將 `/api` 轉發到 Rust（8080）；不使用 CORS。
- 部署：Rust 以靜態檔服務提供 Quasar 建置產物（`frontend/dist/spa`，history 路由 fallback），與 API 同源；可用 `WEB_DIST_DIR` 覆寫位置。
- 前端產物不嵌入 binary（不採用 rust-embed）；部署物為 binary＋dist 目錄。
- 保留 `VITE_API_BASE_URL` 覆寫（預設空），以備未來前端獨立部署。

## 理由

- 同源免 CORS、免環境變數切換，dev 與 prod 行為一致。
- ServeDir 讓前端可獨立更新，不必重編後端。

## 後果

- 部署需一併攜帶 dist 目錄。
- 若未來改為前端獨立主機／CDN，需啟用 `VITE_API_BASE_URL` 並補上 CORS 設定（目前刻意不做）。
