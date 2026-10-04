#!/usr/bin/env bash
# Собирает Nook, переподписывает локальным сертификатом и ставит в /Applications.
set -euo pipefail

cd "$(dirname "$0")/.."

# HARDCODE: временно, на время разработки — локальный самоподписанный сертификат вместо
# Developer ID, чтобы права macOS переживали пересборку. Перед релизом вернуть ad-hoc (ADR 011).
SIGN_IDENTITY="Nook Local Dev"

./bundle.sh
codesign -s "${SIGN_IDENTITY}" --force --deep Nook.app

osascript -e 'quit app "Nook"' || true
sleep 1
pkill -x nook || true
rm -rf /Applications/Nook.app
cp -R Nook.app /Applications/Nook.app
open /Applications/Nook.app
echo "==> Installed and launched /Applications/Nook.app (signed: ${SIGN_IDENTITY})"
