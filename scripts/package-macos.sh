#!/usr/bin/env bash
# Disk image for the macOS app bundle. Assumes `cargo build --release --target
# $TARGET` already ran, and only uses host tools (sips / iconutil / hdiutil), so
# it runs on a macOS runner.
#
#   TARGET=aarch64-apple-darwin scripts/package-macos.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${TARGET:?rust target triple, e.g. aarch64-apple-darwin}"
# The label users read on a download page: Apple silicon is "arm64".
case "${TARGET%%-*}" in
  aarch64) ARCH=arm64 ;;
  x86_64) ARCH=x86_64 ;;
  *) ARCH="${TARGET%%-*}" ;;
esac
VERSION="${VERSION:-$("$ROOT/scripts/version.sh")}"
DIST="${DIST:-$ROOT/dist}"
DMG="$DIST/HJson-$VERSION-macos-$ARCH.dmg"

mkdir -p "$DIST"
# The bundle keeps its plain name — the version belongs to the image, so the
# Dock and /Applications always show "HJson".
WORK="$DIST/macos-$ARCH"
APP="$WORK/HJson.app"
rm -rf "$WORK"; mkdir -p "$WORK"
# make-app.sh rasterizes the SVG icon, assembles the bundle and ad-hoc signs it.
TARGET="$TARGET" VERSION="$VERSION" SKIP_BUILD=1 DIST_APP="$APP" "$ROOT/scripts/make-app.sh" release

# Drag-to-Applications image: the app plus the standard shortcut.
STAGE="$WORK/dmg"
mkdir -p "$STAGE"
cp -R "$APP" "$STAGE/"
ln -s /Applications "$STAGE/Applications"
hdiutil create -volname "HJson" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
rm -rf "$WORK"

echo "packaged $DMG"
