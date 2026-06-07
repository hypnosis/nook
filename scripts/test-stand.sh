#!/usr/bin/env bash
# Тестовый стенд для диагностики координат статус-айтемов.
# Собирает release → .app → N запусков → парсит лог → статистика OK/BAD.
# Вердикт по координатам (Screen Recording не нужен).
#
# Usage: ./scripts/test-stand.sh [N]   (N запусков, по умолчанию 10)

set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."

N="${1:-10}"
APP="Nook.app"
LOG="/tmp/nook-debug.log"

pkill -i nook 2>/dev/null; sleep 1

echo "==> Сборка release..."
cargo build --release 2>&1 | tail -1 || { echo "СБОРКА УПАЛА"; exit 1; }

echo "==> Заворачиваю в ${APP}..."
rm -rf "$APP"; mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/nook "$APP/Contents/MacOS/nook"
cp assets/Nook.icns "$APP/Contents/Resources/Nook.icns" 2>/dev/null || true
cat > "$APP/Contents/Info.plist" << 'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.hypnosis.nook</string>
<key>CFBundleName</key><string>Nook</string>
<key>CFBundleExecutable</key><string>nook</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>LSUIElement</key><true/>
</dict></plist>
PLIST
codesign -s - --force "$APP" >/dev/null 2>&1

echo "==> ${N} запусков..."
ok=0; bad=0
for i in $(seq 1 "$N"); do
  pkill -i nook 2>/dev/null; sleep 1.2
  rm -f "$LOG"
  open "$APP"
  sleep 4
  # Берём строку guard: (координаты на момент первого toggle — финальное состояние).
  # Если её нет — последнюю placement: с координатами вида spacer.x=...
  line=$(grep -E "guard:" "$LOG" 2>/dev/null | tail -1)
  [ -z "$line" ] && line=$(grep -E "placement: spacer.x=" "$LOG" 2>/dev/null | tail -1)
  sx=$(echo "$line" | grep -oE "spacer.x=(Some\()?[0-9.]+" | grep -oE "[0-9]+" | head -1)
  ax=$(echo "$line" | grep -oE "anchor.x=(Some\()?[0-9.]+" | grep -oE "[0-9]+" | head -1)
  sx="${sx:-NIL}"; ax="${ax:-NIL}"
  # OK = оба > 0 и спейсер строго левее якоря
  if [ "$sx" != "NIL" ] && [ "$ax" != "NIL" ] && [ "$sx" != "0" ] && [ "$ax" != "0" ] && [ "$sx" -lt "$ax" ]; then
    ok=$((ok+1)); verdict="OK"
  else
    bad=$((bad+1)); verdict="BAD"
  fi
  printf "  %2d: spacer=%-6s anchor=%-6s -> %s\n" "$i" "$sx" "$ax" "$verdict"
done
pkill -i nook 2>/dev/null

echo ""
echo "==================================="
echo " ИТОГ: OK=$ok  BAD=$bad  из $N  ($(( ok * 100 / N ))% OK)"
echo "==================================="
