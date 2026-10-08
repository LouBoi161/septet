#!/bin/sh
# Wrap a built Septet binary into Septet.app and a disk image.
# Usage: make-dmg.sh <binary> <version> <arch: arm64|x86_64> <out dir>
set -eu
bin=$1; version=$2; arch=$3; out=$4
root=$(cd "$(dirname "$0")/../../.." && pwd)
work=$(mktemp -d)
app="$work/dmg/Septet.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources" "$out"
install -m 755 "$bin" "$app/Contents/MacOS/septet"
sed "s/@VERSION@/$version/g" "$root/septet/packaging/macos/Info.plist" > "$app/Contents/Info.plist"
cp "$root/LICENSE.md" "$root/NOTICE.md" "$root/THIRD-PARTY-LICENSES.md" "$app/Contents/Resources/"

# Icon: an .icns from the 1024 px PNG.
set_dir="$work/septet.iconset"
mkdir -p "$set_dir"
for s in 16 32 128 256 512; do
    sips -z $s $s "$root/septet/assets/septet-1024.png" --out "$set_dir/icon_${s}x${s}.png" >/dev/null
    d=$((s * 2))
    sips -z $d $d "$root/septet/assets/septet-1024.png" --out "$set_dir/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$set_dir" -o "$app/Contents/Resources/septet.icns"

# Not notarized (no Apple developer account): an ad-hoc signature, so Apple Silicon runs it after
# the user allows it once (right-click > Open, or System Settings > Privacy & Security).
codesign --force --deep --sign - "$app"

ln -s /Applications "$work/dmg/Applications"
cp "$root/README.md" "$work/dmg/README.md"
hdiutil create -volname "Septet" -srcfolder "$work/dmg" -ov -format UDZO "$out/Septet-$version-macos-$arch.dmg"
rm -rf "$work"
ls -la "$out"
