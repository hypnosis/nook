#!/usr/bin/env python3
"""Рисует фон окна DMG: тёплый градиент в тон иконки Nook
и короткую стрелку, запечённую строго между иконками (app -> Applications).

Стрелка — часть фоновой картинки, привязана к точечным координатам иконок
(см. make-dmg.sh), поэтому не зависит от того, как Finder отрисует окно.

Окно DMG: WIN_W x WIN_H pt. Фон рисуем в @2x для чёткости на ретине.
Иконки (центры, pt): app (APP_X, ICON_Y), Applications (APPLI_X, ICON_Y).
"""
from PIL import Image, ImageDraw

SCALE = 2

# --- Геометрия окна и иконок (в точках) — ДОЛЖНА совпадать с make-dmg.sh ---
WIN_W, WIN_H = 660, 400
APP_X = 180        # центр иконки приложения по X
APPLI_X = 480      # центр иконки Applications по X
ICON_Y = 195       # центр обеих иконок по Y
ICON_HALF = 64     # половина иконки 128pt

# Палитра в тон иконки (тёплый песочный -> терракот)
TOP = (245, 226, 200)
BOTTOM = (224, 170, 124)

W, H = WIN_W * SCALE, WIN_H * SCALE
img = Image.new("RGB", (W, H), TOP)
draw = ImageDraw.Draw(img)

# Вертикальный градиент сверху вниз
for y in range(H):
    t = y / (H - 1)
    r = round(TOP[0] + (BOTTOM[0] - TOP[0]) * t)
    g = round(TOP[1] + (BOTTOM[1] - TOP[1]) * t)
    b = round(TOP[2] + (BOTTOM[2] - TOP[2]) * t)
    draw.line([(0, y), (W, y)], fill=(r, g, b))

# Стрелка запекается явными координатами в пикселях фона (@2x).
# Подобрано под фактическую раскладку окна: между иконками, на их уровне.
# Центр стрелки в точках ~ (ARROW_CX, ARROW_CY); переводим в пиксели *SCALE.
ARROW_CX = 160   # X центра стрелки (pt) — в зазоре между nook.app и Applications
ARROW_CY = 100   # Y центра стрелки (pt) — чуть выше центра иконок
half_len = 34    # половина длины стрелки (pt)
x_start = (ARROW_CX - half_len) * SCALE
x_end = (ARROW_CX + half_len) * SCALE
y_arrow = ARROW_CY * SCALE

arrow_color = (120, 80, 55)           # тёмный терракот
shaft_w = 4 * SCALE
head = 12 * SCALE
draw.line([(x_start, y_arrow), (x_end - head, y_arrow)],
          fill=arrow_color, width=shaft_w)
draw.polygon([
    (x_end, y_arrow),
    (x_end - head, y_arrow - head * 0.65),
    (x_end - head, y_arrow + head * 0.65),
], fill=arrow_color)

img.save("assets/dmg-bg.png")
print("==> Готово: assets/dmg-bg.png", img.size,
      f"(стрелка центр X={ARROW_CX} Y={ARROW_CY} pt)")
