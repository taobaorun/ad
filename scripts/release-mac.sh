#!/usr/bin/env bash
# release-mac.sh — build, sign, notarize, and staple a macOS DMG.
#
# Required env (set in CI as secrets, or in your local shell):
#   APPLE_SIGNING_IDENTITY    e.g. "Developer ID Application: Your Name (TEAMID)"
#   APPLE_API_KEY_ID          e.g. "ABCD1234EF"
#   APPLE_API_ISSUER          e.g. "12345678-1234-1234-1234-1234567890ab"
#   APPLE_API_KEY_PATH        path to AuthKey_*.p8 file (NOT committed)
#
# Usage: pnpm release:mac
set -euo pipefail

echo "==> Verifying credentials"
: "${APPLE_SIGNING_IDENTITY:?APPLE_SIGNING_IDENTITY is required}"
: "${APPLE_API_KEY_ID:?APPLE_API_KEY_ID is required}"
: "${APPLE_API_ISSUER:?APPLE_API_ISSUER is required}"
: "${APPLE_API_KEY_PATH:?APPLE_API_KEY_PATH is required}"

if [ ! -f "$APPLE_API_KEY_PATH" ]; then
  echo "✗ App Store Connect key not found at $APPLE_API_KEY_PATH" >&2
  exit 1
fi

echo "==> Building Tauri bundle"
pnpm tauri build

BUNDLE_DIR="src-tauri/target/release/bundle"
DMG_PATH=$(find "$BUNDLE_DIR/dmg" -name "*.dmg" -maxdepth 2 | head -n 1)
APP_PATH=$(find "$BUNDLE_DIR/macos" -name "*.app" -maxdepth 2 | head -n 1)

if [ -z "$APP_PATH" ] || [ -z "$DMG_PATH" ]; then
  echo "✗ Could not locate .app or .dmg under $BUNDLE_DIR" >&2
  exit 1
fi

echo "==> Verifying signature on $APP_PATH"
codesign --verify --deep --strict --verbose=2 "$APP_PATH"

echo "==> Submitting $DMG_PATH for notarization"
xcrun notarytool submit "$DMG_PATH" \
  --key "$APPLE_API_KEY_PATH" \
  --key-id "$APPLE_API_KEY_ID" \
  --issuer "$APPLE_API_ISSUER" \
  --wait

echo "==> Stapling notarization ticket"
xcrun stapler staple "$DMG_PATH"
xcrun stapler staple "$APP_PATH"

echo "==> Validation"
spctl -a -vvv "$APP_PATH"

echo ""
echo "✓ Release artifact ready: $DMG_PATH"
