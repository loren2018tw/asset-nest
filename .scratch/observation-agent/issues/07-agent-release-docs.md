# 07: 代理發佈產物與文件

**What to build:** prebuilt 代理產物與安裝文件：tag `agent-v*` 觸發 workflow 建置 musl 靜態 binary（x86_64 先行、aarch64 緊接）上傳 GitHub Release；安裝腳本預設從 Release 下載（`--version` 可指定、預設 latest；下載失敗引導自建）；README 補代理安裝章節（TLS／反向代理建議、認證碼輪替、覆蓋語意）。

**Blocked by:** 06

**Status:** done

- [x] tag 觸發 workflow 產出 x86_64 與 aarch64 musl 產物並上 Release
- [x] 安裝腳本預設下載對應架構產物；`--version` 可選版本；失敗時明確引導 `--source-dir`
- [x] 乾淨機器以一行指令完成安裝且代理上線
- [x] README 代理章節（安裝、認證碼、覆蓋語意、TLS 建議、輪替）
- [x] 文件與實際參數一致

## Comments

實作完成（未 commit、未建立 tag／Release、未實跑 workflow）。變更：新增 `.github/workflows/agent-release.yml`；擴充 `deploy/agent-install.sh`；`README.md` 新增「## 觀測代理」章節（原有「IP 觀測」本機探測描述依票 08 保留未動）。

- **`.github/workflows/agent-release.yml`**：觸發 `push` tag `agent-v*`＋`workflow_dispatch`（測試建置；非 tag 時 release job 不執行）；`permissions: contents: read`（release job 覆寫 `contents: write`）、`concurrency` 比照 install-test。matrix：x86_64 於 `ubuntu-latest`（`x86_64-unknown-linux-musl`）、aarch64 於 `ubuntu-24.04-arm`（`aarch64-unknown-linux-musl`，公開 repo 的標準 ARM runner）。步驟：`musl-tools`＋`cmake`（aws-lc-sys 的 C／CMake 依賴）→ 建立 `<uname -m>-linux-musl-gcc` → `musl-gcc` 別名（Ubuntu 的 musl-tools 只提供 `musl-gcc`，而 cc-rs 對 musl 目標先找 `<arch>-linux-musl-gcc`）→ `rustup target add` → 快取（`~/.cargo/registry`、`~/.cargo/git`、`agent/target`；key 含 target 與 `hashFiles('agent/Cargo.lock')`）→ `cargo build --release --locked --target <target> --manifest-path agent/Cargo.toml` → `file` 檢查靜態連結（x86_64 musl 產物是 `static-pie linked`，非 `statically linked`）→ 更名 `asset-nest-agent-{x86_64|aarch64}` → `upload-artifact@v5`。release job：`needs: build`、`if: startsWith(github.ref, 'refs/tags/agent-v')`，`download-artifact@v5` 後以 `gh release view || gh release create --generate-notes` 建 Release、`gh release upload --clobber` 上傳兩個產物（`GH_TOKEN: github.token`）。
- **`deploy/agent-install.sh`**：新增 `--version <tag>`（預設 `latest`；驗證僅接受 `latest`／`agent-v*`，例 `agent-v0.1.0`）。未給 `--source-dir` 時預設下載 prebuilt：`https://github.com/loren2018tw/asset-nest/releases/{latest/download|download/<tag>}/asset-nest-agent-<arch>`；`uname -m` 映射 `x86_64`／`aarch64`，未知架構明確拒絕並引導 `--source-dir`。下載至 `$INSTALL_DIR` 內暫存檔、成功才 `mv` 就位（失敗保留既有 binary、清掉暫存、不自動 fallback），錯誤訊息含可能原因與 `--source-dir` 範例。下載與建置路徑共用後續流程（`install_artifacts`／使用者／env 保留／systemd／健康檢查）；摘要新增「安裝來源」。`--source-dir` 路徑改為只安裝 `build-essential`＋`cmake`（原 clone 路徑與 `--ref` 移除——spec 的介面沒有 `--ref`，且下載失敗不自動建置，clone 路徑已無入口；`--source-dir` 自建需要 cmake，原清單缺，CI 是因 runner 預裝才可用）。`usage()` 同步更新。
- **順帶修正 `read_auth_code_file`（票 06 的宣稱與實際不符）**：原實作取「第一個非空行」，但後端產生的 env 檔第一行是產生時間註解，`--auth-code-file /etc/asset-nest/asset-nest.env` 實際會取到註解字串；改為優先取含 `AGENT_AUTH_CODE=` 的行、否則第一個非空行（純認證碼檔仍可用），並去 CR／成對引號。這讓 README 一行安裝範例（`--auth-code-file` 直接指向後端 env 檔）真正可用。
- **README**：新增「## 觀測代理」章節——一行安裝範例（同機直接 `--auth-code-file /etc/asset-nest/asset-nest.env`；遠端以 `--auth-code` 並說明取得方式）、選項表（`--subnet`／`--name`／`--sweep-interval`／`--rate`／`--version`／`--source-dir`）、代理行為與覆蓋語意（持續被動＋週期掃描、心跳與在線門檻、未觀測／從未上線區分、資料保留）、系統狀態頁「未對應」與「被拒回報」意義、認證碼輪替步驟、跨不可信網路以反向代理提供 TLS、移除方式。名稱與實際參數逐一核對。
- **本機驗證（未 push／未建 tag／未安裝服務）**：
  - workflow：PyYAML `safe_load` 解析通過；actionlint v1.7.12 對 `agent-release.yml` 無警告（`install-test.yml` 僅既有 `ubuntu-26.04` label 不在 actionlint 清單，非本票變更）。
  - installer：`bash -n`、`--help`；以 source＋stub 執行 34 項函式測試全過——架構映射（x86_64／aarch64／未知拒絕含 `--source-dir` 引導）、URL 組法（latest 與指定 tag）、`--version` 驗證、`--ref` 已不接受、下載成功就位／失敗保留舊 binary 且不留暫存、`install_artifacts` 就位略過複製與建置複製、`main` 下載／原始碼兩分支（stub）。
  - 真實 musl 建置：以本機下載的 musl 工具鏈執行 `cargo build --release --locked --target x86_64-unknown-linux-musl` 成功（`static-pie linked`、可執行），驗證 workflow 的建置命令與靜態檢查；`curl` 實際打 `releases/latest/download/asset-nest-agent-x86_64` 得 404（尚未有 Release，符合預期）。
  - 全綠：`cargo test --manifest-path backend/Cargo.toml`（338 passed、0 failed）、`pnpm test:agent`（10 passed）、`pnpm lint:check`、`pnpm --filter frontend typecheck`。
- **待 tag 驗證**：發佈流程與「乾淨機器一行安裝」未實跑——首次推 `agent-v*` tag 時才會驗證 aarch64 ARM runner、musl 別名、`gh release upload` 與 installer 實際下載；`install-test.yml` 維持 `--source-dir` 路徑（未有 Release 前下載路徑無法在 CI 驗證）。若 ARM runner label 或 `gh` 行為有出入，再依實際錯誤調整。
