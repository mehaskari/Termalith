#!/usr/bin/env bash
# Installs the built installers the way a user would and launches the
# installed app with the smoke test (macOS: DMG; Linux: deb and AppImage).
#
# Usage: scripts/installer-test.sh <target/release/bundle>
# On headless Linux, run under xvfb-run.
set -euo pipefail

BUNDLE=${1:?usage: installer-test.sh <bundle-dir>}
HERE=$(cd "$(dirname "$0")" && pwd)
WORK=${RUNNER_TEMP:-$(mktemp -d)}/installer-test
rm -rf "$WORK" && mkdir -p "$WORK"

case "$(uname -s)" in
Darwin)
  DMG=$(ls "$BUNDLE"/dmg/*.dmg | head -1)
  echo "== DMG: $DMG"
  hdiutil verify "$DMG"
  MNT="$WORK/mnt"
  mkdir -p "$MNT"
  hdiutil attach "$DMG" -nobrowse -readonly -mountpoint "$MNT" >/dev/null
  APP=$(ls -d "$MNT"/*.app | head -1)
  ditto "$APP" "$WORK/Termalith.app"
  hdiutil detach "$MNT" >/dev/null
  PLIST="$WORK/Termalith.app/Contents/Info.plist"
  echo "minimum macOS: $(/usr/libexec/PlistBuddy -c 'Print :LSMinimumSystemVersion' "$PLIST")"
  /usr/libexec/PlistBuddy -c 'Print :NSLocalNetworkUsageDescription' "$PLIST" >/dev/null
  # A broken bundle signature makes macOS report a downloaded app as
  # "damaged" instead of offering Open; the ad-hoc signature must verify.
  codesign --verify --deep --strict "$WORK/Termalith.app"
  echo "signature: $(codesign -dv "$WORK/Termalith.app" 2>&1 | grep -E '^Signature=' )"
  BIN=$(ls "$WORK/Termalith.app/Contents/MacOS/"* | head -1)
  bash "$HERE/smoke-test.sh" "$BIN"
  ;;
Linux)
  DEB=$(realpath "$(ls "$BUNDLE"/deb/*.deb | head -1)")
  echo "== deb: $DEB"
  sudo apt-get install -y --no-install-recommends "$DEB" >/dev/null
  BIN=$(dpkg -L "$(dpkg-deb -f "$DEB" Package)" | grep -E '^/usr/bin/' | head -1)
  echo "installed: $BIN"
  test -f /usr/share/applications/*.desktop || ls /usr/share/applications | grep -i termalith
  bash "$HERE/smoke-test.sh" "$BIN"
  sudo apt-get remove -y "$(dpkg-deb -f "$DEB" Package)" >/dev/null

  APPIMAGE=$(ls "$BUNDLE"/appimage/*.AppImage | head -1)
  echo "== AppImage: $APPIMAGE"
  cp "$APPIMAGE" "$WORK/t.AppImage" && chmod +x "$WORK/t.AppImage"
  # Hosted runners have no FUSE; extracting runs the same payload.
  (cd "$WORK" && ./t.AppImage --appimage-extract >/dev/null)
  BIN=$(ls "$WORK"/squashfs-root/usr/bin/* | head -1)
  bash "$HERE/smoke-test.sh" "$BIN"
  ;;
*)
  echo "unsupported OS"; exit 1 ;;
esac
echo "PASS: installers install and the app starts"
