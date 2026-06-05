#!/usr/bin/env bash
set -euo pipefail

# Brand name (capitalized, per Apple HIG) — bundle and display name.
APP_NAME="Nook"
# Binary slug (lowercase) — cargo binary name.
SLUG="nook"
APP_DIR="${APP_NAME}.app"
BINARY_NAME="${SLUG}"

echo "==> Building ${APP_NAME}..."
cargo build

echo "==> Creating .app bundle structure..."
mkdir -p "${APP_DIR}/Contents/MacOS"
mkdir -p "${APP_DIR}/Contents/Resources"

echo "==> Copying binary..."
cp "target/debug/${BINARY_NAME}" "${APP_DIR}/Contents/MacOS/${BINARY_NAME}"

echo "==> Copying app icon..."
cp "assets/Nook.icns" "${APP_DIR}/Contents/Resources/Nook.icns"

echo "==> Writing Info.plist..."
cat > "${APP_DIR}/Contents/Info.plist" << 'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>com.hypnosis.nook</string>
    <key>CFBundleName</key>
    <string>Nook</string>
    <key>CFBundleDisplayName</key>
    <string>Nook</string>
    <key>CFBundleExecutable</key>
    <string>nook</string>
    <key>CFBundleVersion</key>
    <string>0.1.0</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleIconFile</key>
    <string>Nook</string>
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
