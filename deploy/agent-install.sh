#!/usr/bin/env bash
#
# asset-nest 觀測代理安裝腳本（Ubuntu 24.04／26.04；需 root 與 systemd）
#
# 安裝內容：
#   - 預設自 GitHub Release 下載 prebuilt binary（musl 靜態、x86_64／aarch64）
#   - --source-dir 時改以既有 checkout 建置（Rust stable；不自動下載）
#   - 專用系統使用者 asset-nest-agent（無 home、nologin）
#   - /opt/asset-nest-agent/asset-nest-agent（release binary）
#   - /etc/asset-nest-agent/agent.env（0640 root:asset-nest-agent）
#   - systemd 服務 asset-nest-agent（CAP_NET_RAW 最小權限；見 docs/adr/0015、0018）
#
# 可重複執行（重跑即更新；保留既有 AGENT_INSTANCE_ID 與認證碼，除非明確覆寫）。
# 決策見 docs/adr/0018、docs/adr/0019；可直接 source 本檔取用函式做測試
# （不會執行安裝）。
set -euo pipefail

# ---- 常數／預設值 -----------------------------------------------------------

RELEASE_REPO="loren2018tw/asset-nest"
RELEASES_BASE_URL="https://github.com/${RELEASE_REPO}/releases"
SOURCE_DIR=""
AGENT_VERSION="latest"

INSTALL_DIR="/opt/asset-nest-agent"
CONFIG_DIR="/etc/asset-nest-agent"
ENV_FILE="$CONFIG_DIR/agent.env"
SERVICE_USER="asset-nest-agent"
SERVICE_UNIT="/etc/systemd/system/asset-nest-agent.service"
BINARY_NAME="asset-nest-agent"

SERVER_URL=""
AUTH_CODE_FLAG=""
AUTH_CODE_FILE=""
AUTH_CODE_ENV="${AGENT_AUTH_CODE:-}"
SUBNET=""
AGENT_NAME=""
SWEEP_INTERVAL=""
SWEEP_RATE=""

# 執行期間解析（依 --auth-code-file／既有 env 檔決定）。
AUTH_CODE=""
INSTANCE_ID=""
BIN_PATH=""
ARCH=""
BIN_SOURCE=""

# ---- 輸出工具 ---------------------------------------------------------------

info() { printf '\033[1;34m[asset-nest-agent]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[asset-nest-agent]\033[0m %s\n' "$*" >&2; }
die() {
  printf '\033[1;31m[asset-nest-agent]\033[0m %s\n' "$*" >&2
  exit 1
}

# ---- 用法 -------------------------------------------------------------------

usage() {
  cat <<'EOF'
用法：sudo ./deploy/agent-install.sh --server-url <url> [認證碼選項] [選項]

在 Ubuntu 24.04／26.04 上安裝 asset-nest 觀測代理：預設自 GitHub Release
下載 prebuilt binary（musl 靜態；x86_64／aarch64），亦可指定 --source-dir
自既有 checkout 建置。建立專用系統使用者與 systemd 服務（asset-nest-agent）。
可重複執行（重跑即更新；保留既有 instance id 與認證碼，除非本次明確覆寫）。

必填：
  --server-url <url>         後端位址（http:// 或 https://；
                             例：http://nest.local:8080）

認證碼（三選一；重跑未提供時沿用既有設定）：
  --auth-code <code>         代理認證碼（後端 AGENT_AUTH_CODE）
  --auth-code-file <path>    自檔案讀取（優先取 AGENT_AUTH_CODE= 行；
                             可直接指向後端 /etc/asset-nest/asset-nest.env）
  環境變數 AGENT_AUTH_CODE   由行程環境提供

選填：
  --version <tag>            代理版本標籤（預設 latest；例：agent-v0.1.0；
                             僅 prebuilt 下載路徑，--source-dir 時無作用）
  --subnet <CIDR>            要觀測的 IPv4 網段（預設由本機介面自動偵測；
                             多候選或找不到時必須明確指定）
  --name <name>              代理名稱（預設 hostname）
  --sweep-interval <secs>    主動掃描間隔秒數（預設 900；首次在間隔後）
  --rate <pps>               掃描每秒探測數上限（預設 1000）
  --source-dir <path>        改以既有 checkout 建置（不 clone、不下載）
  -h, --help                 顯示本說明

安裝後位置：
  /opt/asset-nest-agent/asset-nest-agent       代理程式（release）
  /etc/asset-nest-agent/agent.env              環境設定（0640 root:asset-nest-agent）
  /etc/systemd/system/asset-nest-agent.service systemd 服務
EOF
}

