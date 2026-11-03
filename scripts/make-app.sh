#!/bin/zsh
# Build a macOS HJson.app bundle with an .icns dock icon.
# Usage: scripts/make-app.sh [debug|release]   (default: debug)
#
# Environment:
#   TARGET     rust target triple to build for (default: the host). Cross-building
#              is how one runner produces both the arm64 and the x86_64 app.
#   SKIP_BUILD set to reuse a binary the caller already built (CI does this).
#   DIST_APP   where the bundle lands (default: target/HJson.app).
#   VERSION    the version written to Info.plist (default: Cargo.toml's).

set -euo pipefail

MODE="${1:-debug}"
ROOT="${0:A:h:h}"
APP_NAME="HJson"
BUNDLE_ID="com.hjson.viewer"
TARGET="${TARGET:-}"
VERSION="${VERSION:-$("$ROOT/scripts/version.sh")}"
BUILD_DIR="$ROOT/target/${TARGET:+$TARGET/}$MODE"
DIST_APP="${DIST_APP:-$ROOT/target/$APP_NAME.app}"

# 1. Build the binary (unless the caller already did).
cd "$ROOT"
BUILD=()
if [[ -n "$TARGET" ]]; then BUILD=(--target "$TARGET"); fi
case "$MODE" in
  debug)   [[ -n "${SKIP_BUILD:-}" ]] || cargo build $BUILD ;;
  release) [[ -n "${SKIP_BUILD:-}" ]] || cargo build --release $BUILD ;;
  *) echo "unknown mode: $MODE (use debug|release)"; exit 1 ;;
esac

# 2. Rasterize the SVG source to PNGs of every icon size, then pack .icns.
ICON_DIR="$ROOT/target/icon-build"
rm -rf "$ICON_DIR" "$DIST_APP"
mkdir -p "$ICON_DIR"
# There is no SVG-capable rasteriser in /usr/bin, so use librsvg when it is
# installed and fall back to QuickLook, which needs the window server the macOS
# runners do provide.
if command -v rsvg-convert >/dev/null 2>&1; then
  rsvg-convert -w 1024 -h 1024 -o "$ICON_DIR/base.png" "$ROOT/resources/icon.svg"
else
  qlmanage -t -s 1024 -o "$ICON_DIR" "$ROOT/resources/icon.svg" >/dev/null
  mv "$ICON_DIR/icon.svg.png" "$ICON_DIR/base.png"
fi
[[ -s "$ICON_DIR/base.png" ]] || { echo "could not rasterize resources/icon.svg"; exit 1; }

ICONSET="$ICON_DIR/AppIcon.iconset"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$ICON_DIR/base.png" --out "$ICONSET/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" "$ICON_DIR/base.png" --out "$ICONSET/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$ICON_DIR/AppIcon.icns"

# 3. Assemble the bundle.
mkdir -p "$DIST_APP/Contents/MacOS" "$DIST_APP/Contents/Resources"
cp "$ICON_DIR/AppIcon.icns" "$DIST_APP/Contents/Resources/AppIcon.icns"
cp "$BUILD_DIR/hjson" "$DIST_APP/Contents/MacOS/$APP_NAME"

cat > "$DIST_APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$APP_NAME</string>
  <key>CFBundleDisplayName</key><string>HJson — JSON Viewer</string>
  <key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
  <key>CFBundleExecutable</key><string>$APP_NAME</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>LSMinimumSystemVersion</key><string>10.15</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
PLIST

# Ad-hoc sign so Gatekeeper is satisfied for local use.
codesign --force --sign - "$DIST_APP" >/dev/null 2>&1 || true

echo "Built $DIST_APP  (open it to launch:  open '$DIST_APP')"
