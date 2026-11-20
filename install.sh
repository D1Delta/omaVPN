#!/usr/bin/env bash
set -euo pipefail

# omavpn installer script for Arch Linux / Omarchy / generic Linux
# Usage: sudo ./install.sh

RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
NC='\033[0m'

echo -e "${BOLD}${BLUE}=== omavpn Installer ===${NC}\n"

if [ "$(id -u)" -ne 0 ]; then
    echo -e "${RED}Error: This script must be run as root (use sudo ./install.sh)${NC}" >&2
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Detect binary location (either target/release from source, or alongside install.sh in tarball)
OMAVPN_BIN=""
OMAVPND_BIN=""

if [ -f "$SCRIPT_DIR/omavpn" ] && [ -f "$SCRIPT_DIR/omavpnd" ]; then
    OMAVPN_BIN="$SCRIPT_DIR/omavpn"
    OMAVPND_BIN="$SCRIPT_DIR/omavpnd"
elif [ -f "$SCRIPT_DIR/target/release/omavpn" ] && [ -f "$SCRIPT_DIR/target/release/omavpnd" ]; then
    OMAVPN_BIN="$SCRIPT_DIR/target/release/omavpn"
    OMAVPND_BIN="$SCRIPT_DIR/target/release/omavpnd"
else
    echo -e "${RED}Error: Binaries not found. Please run 'cargo build --release' first.${NC}" >&2
    exit 1
fi

SERVICE_FILE=""
if [ -f "$SCRIPT_DIR/systemd/omavpnd.service" ]; then
    SERVICE_FILE="$SCRIPT_DIR/systemd/omavpnd.service"
elif [ -f "$SCRIPT_DIR/omavpnd.service" ]; then
    SERVICE_FILE="$SCRIPT_DIR/omavpnd.service"
fi

echo -e "Installing binaries to ${BOLD}/usr/bin${NC}..."
install -Dm755 "$OMAVPN_BIN" /usr/bin/omavpn
install -Dm755 "$OMAVPND_BIN" /usr/bin/omavpnd

if [ -n "$SERVICE_FILE" ]; then
    SYSTEMD_DIR="/usr/lib/systemd/system"
    if [ ! -d "$SYSTEMD_DIR" ]; then
        SYSTEMD_DIR="/etc/systemd/system"
    fi
    echo -e "Installing systemd service to ${BOLD}$SYSTEMD_DIR/omavpnd.service${NC}..."
    install -Dm644 "$SERVICE_FILE" "$SYSTEMD_DIR/omavpnd.service"
    systemctl daemon-reload
    systemctl enable --now omavpnd.service
    echo -e "${GREEN}✓ omavpnd service enabled and started.${NC}"
fi

# Check dependencies
echo -e "\nChecking dependencies..."
if command -v sing-box >/dev/null 2>&1; then
    echo -e "${GREEN}✓ sing-box found: $(sing-box version 2>/dev/null | head -n1 || echo 'ok')${NC}"
else
    echo -e "${YELLOW}! sing-box is not installed.${NC} On Arch: ${BOLD}sudo pacman -S sing-box${NC}"
fi

if command -v xray >/dev/null 2>&1; then
    echo -e "${GREEN}✓ xray-core found: $(xray version 2>/dev/null | head -n1 || echo 'ok')${NC}"
else
    echo -e "${YELLOW}! xray is not installed (needed for XHTTP & VLESS post-quantum ML-KEM).${NC}"
    echo -e "  To install on Arch: ${BOLD}yay -S xray-bin${NC}"
fi

if command -v wl-paste >/dev/null 2>&1; then
    echo -e "${GREEN}✓ wl-clipboard found.${NC}"
else
    echo -e "${YELLOW}! wl-clipboard is recommended for clipboard imports on Wayland.${NC}"
fi

echo -e "\n${BOLD}${GREEN}Installation complete!${NC}"
echo -e "Run ${BOLD}omavpn${NC} in your terminal to start the TUI client."
echo -e "Run ${BOLD}omavpn --help${NC} for CLI usage."
