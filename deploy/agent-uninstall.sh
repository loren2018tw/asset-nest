#!/usr/bin/env bash
#
# asset-nest 觀測代理移除腳本（Ubuntu；需 root）
#
# 預設：停用並移除服務與程式；保留設定（/etc/asset-nest-agent）。
#   --purge  一併刪除設定（/etc/asset-nest-agent）
#
# 見 README.md「觀測代理」、docs/adr/0018。
set -euo pipefail

INSTALL_DIR="/opt/asset-nest-agent"
CONFIG_DIR="/etc/asset-nest-agent"
SERVICE_UNIT="/etc/systemd/system/asset-nest-agent.service"

PURGE=0

info() { printf '\033[1;34m[asset-nest-agent]\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m[asset-nest-agent]\033[0m %s\n' "$*" >&2; }
die() {
  printf '\033[1;31m[asset-nest-agent]\033[0m %s\n' "$*" >&2
  exit 1
}

usage() {
  cat <<'EOF'
用法：sudo ./deploy/agent-uninstall.sh [--purge]

  （預設）        移除 asset-nest-agent 服務與程式，保留設定
  --purge         一併刪除 /etc/asset-nest-agent
  -h, --help      顯示本說明
EOF
}

while [ $# -gt 0 ]; do
  case "$1" in
    --purge) PURGE=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) die "未知選項：$1（--help 看用法）" ;;
  esac
done

[ "$(id -u)" -eq 0 ] || die "請以 root 執行（sudo ./deploy/agent-uninstall.sh）。"

if systemctl list-unit-files asset-nest-agent.service >/dev/null 2>&1; then
  info "停用 asset-nest-agent 服務..."
  systemctl disable --now asset-nest-agent >/dev/null 2>&1 || true
fi
rm -f "$SERVICE_UNIT"
systemctl daemon-reload

info "移除程式（$INSTALL_DIR）..."
rm -rf "$INSTALL_DIR"

if [ "$PURGE" -eq 1 ]; then
  info "移除設定（$CONFIG_DIR）..."
  rm -rf "$CONFIG_DIR"
else
  warn "已保留設定（$CONFIG_DIR）；若要刪除請加 --purge。"
fi

info "移除完成。"
