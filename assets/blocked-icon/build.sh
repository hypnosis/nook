#!/usr/bin/env bash
# Пересобирает blocked-иконку (< + ! в треугольнике, template @2x).
# Шаги: swift рендерит два РЕАЛЬНЫХ SF Symbols → python склеивает с halo-вырезом.
# Требует: swift (система), python с Pillow. Результат: blocked.png (@2x) рядом.
set -euo pipefail
cd "$(dirname "$0")"

PT=15  # point size символов (типичный menu bar)

echo "==> render SF Symbols via swift"
swift render_symbol.swift "chevron.left" "$PT" chevron.png
swift render_symbol.swift "exclamationmark.triangle" "$PT" warn.png

echo "==> merge via python (Pillow)"
PY="${NOOK_PY:-python3}"
"$PY" merge_icons.py

echo "==> done: blocked.png"
