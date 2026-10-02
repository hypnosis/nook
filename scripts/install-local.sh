#!/usr/bin/env bash
# Собирает Nook и ставит в /Applications с той же ad-hoc подписью, что у релиза.
set -euo pipefail

cd "$(dirname "$0")/.."

./bundle.sh

osascript -e 'quit app "Nook"' || true
sleep 1
pkill -x nook || true
rm -rf /Applications/Nook.app
cp -R Nook.app /Applications/Nook.app
open /Applications/Nook.app
echo "==> Installed and launched /Applications/Nook.app"
