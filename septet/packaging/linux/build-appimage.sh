#!/bin/sh
# Package a built Septet as an AppImage and a tar.gz. Usage: build-appimage.sh <binary> <version> <out dir>
set -eu
bin=$1; version=$2; out=$3
root=$(cd "$(dirname "$0")/../../.." && pwd)
mkdir -p "$out"
work=$(mktemp -d)

# tar.gz: the program, its menu entry and icon, an installer and the licenses.
tdir="$work/septet-$version-linux-x86_64"
mkdir -p "$tdir"
install -m 755 "$bin" "$tdir/septet"
cp "$root/septet/packaging/linux/septet.desktop" "$root/septet/packaging/linux/install.sh" "$root/septet/assets/septet.svg" "$tdir/"
cp "$root/LICENSE.md" "$root/NOTICE.md" "$root/THIRD-PARTY-LICENSES.md" "$root/README.md" "$tdir/"
tar -C "$work" -czf "$out/septet-$version-linux-x86_64.tar.gz" "septet-$version-linux-x86_64"

# AppImage: one file that runs on most distributions (needs the system's Vulkan/OpenGL, X11 or
# Wayland and ALSA libraries, as every desktop has).
app="$work/Septet.AppDir"
mkdir -p "$app/usr/bin" "$app/usr/share/septet" "$app/usr/share/applications" "$app/usr/share/icons/hicolor/256x256/apps"
install -m 755 "$bin" "$app/usr/bin/septet"
cp "$root/LICENSE.md" "$root/NOTICE.md" "$root/THIRD-PARTY-LICENSES.md" "$app/usr/share/septet/"
cp "$root/septet/packaging/linux/septet.desktop" "$app/septet.desktop"
cp "$root/septet/packaging/linux/septet.desktop" "$app/usr/share/applications/"
convert_png() { if command -v magick >/dev/null; then magick -background none "$1" -resize 256x256 "$2"; else rsvg-convert -w 256 -h 256 "$1" -o "$2"; fi; }
convert_png "$root/septet/assets/septet.svg" "$app/septet.png"
cp "$app/septet.png" "$app/usr/share/icons/hicolor/256x256/apps/septet.png"
ln -s usr/bin/septet "$app/AppRun"
tool="$work/appimagetool"
curl -fsSL -o "$tool" https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
chmod +x "$tool"
ARCH=x86_64 VERSION="$version" "$tool" --appimage-extract-and-run "$app" "$out/Septet-$version-x86_64.AppImage"
rm -rf "$work"
ls -la "$out"
