#!/usr/bin/env bash
# Packages a release build of the game for players, into dist/:
#
#   cargo build --release && tools/package.sh
#
#   Linux    washoi-<version>-linux-<arch>.tar.gz    washoi, README.txt, licences,
#            the icon and a .desktop launcher entry
#            washoi-<version>-linux-<arch>.AppImage  the same as one file to
#            run, which updates itself (update_check), when appimagetool is
#            found (APPIMAGETOOL names it otherwise)
#   macOS    washoi-<version>-macos-<arch>.zip       Washoi.app (icon from
#            assets/icon, ad-hoc signed unless MACOS_SIGN_IDENTITY names a
#            Developer ID in the keychain)
#   Windows  washoi-<version>-windows-<arch>.zip     washoi.exe (its icon built in
#            by build.rs), README.txt, licences
#            washoi-<version>-windows-<arch>-setup.exe the same files as an
#            installer (tools/windows/washoi.iss), when Inno Setup 6 is
#            installed (ISCC names its compiler otherwise)
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

A plain start reads its options from washoi.args beside the program, when
that file exists (one or more per line, # starts a comment), so a shortcut
or script is never needed:
  --region us

Settings live in the config folder (~/.config/washoi on Linux,
~/Library/Application Support/Washoi on macOS, %APPDATA%\\Washoi on
Windows); a crash leaves a report in its crashes folder.

If this build is not signed, the first start needs one extra step:
  macOS    open Washoi.app once, then System Settings > Privacy & Security
           > "Open Anyway" (or: xattr -dr com.apple.quarantine Washoi.app)
  Windows  "Windows protected your PC": More info > Run anyway
TXT
}

case $os in
  linux)
    mkdir -p "$stage/$name"
    cp "$bin" "$stage/$name/washoi"
    # Not stripped: a crash report's backtrace needs the symbols.
    extras "$stage/$name"
    # The icon and a launcher entry; the window's app id
    # (net.washoi.Washoi) matches them on Wayland and X11.
    cp assets/icon/washoi-512.png "$stage/$name/net.washoi.Washoi.png"
    cat > "$stage/$name/net.washoi.Washoi.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Washoi
Comment=Arena shooter
Exec=washoi
Icon=net.washoi.Washoi
StartupWMClass=washoi
Categories=Game;ActionGame;
DESKTOP
    cat >> "$stage/$name/README.txt" <<TXT

Linux needs WebKitGTK 4.1 for the lobby and ALSA for sound:
  Arch, Manjaro        sudo pacman -S webkit2gtk-4.1
  Debian, Ubuntu       sudo apt install libwebkit2gtk-4.1-0
  Fedora               sudo dnf install webkit2gtk4.1
  openSUSE             sudo zypper install libwebkit2gtk-4_1-0
(Arch's webkit2gtk package is the older 4.0, not the one needed.)

The AppImage (washoi-$version-linux-$arch.AppImage) is the same game as one
file: chmod +x it and run it. It updates itself when a new version is out.

To add Washoi to the app menu, with its icon:
  mkdir -p ~/.local/bin ~/.local/share/icons/hicolor/512x512/apps ~/.local/share/applications
  cp washoi ~/.local/bin/
  cp net.washoi.Washoi.png ~/.local/share/icons/hicolor/512x512/apps/
  cp net.washoi.Washoi.desktop ~/.local/share/applications/
TXT
    tar -C "$stage" -czf "dist/$name.tar.gz" "$name"
    echo "dist/$name.tar.gz"
    # The AppImage: the same files in an AppDir. Like the tar.gz it uses the
    # system's WebKitGTK 4.1 and ALSA rather than carrying them.
    tool=${APPIMAGETOOL:-$(command -v appimagetool || true)}
    if [ -z "$tool" ]; then
      echo "no appimagetool: the AppImage is skipped" >&2
    else
      appdir="$stage/Washoi.AppDir"
      mkdir -p "$appdir/usr/bin" "$appdir/usr/share/doc/washoi" \
        "$appdir/usr/share/icons/hicolor/512x512/apps" "$appdir/usr/share/applications"
      cp "$bin" "$appdir/usr/bin/washoi"
      cp -R "$stage/$name/licenses" "$stage/$name/README.txt" "$appdir/usr/share/doc/washoi/"
      cp assets/icon/washoi-512.png "$appdir/net.washoi.Washoi.png"
      cp assets/icon/washoi-512.png "$appdir/usr/share/icons/hicolor/512x512/apps/net.washoi.Washoi.png"
      cp assets/icon/washoi-512.png "$appdir/.DirIcon"
      cp "$stage/$name/net.washoi.Washoi.desktop" "$appdir/net.washoi.Washoi.desktop"
      cp "$stage/$name/net.washoi.Washoi.desktop" "$appdir/usr/share/applications/"
      # A script, not a link: it names the package when WebKitGTK is missing.
      cp tools/linux/AppRun "$appdir/AppRun"
      # No FUSE on build machines: the tool runs extracted.
      APPIMAGE_EXTRACT_AND_RUN=1 ARCH=$arch "$tool" --no-appstream "$appdir" "dist/$name.AppImage" >/dev/null
      echo "dist/$name.AppImage"
    fi
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
    # The installer players double-click, with Start menu and desktop
    # shortcuts and an uninstaller.
    iscc=${ISCC:-}
    if [ -z "$iscc" ]; then
      for c in iscc "/c/Program Files (x86)/Inno Setup 6/ISCC.exe" "/c/Program Files/Inno Setup 6/ISCC.exe"; do
        if command -v "$c" >/dev/null 2>&1 || [ -f "$c" ]; then iscc=$c; break; fi
      done
    fi
    if [ -z "$iscc" ]; then
      echo "no Inno Setup 6 (ISCC): the installer is skipped" >&2
    else
      # MSYS would turn the /D switches into paths.
      MSYS2_ARG_CONV_EXCL='*' "$iscc" /Q "/DAppVersion=$version" \
        "/DStageDir=$(cygpath -w "$stage/$name")" "/DOutputDir=$(cygpath -w "$PWD/dist")" \
        "$(cygpath -w tools/windows/washoi.iss)"
      echo "dist/$name-setup.exe"
    fi
    ;;
esac