# ---- 小工具 -----------------------------------------------------------------

version_ge() {
  # version_ge <actual> <required>
  [ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -n1)" = "$2" ]
}

apt_install() {
  env DEBIAN_FRONTEND=noninteractive apt-get install -y "$@"
}

generate_uuid() {
  if [ -r /proc/sys/kernel/random/uuid ]; then
    cat /proc/sys/kernel/random/uuid
  elif command -v uuidgen >/dev/null 2>&1; then
    uuidgen | tr '[:upper:]' '[:lower:]'
  else
    die "找不到可用的 UUID 來源（/proc/sys/kernel/random/uuid 或 uuidgen）。"
  fi
}

read_env_value() {
  # read_env_value <變數名>：自既有 env 檔取第一筆值；不可讀或無此變數回空。
  [ -r "$ENV_FILE" ] || return 0
  grep -m1 "^$1=" "$ENV_FILE" | cut -d= -f2- || true
}

read_auth_code_file() {
  # read_auth_code_file <path>：優先取含 "AGENT_AUTH_CODE=" 的行（可直接指向
  # 後端 /etc/asset-nest/asset-nest.env）；否則取第一個非空行（可為純認證碼）。
  # 皆去尾端 CR 與成對引號。
  local line value
  line="$(grep -m1 'AGENT_AUTH_CODE=' "$1" || true)"
  [ -n "$line" ] || line="$(grep -m1 -v '^[[:space:]]*$' "$1" || true)"
  line="${line%$'\r'}"
  value="${line#*AGENT_AUTH_CODE=}"
  case "$value" in
    \"*\") value="${value#\"}"; value="${value%\"}" ;;
    \'*\') value="${value#\'}"; value="${value%\'}" ;;
  esac
  printf '%s' "$value"
}

validate_positive_int() {
  # validate_positive_int <value> <option>
  case "$1" in
    ''|*[!0-9]*) die "$2 需為正整數（目前：${1:-空}）。" ;;
  esac
  [ "$((10#$1))" -gt 0 ] || die "$2 需為正整數（收到 0）。"
}

validate_ipv4_cidr() {
  # validate_ipv4_cidr <value> <option>
  printf '%s' "$1" | grep -qE '^([0-9]{1,3}\.){3}[0-9]{1,3}/[0-9]{1,2}$' \
    || die "$2 需為 IPv4 CIDR（例：10.1.0.0/24；目前：$1）。"
  local prefix="${1##*/}"
  [ "$((10#$prefix))" -le 32 ] || die "$2 的遮罩長度需介於 0–32（目前：$1）。"
}

# ---- 前置檢查 ---------------------------------------------------------------

require_root() {
  [ "$(id -u)" -eq 0 ] || die "請以 root 執行（sudo ./deploy/agent-install.sh）。"
}

require_supported_os() {
  [ -r /etc/os-release ] || die "找不到 /etc/os-release，無法確認系統版本。"
  # shellcheck disable=SC1091
  local id version
  id="$(. /etc/os-release && printf '%s' "${ID:-}")"
  version="$(. /etc/os-release && printf '%s' "${VERSION_ID:-}")"
  [ "$id" = "ubuntu" ] || die "僅支援 Ubuntu（目前：${id:-未知}）。"
  case "$version" in
    24.04*|26.04*) info "系統版本：Ubuntu $version" ;;
    *) die "僅支援 Ubuntu 24.04／26.04（目前：$version）。" ;;
  esac
}

