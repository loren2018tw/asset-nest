#!/usr/bin/env bash
#
# asset-nest 一鍵安裝腳本（全新 Ubuntu 24.04／26.04）
#
# 安裝內容：
#   - nginx 前門（反向代理至 127.0.0.1:8080；--no-nginx 可跳過）
#   - ISC Kea DHCP 3.2（Cloudsmith 套件庫）與 host_cmds、lease_cmds、
#     subnet_cmds hook
#   - asset-nest（自原始碼建置：Node.js 24、pnpm、Rust stable）
#   - systemd 服務：asset-nest、isc-kea-dhcp4-server
#
# 決策見 docs/adr/0012、docs/adr/0013、docs/adr/0022；使用說明見 README.md。
# 可直接 source 本檔取用函式做測試（不會執行安裝）。
set -euo pipefail

# ---- 常數／預設值 -----------------------------------------------------------

REPO_URL="https://github.com/loren2018tw/asset-nest.git"
REF="main"
SOURCE_DIR=""

INSTALL_DIR="/opt/asset-nest"
STATE_DIR="/var/lib/asset-nest"
CONFIG_DIR="/etc/asset-nest"
ENV_FILE="$CONFIG_DIR/asset-nest.env"
SERVICE_USER="asset-nest"
SERVICE_UNIT="/etc/systemd/system/asset-nest.service"
NGINX_SITE_AVAILABLE="/etc/nginx/sites-available/asset-nest.conf"
NGINX_SITE_ENABLED="/etc/nginx/sites-enabled/asset-nest.conf"

# 未明示 --bind 時留空，由 parse_args 依 nginx／--no-nginx 決定（見 spec §2）。
BIND_ADDR=""

WITH_NGINX=1
FORCE_NGINX_CONF=0
WITH_KEA=1
KEA_PORT="8000"
KEA_USERNAME=""
KEA_URL=""
KEA_PASSWORD_ARG=""
KEA_INTERFACES_RAW=""
KEA_SUBNET=""
FORCE_KEA_CONFIG=0
KEA_PASSWORD=""
AGENT_AUTH_CODE=""
AUTH_USERNAME=""
AUTH_PASSWORD=""
KEA_HOOK_PATH=""
KEA_LEASE_HOOK_PATH=""
KEA_SUBNET_HOOK_PATH=""

KEA_KEYRING="/usr/share/keyrings/isc-kea-3-2-archive-keyring.gpg"
KEA_SOURCE_LIST="/etc/apt/sources.list.d/isc-kea-3-2.list"
KEA_USER_FILE="/etc/kea/asset-nest-api.user"
KEA_PASSWORD_FILE="/etc/kea/asset-nest-api.password"
KEA_CONF="/etc/kea/kea-dhcp4.conf"

# ---- 輸出工具 ---------------------------------------------------------------

info() { printf '\033[1;34m[asset-nest]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[asset-nest]\033[0m %s\n' "$*" >&2; }
die() {
  printf '\033[1;31m[asset-nest]\033[0m %s\n' "$*" >&2
  exit 1
}

# ---- 用法 -------------------------------------------------------------------

usage() {
  cat <<'EOF'
用法：sudo ./deploy/install.sh [選項]

在全新 Ubuntu 24.04／26.04 上安裝 Kea DHCP 3.2、asset-nest 與 nginx 前門
（反向代理；asset-nest 預設僅綁 127.0.0.1），並建立 systemd 服務
（asset-nest、isc-kea-dhcp4-server）。可重複執行（重跑即更新；預設保留既有
Kea 設定、控制通道憑證與 nginx 站台檔）。

一般選項：
  --ref <ref>               asset-nest 原始碼 git ref（預設 main）
  --source-dir <path>       使用既有 checkout（不 clone；CI／開發機用）
  --bind <addr:port>        asset-nest 綁定位址（預設 127.0.0.1:8080；
                            --no-nginx 時 0.0.0.0:8080）
  -h, --help                顯示本說明

nginx 前門選項：
  --no-nginx                不安裝／不設定 nginx；asset-nest 維持直接對外（舊行為）
  --force-nginx-conf        覆寫既有 nginx 站台檔（預設保留，避免蓋掉日後
                            certbot 的修改）

Kea 選項：
  --no-kea                  不安裝 Kea；可搭配 --kea-url 指向既有 Kea
  --kea-url <url>           既有 Kea 控制通道 URL（需搭配 --no-kea）
  --kea-username <user>     控制通道使用者（預設 asset-nest）
  --kea-password <password> 既有 Kea 的密碼（需搭配 --kea-url）
  --kea-interfaces <list>   監聽介面，逗號分隔（預設空＝不監聽；* 表全部）
  --kea-subnet <CIDR>       寫入單一 Kea 網段（id 1；供測試或單網段環境）
  --kea-port <port>         控制通道埠（預設 8000，綁 127.0.0.1）
  --force-kea-config        備份後覆寫既有 Kea 設定並重新產生憑證

安裝後位置：
  /opt/asset-nest                    程式與前端產物
  /etc/asset-nest/asset-nest.env     環境設定（含 Kea 憑證）
  /var/lib/asset-nest/asset-nest.db  資料庫
  /etc/nginx/sites-available/asset-nest.conf  nginx 站台（反向代理）
  /etc/kea/asset-nest-api.{user,password}  Kea 控制通道憑證
EOF
}

