#!/usr/bin/env bash
# =============================================================================
# install.sh — Install monad-monitoring as a systemd service
#
# Usage:
#   sudo ./install.sh [--binary /path/to/monad-monitoring] [--uninstall]
#
# What it does:
#   1. Creates a dedicated 'monad' system user (if not already present).
#   2. Installs the binary to /usr/local/bin/monad-monitoring.
#   3. Creates /opt/monad-node-monitor and writes a .env template (if missing).
#   4. Installs monad-monitoring.service into systemd.
#   5. Enables and starts the service.
#
# To remove:  sudo ./install.sh --uninstall
# =============================================================================
set -euo pipefail

# ---------- defaults --------------------------------------------------------
BINARY_SRC=""          # resolved below if empty
INSTALL_DIR="/opt/monad-node-monitor"
BIN_DEST="/usr/local/bin/monad-monitoring"
SERVICE_NAME="monad-monitoring"
SERVICE_FILE="monad-monitoring.service"
RUN_USER="monad"

# ---------- colours ---------------------------------------------------------
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
info()  { echo -e "${GREEN}[install]${NC} $*"; }
warn()  { echo -e "${YELLOW}[warn]${NC}   $*"; }
error() { echo -e "${RED}[error]${NC}  $*" >&2; exit 1; }

# ---------- root check ------------------------------------------------------
[[ $EUID -ne 0 ]] && error "Run this script as root (sudo ./install.sh)"

# ---------- argument parsing ------------------------------------------------
UNINSTALL=false
while [[ $# -gt 0 ]]; do
    case "$1" in
        --binary)    BINARY_SRC="$2"; shift 2 ;;
        --uninstall) UNINSTALL=true; shift ;;
        *) error "Unknown argument: $1" ;;
    esac
done

# ---------- uninstall path --------------------------------------------------
if $UNINSTALL; then
    info "Stopping and disabling ${SERVICE_NAME}..."
    systemctl stop  "${SERVICE_NAME}" 2>/dev/null || true
    systemctl disable "${SERVICE_NAME}" 2>/dev/null || true
    rm -f "/etc/systemd/system/${SERVICE_FILE}"
    systemctl daemon-reload
    rm -f "${BIN_DEST}"
    warn "Data directory ${INSTALL_DIR} was NOT removed (contains .env and state files)."
    warn "Remove it manually with:  rm -rf ${INSTALL_DIR}"
    info "Uninstall complete."
    exit 0
fi

# ---------- resolve binary --------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [[ -z "$BINARY_SRC" ]]; then
    # Prefer a pre-built release binary next to this script; fall back to
    # the cargo target directory produced by 'cargo build --release'.
    CANDIDATES=(
        "${SCRIPT_DIR}/target/x86_64-unknown-linux-musl/release/monad-monitoring"
        "${SCRIPT_DIR}/target/release/monad-monitoring"
    )
    for c in "${CANDIDATES[@]}"; do
        if [[ -f "$c" ]]; then
            BINARY_SRC="$c"
            break
        fi
    done
fi

[[ -z "$BINARY_SRC" ]] && error "Binary not found. Build it first:
  cargo build --release
or specify the path:
  sudo ./install.sh --binary /path/to/monad-monitoring"

[[ -f "$BINARY_SRC" ]] || error "Binary not found at: ${BINARY_SRC}"

# ---------- service file check ----------------------------------------------
SERVICE_SRC="${SCRIPT_DIR}/${SERVICE_FILE}"
[[ -f "$SERVICE_SRC" ]] || error "Service file not found at: ${SERVICE_SRC}"

# ---------- create system user ----------------------------------------------
if ! id -u "${RUN_USER}" &>/dev/null; then
    info "Creating system user '${RUN_USER}'..."
    useradd --system --no-create-home --shell /usr/sbin/nologin "${RUN_USER}"
else
    info "System user '${RUN_USER}' already exists."
fi

# ---------- install binary --------------------------------------------------
info "Installing binary to ${BIN_DEST}..."
install -o root -g root -m 755 "${BINARY_SRC}" "${BIN_DEST}"

# ---------- create working directory ----------------------------------------
info "Creating working directory ${INSTALL_DIR}..."
mkdir -p "${INSTALL_DIR}"
chown "${RUN_USER}:${RUN_USER}" "${INSTALL_DIR}"
chmod 750 "${INSTALL_DIR}"

# ---------- write .env template (only if missing) ---------------------------
ENV_FILE="${INSTALL_DIR}/.env"
if [[ ! -f "$ENV_FILE" ]]; then
    info "Writing .env template to ${ENV_FILE}..."
    cat > "${ENV_FILE}" <<'EOF'
# Telegram Bot token — obtain from @BotFather
TELEGRAM_TOKEN=

# Telegram chat / channel ID (negative for channels, e.g. -1001234567890)
TELEGRAM_CHAT_ID=

# JSON-RPC port of your Monad node (default 8080)
RPC_PORT=8080
EOF
    chown "${RUN_USER}:${RUN_USER}" "${ENV_FILE}"
    chmod 600 "${ENV_FILE}"
    warn "Edit ${ENV_FILE} and fill in TELEGRAM_TOKEN and TELEGRAM_CHAT_ID before starting the service."
else
    info ".env already exists — skipping template write."
fi

# ---------- install systemd service -----------------------------------------
info "Installing systemd service..."
install -o root -g root -m 644 "${SERVICE_SRC}" "/etc/systemd/system/${SERVICE_FILE}"

systemctl daemon-reload
systemctl enable "${SERVICE_NAME}"

# ---------- start (or restart) the service ----------------------------------
if grep -q '^TELEGRAM_TOKEN=$' "${ENV_FILE}" 2>/dev/null; then
    warn "TELEGRAM_TOKEN is empty in ${ENV_FILE}."
    warn "Fill in the credentials, then run:  sudo systemctl start ${SERVICE_NAME}"
else
    info "Starting ${SERVICE_NAME}..."
    systemctl restart "${SERVICE_NAME}"
    systemctl --no-pager status "${SERVICE_NAME}"
fi

info "Done. Check logs with:  journalctl -u ${SERVICE_NAME} -f"
