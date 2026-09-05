#!/usr/bin/env bash
set -e

REPO_URL="https://github.com/hnpf/tc.git"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

if ! command -v cargo >/dev/null 2>&1; then
    echo "error: cargo is required to install torr. please install rust: https://rustup.rs"
    exit 1
fi

mkdir -p "$INSTALL_DIR"

if [ -f "Cargo.toml" ] && grep -q 'name = "torr"' Cargo.toml 2>/dev/null; then
    echo "==> Building torr from local directory..."
    cargo build --release
    install -m 755 "target/release/torr" "$INSTALL_DIR/torr"
else
    TMP_DIR="$(mktemp -d)"
    trap 'rm -rf "$TMP_DIR"' EXIT

    echo "==> Cloning torr repository..."
    git clone --depth 1 "$REPO_URL" "$TMP_DIR/torr"
    cd "$TMP_DIR/torr"

    echo "==> Building torr..."
    cargo build --release
    install -m 755 "target/release/torr" "$INSTALL_DIR/torr"
fi

echo "==> Successfully installed torr to $INSTALL_DIR/torr"

DESKTOP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
mkdir -p "$DESKTOP_DIR"
install -m 644 torr.desktop "$DESKTOP_DIR/torr.desktop"

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$DESKTOP_DIR" >/dev/null 2>&1 || true
fi

if command -v xdg-mime >/dev/null 2>&1; then
    xdg-mime default torr.desktop x-scheme-handler/magnet >/dev/null 2>&1 || true
    xdg-mime default torr.desktop application/x-bittorrent >/dev/null 2>&1 || true
    echo "==> Registered torr as default xdg handler for magnet links and .torrent files"
fi

case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *)
        echo ""
        echo "note: $INSTALL_DIR is not in your PATH."
        echo "add this to your ~/.bashrc or ~/.zshrc:"
        echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
        ;;
esac

echo ""
echo "Try running: torr --help"