# ---- 小工具 -----------------------------------------------------------------

version_ge() {
  # version_ge <actual> <required>
  [ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -n1)" = "$2" ]
}

generate_password() {
  if command -v openssl >/dev/null 2>&1; then
    openssl rand -hex 16
  else
    od -An -N16 -tx1 /dev/urandom | tr -d ' \n'
  fi
}

apt_install() {
  env DEBIAN_FRONTEND=noninteractive apt-get install -y "$@"
}

# ---- 前置檢查 ---------------------------------------------------------------

require_root() {
  [ "$(id -u)" -eq 0 ] || die "請以 root 執行（sudo ./deploy/install.sh）。"
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
      --ref) REF="${2:?--ref 需要值}"; shift 2 ;;
      --source-dir) SOURCE_DIR="${2:?--source-dir 需要值}"; shift 2 ;;
      --bind) BIND_ADDR="${2:?--bind 需要值}"; shift 2 ;;
      --no-kea) WITH_KEA=0; shift ;;
      --no-nginx) WITH_NGINX=0; shift ;;
      --force-nginx-conf) FORCE_NGINX_CONF=1; shift ;;
      --kea-url) KEA_URL="${2:?--kea-url 需要值}"; shift 2 ;;
      --kea-username) KEA_USERNAME="${2:?--kea-username 需要值}"; shift 2 ;;
      --kea-password) KEA_PASSWORD_ARG="${2:?--kea-password 需要值}"; shift 2 ;;
      --kea-interfaces) KEA_INTERFACES_RAW="${2:?--kea-interfaces 需要值}"; shift 2 ;;
      --kea-subnet) KEA_SUBNET="${2:?--kea-subnet 需要值}"; shift 2 ;;
      --kea-port) KEA_PORT="${2:?--kea-port 需要值}"; shift 2 ;;
      --force-kea-config) FORCE_KEA_CONFIG=1; shift ;;
      -h|--help) usage; exit 0 ;;
      *) die "未知選項：$1（--help 看用法）" ;;
    esac
  done

  # BIND 預設（見 docs/adr/0022、spec §2）：明示 --bind 一律優先；未明示時
  # 預設安裝（nginx 前門）綁 loopback，--no-nginx（直接對外）維持舊行為。
  if [ -z "$BIND_ADDR" ]; then
    if [ "$WITH_NGINX" -eq 1 ]; then
      BIND_ADDR="127.0.0.1:8080"
    else
      BIND_ADDR="0.0.0.0:8080"
    fi
  fi
  validate_args
}