require_systemd() {
  command -v systemctl >/dev/null 2>&1 && [ -d /run/systemd/system ] \
    || die "本腳本需要 systemd。"
}

# ---- 參數解析 ---------------------------------------------------------------

parse_args() {
  while [ $# -gt 0 ]; do
    case "$1" in
      --server-url) SERVER_URL="${2:?--server-url 需要值}"; shift 2 ;;
      --auth-code) AUTH_CODE_FLAG="${2:?--auth-code 需要值}"; shift 2 ;;
      --auth-code-file) AUTH_CODE_FILE="${2:?--auth-code-file 需要值}"; shift 2 ;;
      --version) AGENT_VERSION="${2:?--version 需要值}"; shift 2 ;;
      --subnet) SUBNET="${2:?--subnet 需要值}"; shift 2 ;;
      --name) AGENT_NAME="${2:?--name 需要值}"; shift 2 ;;
      --sweep-interval) SWEEP_INTERVAL="${2:?--sweep-interval 需要值}"; shift 2 ;;
      --rate) SWEEP_RATE="${2:?--rate 需要值}"; shift 2 ;;
      --source-dir) SOURCE_DIR="${2:?--source-dir 需要值}"; shift 2 ;;
      -h|--help) usage; exit 0 ;;
      *) die "未知選項：$1（--help 看用法）" ;;
    esac
  done
  validate_args
}

validate_args() {
  [ -n "$SERVER_URL" ] || die "--server-url 為必填（例：--server-url http://nest.local:8080）。"
  case "$SERVER_URL" in
    http://?*|https://?*) ;;
    *) die "--server-url 需為 http:// 或 https:// 開頭（目前：$SERVER_URL）。" ;;
  esac
  # 去除尾端斜線（代理端亦會正規化；此處先收斂寫入值與健康檢查網址）。
  while [ "${SERVER_URL%/}" != "$SERVER_URL" ]; do
    SERVER_URL="${SERVER_URL%/}"
  done
  case "$SERVER_URL" in
    http://?*|https://?*) ;;
    *) die "--server-url 位址不完整（目前：$SERVER_URL）。" ;;
  esac

  if [ -n "$AUTH_CODE_FLAG" ] && [ -n "$AUTH_CODE_FILE" ]; then
    die "認證碼僅能由 --auth-code、--auth-code-file、環境變數 AGENT_AUTH_CODE 擇一提供。"
  fi
  if [ -n "$AUTH_CODE_FILE" ] && [ ! -r "$AUTH_CODE_FILE" ]; then
    die "認證碼檔不存在或不可讀：$AUTH_CODE_FILE"
  fi

  if [ -n "$SUBNET" ]; then
    validate_ipv4_cidr "$SUBNET" "--subnet"
  fi
  if [ -n "$SWEEP_INTERVAL" ]; then
    validate_positive_int "$SWEEP_INTERVAL" "--sweep-interval"
  fi
  if [ -n "$SWEEP_RATE" ]; then
    validate_positive_int "$SWEEP_RATE" "--rate"
  fi

  case "$AGENT_VERSION" in
    latest|agent-v*) ;;
    *) die "--version 需為 latest 或 agent-v* 版本標籤（例：agent-v0.1.0；目前：$AGENT_VERSION）。" ;;
  esac

  if [ -n "$SOURCE_DIR" ]; then
    [ -f "$SOURCE_DIR/agent/Cargo.toml" ] \
      || die "--source-dir 內找不到 agent/Cargo.toml：$SOURCE_DIR"
  fi
}

# ---- 認證碼與 instance id ----------------------------------------------------

