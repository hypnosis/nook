#!/usr/bin/env bash
set -euo pipefail

# Собирает Nook.icns из assets/icon.png по гайдам Apple.
# Все размеры iconset (16..512, @1x и @2x) — требование iconutil.

SRC="assets/icon.png"
ICONSET="assets/Nook.iconset"
OUT="assets/Nook.icns"

if [[ ! -f "${SRC}" ]]; then
    echo "ERROR: ${SRC} не найден" >&2
    exit 1
fi

echo "==> Очистка iconset..."
rm -rf "${ICONSET}"
mkdir -p "${ICONSET}"

echo "==> Генерация размеров из ${SRC}..."
# имя_файла : сторона в пикселях
sizes=(
    "icon_16x16.png:16"
    "icon_16x16@2x.png:32"
    "icon_32x32.png:32"
    "icon_32x32@2x.png:64"
    "icon_128x128.png:128"
    "icon_128x128@2x.png:256"
    "icon_256x256.png:256"
    "icon_256x256@2x.png:512"
    "icon_512x512.png:512"
    "icon_512x512@2x.png:1024"
)

for entry in "${sizes[@]}"; do
    name="${entry%%:*}"
    px="${entry##*:}"
    sips -z "${px}" "${px}" "${SRC}" --out "${ICONSET}/${name}" >/dev/null
done

echo "==> Упаковка в ${OUT}..."
iconutil -c icns "${ICONSET}" -o "${OUT}"

echo "==> Очистка временного iconset..."
rm -rf "${ICONSET}"

echo "==> Готово: ${OUT}"