validate_args() {
  case "$BIND_ADDR" in *:*) ;; *) die "--bind 需為 addr:port 形式（目前：$BIND_ADDR）。" ;; esac
  case "$KEA_PORT" in ''|*[!0-9]*) die "--kea-port 需為數字（目前：$KEA_PORT）。" ;; esac

  if [ "$WITH_NGINX" -eq 0 ] && [ "$FORCE_NGINX_CONF" -eq 1 ]; then
    warn "--force-nginx-conf 在 --no-nginx 模式無作用。"
    FORCE_NGINX_CONF=0
  fi

  if [ -n "$SOURCE_DIR" ]; then
    [ -f "$SOURCE_DIR/package.json" ] || die "--source-dir 內找不到 package.json：$SOURCE_DIR"
  fi

  if [ -n "$KEA_SUBNET" ]; then
    printf '%s' "$KEA_SUBNET" | grep -qE '^[0-9a-fA-F:.]+/[0-9]{1,3}$' \
      || die "--kea-subnet 需為 CIDR 形式（目前：$KEA_SUBNET）。"
  fi

  if [ "$WITH_KEA" -eq 1 ]; then
    if [ -n "$KEA_URL" ]; then
      die "--kea-url 需搭配 --no-kea（本腳本同機安裝 Kea）。"
    fi
    if [ -n "$KEA_PASSWORD_ARG" ]; then
      die "--kea-password 僅供遠端 Kea（--no-kea + --kea-url）；同機安裝由腳本產生憑證。"
    fi
  else
    if [ -n "$KEA_PASSWORD_ARG" ] && [ -z "$KEA_URL" ]; then
      die "--kea-password 需搭配 --kea-url。"
    fi
    if [ -z "$KEA_URL" ]; then
      warn "未安裝 Kea 且未指定 --kea-url：Kea 整合停用。"
    fi
    if [ "$FORCE_KEA_CONFIG" -eq 1 ]; then
      warn "--force-kea-config 在 --no-kea 模式無作用。"
      FORCE_KEA_CONFIG=0
    fi
  fi
}

# ---- 系統套件與工具鏈 -------------------------------------------------------

install_base_packages() {
  info "安裝基本套件（ca-certificates、curl、git、gnupg、build-essential）..."
  env DEBIAN_FRONTEND=noninteractive apt-get update -qq
  apt_install ca-certificates curl git gnupg build-essential
}

install_node() {
  local node_version=""
  if command -v node >/dev/null 2>&1; then
    node_version="$(node -v | sed 's/^v//')"
  fi
  if [ -n "$node_version" ] && version_ge "$node_version" "24.0.0"; then
    info "已安裝 Node.js $node_version"
    return
  fi

  info "安裝 Node.js 24（NodeSource 套件庫）..."
  install -d -m 0755 /etc/apt/keyrings
  curl -fsSL https://deb.nodesource.com/gpgkey/nodesource-repo.gpg.key \
    | gpg --dearmor >/etc/apt/keyrings/nodesource.gpg
  chmod 0644 /etc/apt/keyrings/nodesource.gpg
  printf 'deb [signed-by=/etc/apt/keyrings/nodesource.gpg] https://deb.nodesource.com/node_24.x nodistro main\n' \
    >/etc/apt/sources.list.d/nodesource.list
  env DEBIAN_FRONTEND=noninteractive apt-get update -qq
  apt_install nodejs
}

prepare_source() {
  SRC_DIR=""
  if [ -n "$SOURCE_DIR" ]; then
    SRC_DIR="$(cd "$SOURCE_DIR" && pwd)"
    info "使用現有原始碼：$SRC_DIR"
    return
  fi

  if [ -d "$INSTALL_DIR/src/.git" ]; then
    info "更新原始碼（$REF）..."
    git -C "$INSTALL_DIR/src" remote set-url origin "$REPO_URL"
    git -C "$INSTALL_DIR/src" fetch --depth 1 origin "$REF"
    git -C "$INSTALL_DIR/src" checkout -f FETCH_HEAD
  else
    info "取得原始碼（$REF）..."
    install -d -m 0755 "$INSTALL_DIR"
    git clone --depth 1 --branch "$REF" "$REPO_URL" "$INSTALL_DIR/src"
  fi
  SRC_DIR="$INSTALL_DIR/src"
}

install_pnpm() {
  local want
  want="$(node -p "require('${SRC_DIR}/package.json').packageManager" | sed 's/^pnpm@//')"
  case "$want" in
    ""|undefined|null) die "package.json 缺少 packageManager（pnpm 版本）。" ;;
  esac

  if command -v pnpm >/dev/null 2>&1 && [ "$(pnpm --version)" = "$want" ]; then
    info "已安裝 pnpm $want"
    return
  fi

  info "安裝 pnpm $want ..."
  npm install -g "pnpm@${want}"
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

build_asset_nest() {
  info "pnpm install（frozen-lockfile）..."
  (cd "$SRC_DIR" && pnpm install --frozen-lockfile)

  info "pnpm build（前端 + 後端 release）..."
  (cd "$SRC_DIR" && pnpm build)

  BIN_PATH="${CARGO_TARGET_DIR:-$SRC_DIR/backend/target}/release/asset-nest"
  DIST_PATH="$SRC_DIR/frontend/dist/spa"
  [ -x "$BIN_PATH" ] || die "找不到後端建置產物：$BIN_PATH"
  [ -f "$DIST_PATH/index.html" ] || die "找不到前端建置產物：$DIST_PATH"
}