resolve_auth_code() {
  local provided="" origin=""

  if [ -n "$AUTH_CODE_FLAG" ]; then
    provided="$AUTH_CODE_FLAG"
    origin="--auth-code"
  elif [ -n "$AUTH_CODE_FILE" ]; then
    [ -r "$AUTH_CODE_FILE" ] || die "認證碼檔不存在或不可讀：$AUTH_CODE_FILE"
    provided="$(read_auth_code_file "$AUTH_CODE_FILE")"
    [ -n "$provided" ] || die "認證碼檔為空或格式不符：$AUTH_CODE_FILE"
    origin="--auth-code-file（$AUTH_CODE_FILE）"
  elif [ -n "$AUTH_CODE_ENV" ]; then
    provided="$AUTH_CODE_ENV"
    origin="環境變數 AGENT_AUTH_CODE"
  fi

  if [ -z "$provided" ]; then
    provided="$(read_env_value AGENT_AUTH_CODE)"
    [ -n "$provided" ] && origin="$ENV_FILE（沿用既有）"
  fi
  if [ -z "$provided" ]; then
    die "未提供代理認證碼：請以 --auth-code、--auth-code-file 或環境變數 AGENT_AUTH_CODE 提供；首次安裝可自後端取得：sudo grep AGENT_AUTH_CODE /etc/asset-nest/asset-nest.env"
  fi

  AUTH_CODE="$provided"
  info "代理認證碼來源：$origin"
}

resolve_instance_id() {
  local existing
  existing="$(read_env_value AGENT_INSTANCE_ID)"
  if [ -n "$existing" ]; then
    INSTANCE_ID="$existing"
    info "沿用既有 instance id：$INSTANCE_ID"
    return
  fi
  INSTANCE_ID="$(generate_uuid)"
  info "產生 instance id：$INSTANCE_ID"
}

# ---- 系統套件與工具鏈 -------------------------------------------------------

install_base_packages() {
  info "安裝基本套件（ca-certificates、curl）..."
  env DEBIAN_FRONTEND=noninteractive apt-get update -qq
  apt_install ca-certificates curl
}

install_build_packages() {
  info "安裝建置套件（build-essential、cmake）..."
  apt_install build-essential cmake
}

prepare_source() {
  SRC_DIR="$(cd "$SOURCE_DIR" && pwd)"
  info "使用現有原始碼：$SRC_DIR"
}

detect_arch() {
  # prebuilt 產物命名用架構（asset-nest-agent-<arch>）。
  case "$(uname -m)" in
    x86_64) ARCH="x86_64" ;;
    aarch64) ARCH="aarch64" ;;
    *) die "不支援的處理器架構：$(uname -m)；prebuilt 產物僅提供 x86_64 與 aarch64。請改用 --source-dir 於本機建置。" ;;
  esac
  info "處理器架構：$ARCH"
}

prebuilt_url() {
  # prebuilt_url：依 AGENT_VERSION 組出 GitHub Release 下載網址。
  local file="asset-nest-agent-$ARCH"
  if [ "$AGENT_VERSION" = "latest" ]; then
    printf '%s/latest/download/%s\n' "$RELEASES_BASE_URL" "$file"
  else
    printf '%s/download/%s/%s\n' "$RELEASES_BASE_URL" "$AGENT_VERSION" "$file"
  fi
}

download_prebuilt() {
  detect_arch
  local url tmp
  url="$(prebuilt_url)"
  info "下載 prebuilt 代理（版本：$AGENT_VERSION）：$url"
  install -d -m 0755 "$INSTALL_DIR"
  tmp="$(mktemp "$INSTALL_DIR/.asset-nest-agent.XXXXXX")"
  if ! curl -fSL --proto '=https' --tlsv1.2 --max-time 300 -o "$tmp" "$url"; then
    rm -f "$tmp"
    die "prebuilt 下載失敗：$url
  可能原因：版本不存在、該架構尚未發佈，或網路／Proxy 受限。
  請確認 --version（目前：$AGENT_VERSION），或改用 --source-dir 於本機以原始碼建置：
    sudo ./deploy/agent-install.sh --server-url <url> --source-dir <checkout> [認證碼選項]"
  fi
  if [ ! -s "$tmp" ]; then
    rm -f "$tmp"
    die "prebuilt 下載內容為空：$url"
  fi
  chmod 0755 "$tmp"
  mv -f "$tmp" "$INSTALL_DIR/$BINARY_NAME"
  BIN_PATH="$INSTALL_DIR/$BINARY_NAME"
  BIN_SOURCE="prebuilt（$AGENT_VERSION、$ARCH）"
}

