#!/usr/bin/env bash
set -euo pipefail

APP_NAME="nook"
APP_DIR="${APP_NAME}.app"
BINARY_NAME="${APP_NAME}"
VERSION="0.1.0"

DMG_NAME="${APP_NAME}-${VERSION}.dmg"
VOL_NAME="${APP_NAME}"
STAGING_DIR="dmg-staging"

echo "==> Building release ${APP_NAME}..."
cargo build --release

echo "==> Creating .app bundle structure..."
rm -rf "${APP_DIR}"
mkdir -p "${APP_DIR}/Contents/MacOS"

echo "==> Copying release binary..."
cp "target/release/${BINARY_NAME}" "${APP_DIR}/Contents/MacOS/${BINARY_NAME}"

echo "==> Writing Info.plist..."
cat > "${APP_DIR}/Contents/Info.plist" << 'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>com.danila.nook</string>
    <key>CFBundleName</key>
    <string>nook</string>
    <key>CFBundleExecutable</key>
    <string>nook</string>
    <key>CFBundleVersion</key>
    <string>0.1.0</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>LSUIElement</key>
    <true/>
    <key>LSMinimumSystemVersion</key>
    <string>26.0</string>
</dict>
</plist>
PLIST

echo "==> Ad-hoc signing..."
codesign -s - --force --deep "${APP_DIR}"

echo "==> Preparing DMG staging dir..."
rm -rf "${STAGING_DIR}"
rm -f "${DMG_NAME}"
mkdir -p "${STAGING_DIR}"
cp -R "${APP_DIR}" "${STAGING_DIR}/${APP_DIR}"
ln -s /Applications "${STAGING_DIR}/Applications"

echo "==> Building DMG: ${DMG_NAME}..."
hdiutil create \
    -volname "${VOL_NAME}" \
    -srcfolder "${STAGING_DIR}" \
    -fs HFS+ \
    -format UDZO \
    -ov \
    "${DMG_NAME}"

echo "==> Cleaning up staging dir..."
rm -rf "${STAGING_DIR}"

echo "==> Done: ${DMG_NAME}"