install_artifacts() {
  info "安裝程式至 $INSTALL_DIR ..."
  install -d -m 0755 "$INSTALL_DIR"
  install -m 0755 "$BIN_PATH" "$INSTALL_DIR/asset-nest"
  rm -rf "$INSTALL_DIR/web"
  cp -a "$DIST_PATH" "$INSTALL_DIR/web"
  chown -R root:root "$INSTALL_DIR"
}

ensure_service_user() {
  if ! id -u "$SERVICE_USER" >/dev/null 2>&1; then
    info "建立系統使用者 ${SERVICE_USER} ..."
    adduser --system --group --quiet --no-create-home --home "$STATE_DIR" \
      --shell /usr/sbin/nologin --disabled-password --disabled-login "$SERVICE_USER"
  fi
  install -d -m 0750 -o "$SERVICE_USER" -g "$SERVICE_USER" "$STATE_DIR"
}

# ---- Kea --------------------------------------------------------------------

install_kea_packages() {
  local codename
  codename="$(. /etc/os-release && printf '%s' "${VERSION_CODENAME:-}")"
  [ -n "$codename" ] || die "無法由 /etc/os-release 取得 VERSION_CODENAME。"

  info "設定 ISC Kea 3.2 套件庫（$codename）..."
  curl -fsSL "https://dl.cloudsmith.io/public/isc/kea-3-2/gpg.720DD962343B440F.key" \
    | gpg --dearmor >"$KEA_KEYRING"
  chmod 0644 "$KEA_KEYRING"
  printf 'deb [signed-by=%s] https://dl.cloudsmith.io/public/isc/kea-3-2/deb/ubuntu %s main\n' \
    "$KEA_KEYRING" "$codename" >"$KEA_SOURCE_LIST"

  info "安裝 isc-kea-dhcp4 與 isc-kea-hooks ..."
  env DEBIAN_FRONTEND=noninteractive apt-get update -qq
  apt_install isc-kea-dhcp4 isc-kea-hooks
}

configure_kea_credentials() {
  if [ -f "$KEA_PASSWORD_FILE" ] && [ "$FORCE_KEA_CONFIG" -eq 0 ]; then
    KEA_PASSWORD="$(cat "$KEA_PASSWORD_FILE")"
    info "沿用既有 Kea 控制通道憑證。"
  else
    KEA_PASSWORD="$(generate_password)"
  fi

  printf '%s' "${KEA_USERNAME:-asset-nest}" >"$KEA_USER_FILE"
  printf '%s' "$KEA_PASSWORD" >"$KEA_PASSWORD_FILE"
  chown root:_kea "$KEA_USER_FILE" "$KEA_PASSWORD_FILE"
  chmod 0640 "$KEA_USER_FILE" "$KEA_PASSWORD_FILE"
}