install_rust() {
  export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
  if command -v cargo >/dev/null 2>&1 \
    && version_ge "$(cargo --version | awk '{print $2}')" "1.85.0"; then
    info "已安裝 Rust $(cargo --version | awk '{print $2}')"
    return
  fi

  info "安裝 Rust stable（rustup，minimal）..."
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain stable
  export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
  command -v cargo >/dev/null 2>&1 || die "Rust 安裝失敗。"
}

build_agent() {
  info "cargo build（agent release）..."
  (cd "$SRC_DIR" && cargo build --release --manifest-path agent/Cargo.toml)
  BIN_PATH="${CARGO_TARGET_DIR:-$SRC_DIR/agent/target}/release/$BINARY_NAME"
  [ -x "$BIN_PATH" ] || die "找不到代理建置產物：$BIN_PATH"
  BIN_SOURCE="原始碼建置（$SRC_DIR）"
}

install_artifacts() {
  info "安裝程式至 $INSTALL_DIR ..."
  install -d -m 0755 "$INSTALL_DIR"
  # 下載路徑已直接就位（mv），僅建置路徑需要複製。
  if [ "$BIN_PATH" != "$INSTALL_DIR/$BINARY_NAME" ]; then
    install -m 0755 "$BIN_PATH" "$INSTALL_DIR/$BINARY_NAME"
  fi
  chown root:root "$INSTALL_DIR/$BINARY_NAME"
}

ensure_service_user() {
  if ! id -u "$SERVICE_USER" >/dev/null 2>&1; then
    info "建立系統使用者 ${SERVICE_USER} ..."
    adduser --system --group --quiet --no-create-home --home /nonexistent \
      --shell /usr/sbin/nologin --disabled-password --disabled-login "$SERVICE_USER"
  fi
}

# ---- 代理服務 ---------------------------------------------------------------

write_env_file() {
  install -d -m 0750 -o root -g "$SERVICE_USER" "$CONFIG_DIR"
  info "寫入環境設定（$ENV_FILE）..."
  {
    printf '# 由 deploy/agent-install.sh 產生（%s）\n' "$(date -Iseconds)"
    printf 'AGENT_SERVER_URL=%s\n' "$SERVER_URL"
    printf 'AGENT_AUTH_CODE=%s\n' "$AUTH_CODE"
    printf 'AGENT_INSTANCE_ID=%s\n' "$INSTANCE_ID"
    if [ -n "$SUBNET" ]; then
      printf 'AGENT_SUBNET_CIDR=%s\n' "$SUBNET"
    fi
    if [ -n "$AGENT_NAME" ]; then
      printf 'AGENT_NAME=%s\n' "$AGENT_NAME"
    fi
    if [ -n "$SWEEP_INTERVAL" ]; then
      printf 'AGENT_SWEEP_INTERVAL_SECS=%s\n' "$SWEEP_INTERVAL"
    fi
    if [ -n "$SWEEP_RATE" ]; then
      printf 'AGENT_SWEEP_RATE_PPS=%s\n' "$SWEEP_RATE"
    fi
  } >"$ENV_FILE"
  chown root:"$SERVICE_USER" "$ENV_FILE"
  chmod 0640 "$ENV_FILE"
}

write_unit_file() {
  info "建立 systemd 服務（asset-nest-agent.service）..."
  cat >"$SERVICE_UNIT" <<EOF
[Unit]
Description=asset-nest 觀測代理（ARP 掃描與被動監聽）
Documentation=https://github.com/loren2018tw/asset-nest
Wants=network-online.target
After=network-online.target

[Service]
Type=simple
User=${SERVICE_USER}
Group=${SERVICE_USER}
EnvironmentFile=${ENV_FILE}
ExecStart=${INSTALL_DIR}/${BINARY_NAME}
Restart=on-failure
RestartSec=3
NoNewPrivileges=true
# ARP 觀測（raw socket）的最小權限（見 docs/adr/0015、docs/adr/0018）
AmbientCapabilities=CAP_NET_RAW
CapabilityBoundingSet=CAP_NET_RAW
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ProtectKernelTunables=true
ProtectControlGroups=true
RestrictSUIDSGID=true

[Install]
WantedBy=multi-user.target
EOF
  chmod 0644 "$SERVICE_UNIT"
}

