#!/usr/bin/env bash
# =============================================================================
# TitanVault — Installer for Linux
# SecuryBlack Storage, Backup and Disaster Recovery Agent
# =============================================================================

set -euo pipefail

SB_AGENT_LABEL="titanvault"
REPO="securyblack/titan-vault"
BIN_NAME="titanvault"
SERVICE_NAME="titanvault"
SERVICE_DESC="TitanVault Backup & Disaster Recovery Agent (SecuryBlack)"

# Descargar librería compartida de sb-agent-core
LIB_URL="https://raw.githubusercontent.com/securyblack/sb-agent-core/main/scripts/install-lib.sh"
LIB_TMP="$(mktemp)"
curl -fsSL "$LIB_URL" -o "$LIB_TMP"
# shellcheck source=/dev/null
source "$LIB_TMP"
rm -f "$LIB_TMP"

sb_require_root
sb_require_cmds curl tar systemctl

TARGET="$(sb_detect_arch_linux)"
sb_info "Detected target architecture: ${TARGET}"

VERSION="${1:-}"
if [[ -z "$VERSION" ]]; then
    VERSION="$(sb_fetch_latest_version "$REPO")"
fi
sb_info "Installing TitanVault version: ${VERSION}"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

ASSET_NAME="${BIN_NAME}-${TARGET}.tar.gz"
ASSET_URL="https://github.com/${REPO}/releases/download/${VERSION}/${ASSET_NAME}"
sb_download_and_verify "$ASSET_URL" "$TMP_DIR/${ASSET_NAME}"
sb_install_binary "$TMP_DIR/${ASSET_NAME}" "$BIN_NAME" "/usr/local/bin"

# Escribir configuración inicial si no existe
CONFIG_DIR="/etc/titanvault"
mkdir -p "$CONFIG_DIR"
if [[ ! -f "$CONFIG_DIR/config.toml" ]]; then
    sb_info "Creating default configuration in $CONFIG_DIR/config.toml..."
    cat > "$CONFIG_DIR/config.toml" << 'EOF'
version = "0.1.0"
agent_name = "titanvault"
mode = "standalone"

[schedule]
enabled = true
cron = "0 2 * * *"
hourly_cron = "0 * * * *"

[retention]
keep_hourly = 24
keep_daily = 7
keep_weekly = 4
keep_monthly = 12
keep_yearly = 3

[crypto]
enabled = false
algorithm = "chacha20-poly1305"

[sources]
databases = []
filesystems = []

[targets]
EOF
    chmod 600 "$CONFIG_DIR/config.toml"
fi

# Instalar y activar servicio systemd
sb_write_systemd_unit "$SERVICE_NAME" "$SERVICE_DESC" "/usr/local/bin/$BIN_NAME"
sb_enable_start_service "$SERVICE_NAME"

sb_success "TitanVault has been successfully installed and started!"
sb_info "To configure backups interactively, launch: titanvault tui"
sb_info "To view live status in terminal: titanvault top"
