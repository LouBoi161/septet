#!/bin/sh
# Install Septet for the current user: binary, icon and menu entry. Run from the unpacked
# download (or the repository, after `cargo build --release -p septet`).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
if [ -x "$here/septet" ]; then
    bin="$here/septet"; share="$here"
else
    root=$(cd "$here/../../.." && pwd)
    bin="$root/target/release/septet"; share="$root/septet/packaging/linux"
    [ -x "$bin" ] || { echo "Build first: cargo build --release -p septet" >&2; exit 1; }
    cp "$root/septet/assets/septet.svg" "$share/septet.svg" 2>/dev/null || true
fi
prefix="${XDG_DATA_HOME:-$HOME/.local/share}"
mkdir -p "$HOME/.local/bin" "$prefix/applications" "$prefix/icons/hicolor/scalable/apps"
install -m 755 "$bin" "$HOME/.local/bin/septet"
install -m 644 "$share/septet.svg" "$prefix/icons/hicolor/scalable/apps/septet.svg"
install -m 644 "$share/septet.desktop" "$prefix/applications/septet.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$prefix/applications" || true
echo "Installed ~/.local/bin/septet and the menu entry \"Septet\"."
