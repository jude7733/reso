#!/usr/bin/env bash
# ==============================================================================
# Reso - Build & Installation Script
# Bit-perfect lossless Internet radio streamer & PipeWire inspector
# ==============================================================================

set -euo pipefail

# Colors for terminal output
BOLD="\033[1m"
GREEN="\033[0;32m"
CYAN="\033[0;36m"
YELLOW="\033[0;33m"
RED="\033[0;31m"
RESET="\033[0m"

log_info() {
    echo -e "${CYAN}${BOLD}[INFO]${RESET} $1"
}

log_success() {
    echo -e "${GREEN}${BOLD}[SUCCESS]${RESET} $1"
}

log_warn() {
    echo -e "${YELLOW}${BOLD}[WARN]${RESET} $1"
}

log_error() {
    echo -e "${RED}${BOLD}[ERROR]${RESET} $1" >&2
}

# Resolve project root directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "${SCRIPT_DIR}"

# Determine installation prefix
# Default: User-local (~/.local) without requiring sudo privileges
PREFIX="${PREFIX:-${HOME}/.local}"
INSTALL_SYSTEM=false

for arg in "$@"; do
    case "${arg}" in
        --system)
            PREFIX="/usr/local"
            INSTALL_SYSTEM=true
            ;;
        --help|-h)
            echo -e "${BOLD}Reso Installer${RESET}"
            echo "Usage: ./install.sh [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --system         Install system-wide to /usr/local (requires sudo/root)"
            echo "  --uninstall      Remove installed Reso binary, desktop entry, and icons"
            echo "  -h, --help       Show this help message"
            echo ""
            echo "Environment variables:"
            echo "  PREFIX           Custom install prefix (default: ~/.local)"
            exit 0
            ;;
        --uninstall)
            log_info "Uninstalling Reso from ${PREFIX}..."
            rm -f "${PREFIX}/bin/reso"
            rm -f "${PREFIX}/share/applications/reso.desktop"
            rm -rf "${PREFIX}/share/reso"
            rm -f "${PREFIX}/share/icons/hicolor/scalable/apps/reso.svg"
            rm -f "${PREFIX}/share/icons/hicolor/512x512/apps/reso.png"
            rm -f "${PREFIX}/share/icons/hicolor/256x256/apps/reso.png"
            rm -f "${PREFIX}/share/icons/hicolor/128x128/apps/reso.png"
            rm -f "${PREFIX}/share/icons/hicolor/64x64/apps/reso.png"
            rm -f "${PREFIX}/share/icons/hicolor/48x48/apps/reso.png"

            if command -v update-desktop-database >/dev/null 2>&1; then
                update-desktop-database "${PREFIX}/share/applications" 2>/dev/null || true
            fi
            if command -v gtk-update-icon-cache >/dev/null 2>&1; then
                gtk-update-icon-cache -q -f -t "${PREFIX}/share/icons/hicolor" 2>/dev/null || true
            fi

            log_success "Reso uninstalled cleanly."
            exit 0
            ;;
        *)
            log_error "Unknown option: ${arg}"
            echo "Use ./install.sh --help for available options."
            exit 1
            ;;
    esac
done

# Ensure cargo is available
if ! command -v cargo >/dev/null 2>&1; then
    log_error "Rust and cargo are required to build Reso. Please install rustup: https://rustup.rs"
    exit 1
fi

BIN_DIR="${PREFIX}/bin"
DATA_DIR="${PREFIX}/share"
APP_DIR="${DATA_DIR}/applications"
ICON_DIR="${DATA_DIR}/icons/hicolor"

if [ "${INSTALL_SYSTEM}" = true ] && [ "$(id -u)" -ne 0 ]; then
    log_error "--system installation requires root privileges. Please run with sudo: sudo ./install.sh --system"
    exit 1
fi

echo -e "${CYAN}${BOLD}"
echo "  ╔════════════════════════════════════════════════════════════╗"
echo "  ║                  RESO INSTALLATION WIZARD                  ║"
echo "  ║     Audiophile Bit-Perfect Radio & PipeWire Diagnostics    ║"
echo "  ╚════════════════════════════════════════════════════════════╝"
echo -e "${RESET}"

# Step 1: Build release binary
log_info "Compiling Reso in release mode (cargo build --release)..."
cargo build --release

RELEASE_BIN="${SCRIPT_DIR}/target/release/reso"
if [ ! -f "${RELEASE_BIN}" ]; then
    log_error "Release binary not found at ${RELEASE_BIN}!"
    exit 1
fi

# Step 2: Install binary atomically (prevents 'Text file busy' ETXTBSY error if running)
log_info "Installing executable to ${BIN_DIR}/reso..."
install -d -m 755 "${BIN_DIR}"
install -m 755 "${RELEASE_BIN}" "${BIN_DIR}/reso"

# Step 3: Install desktop integration entry
log_info "Installing desktop entry to ${APP_DIR}/reso.desktop..."
install -d -m 755 "${APP_DIR}"
if [ -f "${SCRIPT_DIR}/reso.desktop" ]; then
    install -m 644 "${SCRIPT_DIR}/reso.desktop" "${APP_DIR}/reso.desktop"
fi

# Step 4: Install multi-resolution and scalable application icons
log_info "Installing application icons..."
if [ -d "${SCRIPT_DIR}/assets" ]; then
    # Scalable SVG
    if [ -f "${SCRIPT_DIR}/assets/reso.svg" ]; then
        install -d -m 755 "${ICON_DIR}/scalable/apps"
        install -m 644 "${SCRIPT_DIR}/assets/reso.svg" "${ICON_DIR}/scalable/apps/reso.svg"
    fi

    # Standard Hicolor PNG resolutions
    for size in 512 256 128 64 48; do
        PNG_FILE="${SCRIPT_DIR}/assets/reso-${size}.png"
        if [ "${size}" -eq 256 ] && [ ! -f "${PNG_FILE}" ]; then
            PNG_FILE="${SCRIPT_DIR}/assets/reso.png"
        fi
        if [ -f "${PNG_FILE}" ]; then
            install -d -m 755 "${ICON_DIR}/${size}x${size}/apps"
            install -m 644 "${PNG_FILE}" "${ICON_DIR}/${size}x${size}/apps/reso.png"
        fi
    done

    # Fallback app data folder for direct resolution
    install -d -m 755 "${DATA_DIR}/reso"
    if [ -f "${SCRIPT_DIR}/assets/reso.png" ]; then
        install -m 644 "${SCRIPT_DIR}/assets/reso.png" "${DATA_DIR}/reso/reso.png"
    fi
fi

# Step 5: Update caches if available
log_info "Updating desktop and icon caches..."
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "${APP_DIR}" 2>/dev/null || true
fi

if command -v gtk-update-icon-cache >/dev/null 2>&1; then
    gtk-update-icon-cache -q -f -t "${ICON_DIR}" 2>/dev/null || true
fi

# Step 6: Verify PATH
echo ""
log_success "Reso successfully installed to ${BIN_DIR}/reso!"

if [[ ":${PATH}:" != *":${BIN_DIR}:"* ]]; then
    log_warn "${BIN_DIR} is not currently in your PATH."
    echo "  Add it to your shell config (~/.bashrc or ~/.zshrc):"
    echo -e "    ${BOLD}export PATH=\"\$HOME/.local/bin:\$PATH\"${RESET}"
fi

echo ""
echo -e "You can now run: ${CYAN}${BOLD}reso${RESET} (or launch Reso from your application launcher)"
