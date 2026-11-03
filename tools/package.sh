#!/usr/bin/env bash
# Packages a release build of the game for players, into dist/:
#
#   cargo build --release && tools/package.sh
#
#   Linux    washoi-<version>-linux-<arch>.tar.gz    washoi, README.txt, licences
#   macOS    washoi-<version>-macos-<arch>.zip       Washoi.app (icon from
#            assets/icon, ad-hoc signed unless MACOS_SIGN_IDENTITY names a
#            Developer ID in the keychain)
#   Windows  washoi-<version>-windows-<arch>.zip     washoi.exe, README.txt, licences
#
# BIN overrides the binary (default target/release/washoi[.exe]); VERSION
# the version in the names (default Cargo.toml's).
set -euo pipefail
cd "$(dirname "$0")/.."
version=${VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}
arch=$(uname -m)
[ "$arch" = arm64 ] && arch=aarch64
case "$(uname -s)" in
  Linux) os=linux; exe=washoi ;;
  Darwin) os=macos; exe=washoi ;;
  MINGW* | MSYS* | CYGWIN*) os=windows; exe=washoi.exe ;;
  *) echo "unknown system $(uname -s)" >&2; exit 1 ;;
esac
bin=${BIN:-target/release/$exe}
[ -f "$bin" ] || { echo "$bin: build it first (cargo build --release)" >&2; exit 1; }
name="washoi-$version-$os-$arch"
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
mkdir -p dist

# What every package carries beside the program.
extras() {
  local into=$1
  mkdir -p "$into/licenses"
  cp LICENSE "$into/licenses/LICENSE.txt"
  cp assets/fonts/Orbitron-OFL.txt assets/fonts/Rajdhani-OFL.txt "$into/licenses/"
  "${PYTHON:-python3}" tools/third_party_licenses.py > "$into/licenses/THIRD_PARTY_LICENSES.txt"
  cat > "$into/README.txt" <<TXT
Washoi $version

Start the game with washoi (Washoi.app on macOS). The first start downloads
the game data (about 350 MB); later starts fetch only what changed.

  washoi --region us     play on the US servers (default: eu)
  washoi --offline       start with the data already downloaded
  washoi --version       print the version

Settings live in the config folder (~/.config/washoi on Linux,
~/Library/Application Support/Washoi on macOS, %APPDATA%\\Washoi on
Windows); a crash leaves a report in its crashes folder.
TXT
}

case $os in
  linux)
    mkdir -p "$stage/$name"
    cp "$bin" "$stage/$name/washoi"
    # Not stripped: a crash report's backtrace needs the symbols.
    extras "$stage/$name"
    tar -C "$stage" -czf "dist/$name.tar.gz" "$name"
    echo "dist/$name.tar.gz"
    ;;
  macos)
    app="$stage/Washoi.app"
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    cp "$bin" "$app/Contents/MacOS/washoi"
    extras "$app/Contents/Resources"
    iconset="$stage/washoi.iconset"
    mkdir -p "$iconset"
    for size in 16 32 128 256 512; do
      sips -z $size $size assets/icon/washoi-512.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
      double=$((size * 2))
      [ $double -le 512 ] && sips -z $double $double assets/icon/washoi-512.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
    done
    iconutil -c icns "$iconset" -o "$app/Contents/Resources/washoi.icns"
    cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Washoi</string>
  <key>CFBundleDisplayName</key><string>Washoi</string>
  <key>CFBundleIdentifier</key><string>com.higherstandardgames.washoi</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleExecutable</key><string>washoi</string>
  <key>CFBundleIconFile</key><string>washoi</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.action-games</string>
</dict>
</plist>
PLIST
    # Ad-hoc unless a Developer ID is in the keychain; notarizing follows
    # in the release workflow when its secrets are there.
    # Notarizing needs a secure timestamp; an ad-hoc signature cannot have one.
    if [ -n "${MACOS_SIGN_IDENTITY:-}" ]; then
      codesign --force --deep --options runtime --timestamp --sign "$MACOS_SIGN_IDENTITY" "$app"
    else
      codesign --force --deep --options runtime --timestamp=none --sign - "$app"
    fi
    (cd "$stage" && ditto -c -k --keepParent Washoi.app "$OLDPWD/dist/$name.zip")
    echo "dist/$name.zip"
    ;;
  windows)
    mkdir -p "$stage/$name"
    cp "$bin" "$stage/$name/washoi.exe"
    # The symbols beside the exe, so a crash report's backtrace has names.
    pdb="${bin%.exe}.pdb"
    [ -f "$pdb" ] && cp "$pdb" "$stage/$name/washoi.pdb"
    extras "$stage/$name"
    (cd "$stage" && 7z a -tzip "$OLDPWD/dist/$name.zip" "$name" >/dev/null)
    echo "dist/$name.zip"
    ;;
esac