render_kea_config() {
  local interfaces_json="[]" subnet_json="[]" part parts
  if [ -n "$KEA_INTERFACES_RAW" ]; then
    IFS=',' read -r -a parts <<<"$KEA_INTERFACES_RAW"
    interfaces_json="["
    for part in "${parts[@]}"; do
      part="${part#"${part%%[![:space:]]*}"}"
      part="${part%"${part##*[![:space:]]}"}"
      [ -n "$part" ] || die "--kea-interfaces 含空白項目：$KEA_INTERFACES_RAW"
      interfaces_json+="\"${part}\", "
    done
    interfaces_json="${interfaces_json%, }]"
  fi

  if [ -n "$KEA_SUBNET" ]; then
    subnet_json="[ { \"id\": 1, \"subnet\": \"${KEA_SUBNET}\" } ]"
  fi

  cat <<EOF
// 本檔由 asset-nest 安裝腳本產生（deploy/install.sh）；見 docs/adr/0012。
// Kea 的 config-write 會以執行中設定改寫本檔（見 docs/adr/0011）。
{
  "Dhcp4": {
    "interfaces-config": {
      "interfaces": ${interfaces_json}
    },
    // 最上層預設 DNS（domain-name-servers）；未另行覆寫的網段皆適用。
    "option-data": [
      {
        "name": "domain-name-servers",
        "code": 6,
        "space": "dhcp4",
        "data": "8.8.8.8"
      }
    ],
    "control-sockets": [
      {
        "socket-type": "http",
        "socket-address": "127.0.0.1",
        "socket-port": ${KEA_PORT},
        "authentication": {
          "type": "basic",
          "realm": "kea-dhcp4",
          "clients": [
            {
              "user-file": "${KEA_USER_FILE}",
              "password-file": "${KEA_PASSWORD_FILE}"
            }
          ]
        }
      }
    ],
    "hooks-libraries": [
      { "library": "${KEA_HOOK_PATH}" },
      { "library": "${KEA_LEASE_HOOK_PATH}" },
      { "library": "${KEA_SUBNET_HOOK_PATH}" }
    ],
    "lease-database": {
      "type": "memfile",
      "lfc-interval": 3600
    },
    "subnet4": ${subnet_json},
    "loggers": [
      {
        "name": "kea-dhcp4",
        "output-options": [ { "output": "stdout" } ],
        "severity": "INFO",
        "debuglevel": 0
      }
    ]
  }
}
EOF
}

configure_kea() {
  local hook lease_hook subnet_hook check_log backup
  hook=""
  for f in /usr/lib/*/kea/hooks/libdhcp_host_cmds.so; do
    if [ -f "$f" ]; then
      hook="$f"
      break
    fi
  done
  [ -n "$hook" ] || die "找不到 host_cmds hook（isc-kea-hooks 是否安裝成功？）。"
  KEA_HOOK_PATH="$hook"

  lease_hook=""
  for f in /usr/lib/*/kea/hooks/libdhcp_lease_cmds.so; do
    if [ -f "$f" ]; then
      lease_hook="$f"
      break
    fi
  done
  [ -n "$lease_hook" ] || die "找不到 lease_cmds hook（isc-kea-hooks 是否安裝成功？）。"
  KEA_LEASE_HOOK_PATH="$lease_hook"

  subnet_hook=""
  for f in /usr/lib/*/kea/hooks/libdhcp_subnet_cmds.so; do
    if [ -f "$f" ]; then
      subnet_hook="$f"
      break
    fi
  done
  [ -n "$subnet_hook" ] || die "找不到 subnet_cmds hook（isc-kea-hooks 是否安裝成功？）。"
  KEA_SUBNET_HOOK_PATH="$subnet_hook"

  if [ -f "$KEA_CONF" ] && grep -q 'asset-nest-api.user' "$KEA_CONF" \
    && [ "$FORCE_KEA_CONFIG" -eq 0 ]; then
    info "保留既有 Kea 設定（$KEA_CONF）。"
    if ! grep -q 'libdhcp_lease_cmds' "$KEA_CONF"; then
      warn "既有 Kea 設定未載入 lease_cmds hook（libdhcp_lease_cmds.so）：租約清單需此 hook，可手動加入 hooks-libraries 或改用 --force-kea-config 重新產生。"
    fi
    if ! grep -q 'libdhcp_subnet_cmds' "$KEA_CONF"; then
      warn "既有 Kea 設定未載入 subnet_cmds hook（libdhcp_subnet_cmds.so）：網段層同步（位址池與 gateway）需此 hook，可手動加入 hooks-libraries 或改用 --force-kea-config 重新產生。"
    fi
    return
  fi

  if [ -f "$KEA_CONF" ]; then
    backup="${KEA_CONF}.bak.$(date +%Y%m%d%H%M%S)"
    cp -a "$KEA_CONF" "$backup"
    warn "已備份既有 Kea 設定：$backup"
  fi

  info "寫入 Kea 設定（$KEA_CONF）..."
  render_kea_config >"$KEA_CONF"
  chown _kea:_kea "$KEA_CONF"
  chmod 0640 "$KEA_CONF"

  check_log="$(mktemp)"
  if ! kea-dhcp4 -T "$KEA_CONF" >"$check_log" 2>&1; then
    cat "$check_log" >&2
    rm -f "$check_log"
    die "Kea 設定驗證失敗：$KEA_CONF"
  fi
  rm -f "$check_log"
}

# ---- nginx 前門 -------------------------------------------------------------

install_nginx() {
  info "安裝 nginx ..."
  apt_install nginx
  # apt 通常已自動啟動 nginx；--now 確保未自動啟動的環境（如部分容器化 CI）
  # 也已啟動，configure_nginx 才能 reload。
  systemctl enable --now nginx >/dev/null
}

check_port_80() {
  # 80 埠若已被非 nginx 程序佔用，本站台無法掛 default_server；安裝前明確
  # 失敗並提示 --no-nginx（見 spec §2）。
  if ! command -v ss >/dev/null 2>&1; then
    warn "找不到 ss（iproute2），略過 80 埠佔用檢查。"
    return 0
  fi

  local listeners
  listeners="$(ss -ltnpH 'sport = :80' 2>/dev/null || true)"
  [ -n "$listeners" ] || return 0

  if printf '%s' "$listeners" | grep -qv 'users:(("nginx"'; then
    printf '%s\n' "$listeners" >&2
    die "80 埠已被非 nginx 程序佔用；請先停用該服務，或改用 --no-nginx（維持直接對外綁定）。"
  fi
  info "80 埠已由既有 nginx 使用，續行設定站台。"
}

render_nginx_site() {
  # 站台 template（見 spec §3、docs/adr/0022）：全站反向代理至 loopback；
  # XFF 以 $remote_addr 覆寫（後端信任第一段）。
  cat <<'EOF'
# asset-nest 反向代理（由 deploy/install.sh 產生；見 docs/adr/0022）
server {
    listen 80 default_server;
    listen [::]:80 default_server;
    server_name _;

    # HTTPS 尚未設定；日後以 certbot（Let's Encrypt）於本站台啟用。
    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        # 覆寫（非 $proxy_add_x_forwarded_for）：避免客戶端自帶值被視為第一段
        proxy_set_header X-Forwarded-For $remote_addr;
    }

    client_max_body_size 16m;  # CSV 匯入
}
EOF
}

configure_nginx() {
  if [ -f "$NGINX_SITE_AVAILABLE" ] && [ "$FORCE_NGINX_CONF" -eq 0 ]; then
    info "保留既有 nginx 站台檔（$NGINX_SITE_AVAILABLE）。"
  else
    info "寫入 nginx 站台檔（$NGINX_SITE_AVAILABLE）..."
    render_nginx_site >"$NGINX_SITE_AVAILABLE"
    chmod 0644 "$NGINX_SITE_AVAILABLE"
  fi

  ln -sf "$NGINX_SITE_AVAILABLE" "$NGINX_SITE_ENABLED"

  # 原廠 default 站台同為 default_server，會與本站台衝突；僅當仍是原廠連結
  # 時移除，不動其他既有站台。
  if [ -L /etc/nginx/sites-enabled/default ] \
    && [ "$(readlink -f /etc/nginx/sites-enabled/default)" = "/etc/nginx/sites-available/default" ]; then
    info "移除原廠 nginx default 站台連結（/etc/nginx/sites-enabled/default）..."
    rm -f /etc/nginx/sites-enabled/default
  fi

  local check_log
  check_log="$(mktemp)"
  if ! nginx -t >"$check_log" 2>&1; then
    cat "$check_log" >&2
    rm -f "$check_log"
    die "nginx 設定驗證失敗；請檢查 $NGINX_SITE_AVAILABLE。"
  fi
  rm -f "$check_log"

  info "重新載入 nginx ..."
  systemctl reload nginx
}

# ---- asset-nest 服務 --------------------------------------------------------

configure_agent_auth_code() {
  # 觀測代理共用認證碼（見 docs/adr/0019）：首次隨機產生、重跑沿用既有值。
  if [ -f "$ENV_FILE" ]; then
    AGENT_AUTH_CODE="$(grep -m1 '^AGENT_AUTH_CODE=' "$ENV_FILE" | cut -d= -f2- || true)"
  fi
  if [ -n "$AGENT_AUTH_CODE" ]; then
    info "沿用既有代理認證碼（$ENV_FILE）。"
  else
    AGENT_AUTH_CODE="$(generate_password)"
    info "產生代理認證碼（AGENT_AUTH_CODE）。"
  fi
}

configure_auth_credentials() {
  # 登入帳密（見 docs/adr/0021）：重跑一律沿用既有值；缺漏時帳號 admin、
  # 密碼以 generate_password 隨機產生。訊息與摘要不得含密碼值。
  local env_username="" env_password=""
  if [ -f "$ENV_FILE" ]; then
    env_username="$(grep -m1 '^AUTH_USERNAME=' "$ENV_FILE" | cut -d= -f2- || true)"
    env_password="$(grep -m1 '^AUTH_PASSWORD=' "$ENV_FILE" | cut -d= -f2- || true)"
  fi

  if [ -n "$env_username" ]; then
    AUTH_USERNAME="$env_username"
    info "沿用既有登入帳號（$ENV_FILE）。"
  else
    AUTH_USERNAME="admin"
  fi

  if [ -n "$env_password" ]; then
    AUTH_PASSWORD="$env_password"
    info "沿用既有登入密碼（$ENV_FILE）。"
  else
    AUTH_PASSWORD="$(generate_password)"
    info "已產生登入密碼（AUTH_PASSWORD）。"
  fi
}

write_env_file() {
  install -d -m 0750 -o root -g "$SERVICE_USER" "$CONFIG_DIR"
  info "寫入環境設定（$ENV_FILE）..."
  {
    printf '# 由 deploy/install.sh 產生（%s）\n' "$(date -Iseconds)"
    printf 'BIND_ADDR=%s\n' "$BIND_ADDR"
    printf 'DATABASE_URL=sqlite://%s/asset-nest.db\n' "$STATE_DIR"
    printf 'WEB_DIST_DIR=%s/web\n' "$INSTALL_DIR"
    printf '# 代理入庫認證碼（見 docs/adr/0019；代理安裝時輸入同一組）\n'
    printf 'AGENT_AUTH_CODE=%s\n' "$AGENT_AUTH_CODE"
    printf '# 登入帳密（見 docs/adr/0021、.env.example；變更即所有裝置登出）\n'
    printf 'AUTH_USERNAME=%s\n' "$AUTH_USERNAME"
    printf 'AUTH_PASSWORD=%s\n' "$AUTH_PASSWORD"
    if [ "$WITH_KEA" -eq 1 ]; then
      printf 'KEA_API_URL=http://127.0.0.1:%s\n' "$KEA_PORT"
      printf 'KEA_API_USERNAME=%s\n' "${KEA_USERNAME:-asset-nest}"
      printf 'KEA_API_PASSWORD=%s\n' "$KEA_PASSWORD"
    elif [ -n "$KEA_URL" ]; then
      printf 'KEA_API_URL=%s\n' "$KEA_URL"
      if [ -n "$KEA_USERNAME" ]; then
        printf 'KEA_API_USERNAME=%s\n' "$KEA_USERNAME"
      fi
      if [ -n "$KEA_PASSWORD_ARG" ]; then
        printf 'KEA_API_PASSWORD=%s\n' "$KEA_PASSWORD_ARG"
      fi
    fi
  } >"$ENV_FILE"
  chown root:"$SERVICE_USER" "$ENV_FILE"
  chmod 0640 "$ENV_FILE"
}

write_unit_file() {
  info "建立 systemd 服務（asset-nest.service）..."
  cat >"$SERVICE_UNIT" <<EOF
[Unit]
Description=asset-nest IT 資產整合管理系統
Documentation=https://github.com/loren2018tw/asset-nest
Wants=network-online.target
After=network-online.target

[Service]
Type=simple
User=${SERVICE_USER}
Group=${SERVICE_USER}
WorkingDirectory=${INSTALL_DIR}
EnvironmentFile=${ENV_FILE}
ExecStart=${INSTALL_DIR}/asset-nest
Restart=on-failure
RestartSec=3
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ProtectKernelTunables=true
ProtectControlGroups=true
RestrictSUIDSGID=true
ReadWritePaths=${STATE_DIR}

[Install]
WantedBy=multi-user.target
EOF
  chmod 0644 "$SERVICE_UNIT"
}

# ---- 驗證與收尾 -------------------------------------------------------------

wait_health() {
  local host port attempt
  host="${BIND_ADDR%:*}"
  port="${BIND_ADDR##*:}"
  case "$host" in 0.0.0.0|::|"") host="127.0.0.1" ;; esac

  info "等待 asset-nest 健康檢查（http://${host}:${port}/api/health）..."
  for attempt in $(seq 1 30); do
    if curl -fsS "http://${host}:${port}/api/health" >/dev/null 2>&1; then
      info "asset-nest 健康檢查通過。"
      return
    fi
    sleep 1
  done
  die "asset-nest 健康檢查逾時；請執行：journalctl -u asset-nest"
}

verify_nginx() {
  [ "$WITH_NGINX" -eq 1 ] || return 0
  info "驗證 nginx 前門（http://127.0.0.1/api/health）..."
  curl -fsS http://127.0.0.1/api/health >/dev/null \
    || die "nginx 前門驗證失敗；請檢查 systemctl status nginx 與 nginx -t。"
  info "nginx 前門驗證通過。"
}

verify_kea() {
  [ "$WITH_KEA" -eq 1 ] || return 0
  info "驗證 Kea 控制通道（version-get）..."
  curl -fsS -u "${KEA_USERNAME:-asset-nest}:${KEA_PASSWORD}" \
    -X POST "http://127.0.0.1:${KEA_PORT}/" \
    -H 'Content-Type: application/json' \
    -d '{"command":"version-get"}' >/dev/null \
    || die "Kea 控制通道驗證失敗；請執行：journalctl -u isc-kea-dhcp4-server"
  info "Kea 控制通道驗證通過。"
}

print_summary() {
  local port web_url proxy_url firewall_hint
  port="${BIND_ADDR##*:}"
  if [ "$WITH_NGINX" -eq 1 ]; then
    web_url="http://<主機>/（nginx → 127.0.0.1:8080）"
    proxy_url="http://<主機>"
    firewall_hint="以防火牆限制 80 來源（或僅允許區域網路）。"
  else
    web_url="http://<主機>:${port}（BIND_ADDR=${BIND_ADDR}）"
    proxy_url="http://<主機>:${port}"
    firewall_hint="以防火牆限制 ${port} 來源。"
  fi

  cat <<EOF

安裝完成。

  asset-nest：${web_url}
  資料庫：${STATE_DIR}/asset-nest.db
  環境檔：${ENV_FILE}
  登入帳密：grep '^AUTH_' ${ENV_FILE}
  代理認證碼：grep AGENT_AUTH_CODE ${ENV_FILE}
  安裝觀測代理（於目標網段主機執行）：
    sudo ./deploy/agent-install.sh --server-url ${proxy_url} --auth-code <認證碼>
EOF
  if [ "$WITH_NGINX" -eq 1 ]; then
    cat <<EOF
  HTTPS：尚未設定；日後以 certbot（Let's Encrypt）於 nginx 設定。
EOF
  fi
  if [ "$WITH_KEA" -eq 1 ]; then
    cat <<EOF
  Kea：isc-kea-dhcp4-server（控制通道 127.0.0.1:${KEA_PORT}）
  Kea 憑證：${KEA_USER_FILE}、${KEA_PASSWORD_FILE}

後續：
  1. ${firewall_hint}
  2. 於 ${KEA_CONF} 設定 interfaces-config 與 subnet4 後：
     systemctl restart isc-kea-dhcp4-server
  3. 在本系統建立對應網段並填入 Kea subnet id（kea_subnet_id）；保留、位址池
     與 gateway 由「Kea 同步」對齊（見 docs/adr/0011、docs/adr/0013）。
EOF
  fi
}

# ---- 主流程 -----------------------------------------------------------------

main() {
  parse_args "$@"
  require_root
  require_supported_os
  require_systemd

  info "開始安裝 asset-nest$([ "$WITH_NGINX" -eq 1 ] && printf '（nginx 前門）' || true)$([ "$WITH_KEA" -eq 1 ] && printf ' 與 Kea DHCP 3.2' || true)。"
  install_base_packages

  if [ "$WITH_NGINX" -eq 1 ]; then
    check_port_80
    install_nginx
    configure_nginx
  fi

  install_node
  prepare_source
  install_pnpm
  install_rust
  build_asset_nest
  install_artifacts
  ensure_service_user

  if [ "$WITH_KEA" -eq 1 ]; then
    install_kea_packages
    configure_kea_credentials
    configure_kea
    systemctl daemon-reload
    systemctl enable isc-kea-dhcp4-server >/dev/null
    systemctl restart isc-kea-dhcp4-server
  fi

  configure_agent_auth_code
  configure_auth_credentials
  write_env_file
  write_unit_file
  systemctl daemon-reload
  systemctl enable asset-nest >/dev/null
  systemctl restart asset-nest

  wait_health
  verify_nginx
  verify_kea
  print_summary
}

# 由 bash 執行時才跑安裝：檔案執行（BASH_SOURCE[0] = $0）或 stdin 執行
# （如 curl | bash，BASH_SOURCE 未設定）。source 時只定義函式，不執行安裝。
if [ -z "${BASH_SOURCE[0]:-}" ] || [ "${BASH_SOURCE[0]:-}" = "$0" ]; then
  trap 'warn "安裝失敗（第 $LINENO 行）；請依上方訊息排除。"' ERR
  main "$@"
fi