# ---- 驗證與收尾 -------------------------------------------------------------

wait_service_active() {
  local attempt
  for attempt in $(seq 1 5); do
    if systemctl is-active --quiet asset-nest-agent; then
      info "asset-nest-agent 服務已啟動。"
      return
    fi
    sleep 1
  done
  die "asset-nest-agent 未啟動；請執行：journalctl -u asset-nest-agent"
}

verify_server_reachable() {
  info "檢查後端可達性（${SERVER_URL}/api/health）..."
  if curl -fsS --max-time 10 "${SERVER_URL}/api/health" >/dev/null 2>&1; then
    info "後端健康檢查通過。"
    return
  fi
  warn "無法連線 ${SERVER_URL}/api/health；代理已安裝並啟動、將自行重試，請確認："
  warn "  1. --server-url 是否正確（目前：$SERVER_URL）"
  warn "  2. 後端 asset-nest 是否執行中（systemctl status asset-nest）"
  warn "  3. 防火牆／反向代理是否允許此主機連線；TLS 憑證是否受信任"
  warn "服務紀錄：journalctl -u asset-nest-agent -f"
}

print_summary() {
  cat <<EOF

安裝完成。

  代理程式：${INSTALL_DIR}/${BINARY_NAME}
  安裝來源：${BIN_SOURCE}
  systemd：asset-nest-agent（User=${SERVICE_USER}、CAP_NET_RAW）
  環境檔：${ENV_FILE}（0640 root:${SERVICE_USER}）
  後端：${SERVER_URL}
  instance id：${INSTANCE_ID}
  掃描間隔：${SWEEP_INTERVAL:-900} 秒；速率：${SWEEP_RATE:-1000} pps${SUBNET:+；網段：$SUBNET}

後續：
  1. 系統狀態頁的「觀測代理」應出現本代理（啟動後立即送出第一次心跳）。
  2. 覆蓋網段須與受管網段 CIDR 完全一致，否則顯示「未對應」（見 docs/adr/0019）。
  3. 修改設定：編輯 ${ENV_FILE} 後 systemctl restart asset-nest-agent。
  4. 移除：sudo ./deploy/agent-uninstall.sh（預設保留設定；--purge 一併刪除）。
  5. 跨不可信網路時以 TLS（反向代理）保護代理與後端連線。
EOF
}

# ---- 主流程 -----------------------------------------------------------------

main() {
  parse_args "$@"
  require_root
  require_supported_os
  require_systemd

  resolve_auth_code
  resolve_instance_id

  info "開始安裝 asset-nest 觀測代理。"
  install_base_packages
  if [ -n "$SOURCE_DIR" ]; then
    install_build_packages
    prepare_source
    install_rust
    build_agent
  else
    download_prebuilt
  fi
  install_artifacts
  ensure_service_user
  write_env_file
  write_unit_file
  systemctl daemon-reload
  systemctl enable asset-nest-agent >/dev/null
  systemctl restart asset-nest-agent

  wait_service_active
  verify_server_reachable
  print_summary
}

# 由 bash 執行時才跑安裝：檔案執行（BASH_SOURCE[0] = $0）或 stdin 執行
# （如 curl | bash，BASH_SOURCE 未設定）。source 時只定義函式，不執行安裝。
if [ -z "${BASH_SOURCE[0]:-}" ] || [ "${BASH_SOURCE[0]:-}" = "$0" ]; then
  trap 'warn "安裝失敗（第 $LINENO 行）；請依上方訊息排除。"' ERR
  main "$@"
fi
