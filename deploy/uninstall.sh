#!/usr/bin/env bash
#
# asset-nest 移除腳本（Ubuntu；需 root）
#
# 預設：停用並移除 asset-nest 服務與程式；保留資料庫與設定。
#   --purge       一併刪除資料庫（/var/lib/asset-nest）與設定（/etc/asset-nest）
#   --remove-kea  一併停用並移除本安裝腳本所裝的 Kea 3.2（套件、套件庫、憑證）
#
# 見 README.md「一鍵安裝」、docs/adr/0012。
set -euo pipefail

INSTALL_DIR="/opt/asset-nest"
STATE_DIR="/var/lib/asset-nest"
CONFIG_DIR="/etc/asset-nest"
SERVICE_UNIT="/etc/systemd/system/asset-nest.service"

KEA_KEYRING="/usr/share/keyrings/isc-kea-3-2-archive-keyring.gpg"
KEA_SOURCE_LIST="/etc/apt/sources.list.d/isc-kea-3-2.list"
KEA_USER_FILE="/etc/kea/asset-nest-api.user"
KEA_PASSWORD_FILE="/etc/kea/asset-nest-api.password"

PURGE=0
REMOVE_KEA=0

info() { printf '\033[1;34m[asset-nest]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[asset-nest]\033[0m %s\n' "$*" >&2; }
die() {
  printf '\033[1;31m[asset-nest]\033[0m %s\n' "$*" >&2
  exit 1
}

usage() {
  cat <<'EOF'
用法：sudo ./deploy/uninstall.sh [--purge] [--remove-kea]

  （預設）        移除 asset-nest 服務與程式，保留資料庫與設定
  --purge         一併刪除 /var/lib/asset-nest 與 /etc/asset-nest
  --remove-kea    一併停用並移除 Kea 3.2 套件、ISC 套件庫與控制通道憑證
                  （若 Kea 還被其他服務使用，請勿加此選項）
  -h, --help      顯示本說明
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    --purge) PURGE=1; shift ;;
    --remove-kea) REMOVE_KEA=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "未知選項：$1（--help 看用法）" ;;
  esac
done

[ "$(id -u)" -eq 0 ] || die "請以 root 執行（sudo ./deploy/uninstall.sh）。"

if systemctl list-unit-files asset-nest.service >/dev/null 2>&1; then
  info "停用 asset-nest 服務..."
  systemctl disable --now asset-nest >/dev/null 2>&1 || true
fi
rm -f "$SERVICE_UNIT"
systemctl daemon-reload

info "移除程式（$INSTALL_DIR）..."
rm -rf "$INSTALL_DIR"

if [ "$PURGE" -eq 1 ]; then
  info "移除資料與設定（$STATE_DIR、$CONFIG_DIR）..."
  rm -rf "$STATE_DIR" "$CONFIG_DIR"
else
  warn "已保留資料庫（$STATE_DIR）與設定（$CONFIG_DIR）；若要刪除請加 --purge。"
fi

if [ "$REMOVE_KEA" -eq 1 ]; then
  info "停用並移除 Kea 3.2 ..."
  systemctl disable --now isc-kea-dhcp4-server >/dev/null 2>&1 || true
  env DEBIAN_FRONTEND=noninteractive apt-get purge -y isc-kea-dhcp4 isc-kea-hooks
  rm -f "$KEA_USER_FILE" "$KEA_PASSWORD_FILE" /etc/kea/kea-dhcp4.conf.bak.*
  rm -f "$KEA_SOURCE_LIST" "$KEA_KEYRING"
  env DEBIAN_FRONTEND=noninteractive apt-get update -qq
fi

info "移除完成。"
if [ "$REMOVE_KEA" -eq 0 ]; then
  warn "Kea 未移除；若要一併移除請執行：sudo ./deploy/uninstall.sh --remove-kea"
fi
