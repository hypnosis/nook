#!/usr/bin/env bash
set -euo pipefail

APP_NAME="clearbar"
APP_DIR="${APP_NAME}.app"
BINARY_NAME="${APP_NAME}"

echo "==> Building ${APP_NAME}..."
cargo build

echo "==> Creating .app bundle structure..."
mkdir -p "${APP_DIR}/Contents/MacOS"

echo "==> Copying binary..."
cp "target/debug/${BINARY_NAME}" "${APP_DIR}/Contents/MacOS/${BINARY_NAME}"

echo "==> Writing Info.plist..."
cat > "${APP_DIR}/Contents/Info.plist" << 'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>com.danila.clearbar</string>
    <key>CFBundleName</key>
    <string>clearbar</string>
    <key>CFBundleExecutable</key>
    <string>clearbar</string>
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

echo "==> Done: ${APP_DIR}"
