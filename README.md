# asset-nest — IT 資產整合管理系統

Rust（axum）後端 + Quasar（Vue 3 / Vite）前端的整合系統骨架。資產、IP 位址與 Kea DHCP 保留同步功能陸續開發中（目前僅骨架）。

- 領域詞彙：`GLOSSARY.md`
- 決策記錄：`docs/adr/`
- Issue 追蹤：`.scratch/`（見 `AGENTS.md`）

## 環境需求

- Node.js >= 24、pnpm 11
- Rust（stable，見 `rust-toolchain.toml`）

## 開發

```sh
pnpm install
pnpm dev          # Quasar dev server (9000) + Rust 後端 (8080)
```

開啟 http://localhost:9000。前端以相對路徑 `/api` 呼叫後端，開發時由 Vite proxy 轉發（見 `docs/adr/0004`）。

## 建置與部署

```sh
pnpm build        # quasar build → cargo build --release
./backend/target/release/asset-nest   # 同源服務 API 與前端，http://localhost:8080
```

部署物：`backend/target/release/asset-nest` + `frontend/dist/spa/`（可用 `WEB_DIST_DIR` 覆寫路徑）。環境設定見 `.env.example`。

## 常用指令

| 指令 | 說明 |
|------|------|
| `pnpm dev` | 同時啟動前後端 |
| `pnpm build` | 建置前端與後端（release） |
| `pnpm lint:check` | 前端 lint / format 檢查 |
| `pnpm test` | 後端測試（cargo test） |
| `pnpm test:kea` | Kea 真機連線測試（version-get；讀 `.env` 的 `KEA_API_URL`） |
