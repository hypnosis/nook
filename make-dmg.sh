#!/usr/bin/env bash
set -euo pipefail

# Brand name (capitalized, per Apple HIG) — used for the bundle, display name and DMG volume.
APP_NAME="Nook"
# Binary / file slug (lowercase) — cargo binary name and download file names.
SLUG="nook"
APP_DIR="${APP_NAME}.app"
BINARY_NAME="${SLUG}"
VERSION="0.1.0"

DMG_NAME="${SLUG}-${VERSION}.dmg"
VOL_NAME="${APP_NAME}"

echo "==> Building release ${APP_NAME}..."
cargo build --release

echo "==> Creating .app bundle structure..."
rm -rf "${APP_DIR}"
mkdir -p "${APP_DIR}/Contents/MacOS"
mkdir -p "${APP_DIR}/Contents/Resources"

echo "==> Copying release binary..."
cp "target/release/${BINARY_NAME}" "${APP_DIR}/Contents/MacOS/${BINARY_NAME}"

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

echo "==> Building styled DMG via create-dmg..."
# create-dmg надёжно убирает тулбар, ставит фон 1:1 и раскладывает иконки.
# Координаты иконок (pt) совпадают с зазором, в который запечена стрелка
# на фоне (assets/dmg-bg.png, генерится assets/make-dmg-bg.py).
rm -f "${DMG_NAME}"
create-dmg \
    --volname "${VOL_NAME}" \
    --volicon "assets/Nook.icns" \
    --background "assets/dmg-bg.png" \
    --window-pos 200 120 \
    --window-size 660 400 \
    --icon-size 128 \
    --icon "${APP_DIR}" 180 195 \
    --app-drop-link 480 195 \
    --no-internet-enable \
    "${DMG_NAME}" \
    "${APP_DIR}"

echo "==> Done: ${DMG_NAME}"
