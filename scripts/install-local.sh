#!/usr/bin/env bash
# Собирает Nook, переподписывает локальным сертификатом и ставит в /Applications.
set -euo pipefail

cd "$(dirname "$0")/.."

# HARDCODE: временно, убрать в SPRINT_07 — локальный самоподписанный сертификат вместо Developer ID.
# TODO(SPRINT_07): удалить этот скрипт и сертификат «Nook Local Dev» из связки ключей.
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
