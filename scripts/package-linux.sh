#!/usr/bin/env bash
# Linux packages: a .deb and a plain tarball of the same FHS tree. Assumes
# `cargo build --release --target $TARGET` already ran.
#
#   TARGET=x86_64-unknown-linux-gnu scripts/package-linux.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="${TARGET:?rust target triple, e.g. x86_64-unknown-linux-gnu}"
VERSION="${VERSION:-$("$ROOT/scripts/version.sh")}"
DIST="${DIST:-$ROOT/dist}"
BIN="$ROOT/target/$TARGET/release/hjson"
# rust triples and debian arches disagree on two names.
case "${TARGET%%-*}" in
  x86_64) ARCH=amd64 ;;
  aarch64) ARCH=arm64 ;;
  *) ARCH="${TARGET%%-*}" ;;
esac
[[ -x "$BIN" ]] || { echo "no binary at $BIN — build it first"; exit 1; }

PREFIX="hjson-$VERSION-linux-$ARCH"
STAGE="$DIST/$PREFIX"
rm -rf "$STAGE"; mkdir -p "$STAGE/usr/bin" "$STAGE/usr/share/applications" \
  "$STAGE/usr/share/icons/hicolor/scalable/apps"
cp "$BIN" "$STAGE/usr/bin/hjson"
chmod 755 "$STAGE/usr/bin/hjson"
cp "$ROOT/resources/icon.svg" "$STAGE/usr/share/icons/hicolor/scalable/apps/hjson.svg"

# A raster is what most panels actually show; skip it if librsvg is absent.
if command -v rsvg-convert >/dev/null; then
  mkdir -p "$STAGE/usr/share/icons/hicolor/256x256/apps"
  rsvg-convert -w 256 -h 256 "$ROOT/resources/icon.svg" \
    -o "$STAGE/usr/share/icons/hicolor/256x256/apps/hjson.png"
fi

cat > "$STAGE/usr/share/applications/com.hjson.viewer.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=HJson
Comment=JSON viewer and structure comparer
Exec=hjson %f
Icon=hjson
Terminal=false
Categories=Development;Utility;
MimeType=application/json;
DESKTOP

# .deb: the tree doubles as the package root, so no fakeroot or packing as root.
# The Depends list mirrors the binary's DT_NEEDED plus what gpui opens at run
# time (wayland, fontconfig); the linker line from a build is what names them.
DEB="$DIST/${PREFIX}.deb"
mkdir -p "$STAGE/DEBIAN"
cat > "$STAGE/DEBIAN/control" <<CONTROL
Package: hjson
Version: $VERSION
Architecture: $ARCH
Priority: optional
Section: utils
Maintainer: leaxoy <leaxoy@users.noreply.github.com>
Depends: libx11-6, libxcb1, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libfontconfig1, libfreetype6
Description: JSON viewer and structure comparer
 Parses and displays JSON as a collapsible tree beside its raw text, and
 compares two documents by JSONPath. Everything runs locally.
CONTROL
chmod 755 "$STAGE/DEBIAN"
dpkg-deb --root-owner-group -Zzstd -b "$STAGE" "$DEB"

# Tarball of the same tree, minus the debian metadata.
tar -C "$DIST" --exclude='*/DEBIAN' -cJf "$DIST/${PREFIX}.tar.xz" "$PREFIX"
rm -rf "$STAGE"

echo "packaged $DEB and $DIST/${PREFIX}.tar.xz"
