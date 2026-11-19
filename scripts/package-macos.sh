#!/usr/bin/env bash
# Build dist/GlintVNC.app and dist/GlintVNC-<version>-macOS-x86_64.dmg
# Usage: bash scripts/package-macos.sh [--sign "Developer ID Application: ..."]
# Without --sign the bundle is ad-hoc signed (local use only, NOT distributable).
set -euo pipefail
cd "$(dirname "$0")/.."
[[ "$(uname -s)" == "Darwin" ]] || { echo 'macOS only' >&2; exit 2; }

SIGN_IDENTITY=""
if [[ "${1:-}" == "--sign" ]]; then
  SIGN_IDENTITY="${2:?missing identity after --sign}"
fi

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
[[ -n "$VERSION" ]] || { echo 'could not read workspace version' >&2; exit 2; }

echo '== Building release binary (x86_64-apple-darwin) =='
MACOSX_DEPLOYMENT_TARGET=13.0 cargo build --release --target x86_64-apple-darwin -p glintvnc-app

BUILD_OUT=target/x86_64-apple-darwin/release/glintvnc
[[ -f "$BUILD_OUT" ]] || { echo "missing $BUILD_OUT" >&2; exit 2; }

echo '== Assembling app bundle =='
DIST=dist
APP="$DIST/GlintVNC.app"
STAGE="$DIST/dmg-staging"
rm -rf "$APP" "$STAGE"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources" "$STAGE"
cp "$BUILD_OUT" "$APP/Contents/MacOS/glintvnc"
chmod +x "$APP/Contents/MacOS/glintvnc"
cp assets/Info.plist "$APP/Contents/Info.plist"
# Keep plist version in sync with the workspace version.
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $VERSION" "$APP/Contents/Info.plist" 2>/dev/null || true
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $VERSION" "$APP/Contents/Info.plist" 2>/dev/null || true
if [[ -f assets/GlintVNC.icns ]]; then
  cp assets/GlintVNC.icns "$APP/Contents/Resources/GlintVNC.icns"
  /usr/libexec/PlistBuddy -c "Add :CFBundleIconFile string GlintVNC" "$APP/Contents/Info.plist" 2>/dev/null || \
  /usr/libexec/PlistBuddy -c "Set :CFBundleIconFile GlintVNC" "$APP/Contents/Info.plist" 2>/dev/null || true
fi
printf 'APPL????' > "$APP/Contents/PkgInfo"

echo '== Signing =='
if [[ -n "$SIGN_IDENTITY" ]]; then
  codesign --force --deep --options runtime --sign "$SIGN_IDENTITY" "$APP"
  codesign --verify --deep --strict --verbose=2 "$APP"
  echo "Signed with: $SIGN_IDENTITY"
else
  codesign --force --deep --sign - "$APP"
  echo 'AD-HOC SIGNED: local use only. For distribution, sign with a Developer ID and notarize.'
fi

echo '== Creating DMG =='
DMG="$DIST/GlintVNC-${VERSION}-macOS-x86_64.dmg"
cp -R "$APP" "$STAGE/GlintVNC.app"
ln -s /Applications "$STAGE/Applications"
rm -f "$DMG"
hdiutil create -volname "GlintVNC" -srcfolder "$STAGE" -ov -format UDZO "$DMG" >/dev/null
rm -rf "$STAGE"

shasum -a 256 "$DMG"
echo "Created: $DMG"
echo 'NOTE: without a Developer ID signature + notarization, Gatekeeper will block first launch (right-click > Open).'
