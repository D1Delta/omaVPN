#!/usr/bin/env bash
set -euo pipefail

# omavpn uninstaller script
# Usage: sudo ./uninstall.sh

RED='\033[0;31m'
GREEN='\033[0;32m'
BOLD='\033[1m'
NC='\033[0m'

echo -e "${BOLD}=== omavpn Uninstaller ===${NC}\n"

if [ "$(id -u)" -ne 0 ]; then
    echo -e "${RED}Error: This script must be run as root (use sudo ./uninstall.sh)${NC}" >&2
    exit 1
fi

echo "Stopping and disabling omavpnd service..."
systemctl stop omavpnd.service || true
systemctl disable omavpnd.service || true

echo "Removing binaries and service file..."
rm -f /usr/bin/omavpn
rm -f /usr/bin/omavpnd
rm -f /usr/lib/systemd/system/omavpnd.service
rm -f /etc/systemd/system/omavpnd.service
systemctl daemon-reload

echo -e "\n${GREEN}✓ omavpn has been uninstalled.${NC}"
echo "User config in ~/.config/omavpn was preserved. To remove it run: rm -rf ~/.config/omavpn"
