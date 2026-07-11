"""Склейка < + ! (в треугольнике) в template-иконку с halo-вырезом.
< слева (реальный chevron.left), ! поверх, наезжает, меньше на 20%.
Halo: вокруг ! прозрачный зазор — стирает линии chevron под треугольником,
чтобы ! читался поверх, не сливаясь. Остаётся template (монохром под тему).
"""
from PIL import Image, ImageFilter, ImageChops

PAD_PT = -6.0          # наезд ! поверх правой части <
WARN_SCALE = 0.80      # ! меньше на 20%
HALO_PX = 3            # ширина прозрачного ореола вокруг ! (в @2x пикселях)
SCALE = 2              # @2x
pad = int(round(PAD_PT * SCALE))  # -12px

chevron = Image.open("chevron.png").convert("RGBA")   # <  (низ)
warn = Image.open("warn.png").convert("RGBA")         # !  (верх)

# ! меньше на 20%
warn = warn.resize(
    (max(1, int(warn.width * WARN_SCALE)), max(1, int(warn.height * WARN_SCALE))),
    Image.Resampling.LANCZOS,
)

W = chevron.width + pad + warn.width
H = max(chevron.height, warn.height)

chev_xy = (0, (H - chevron.height) // 2)
warn_xy = (chevron.width + pad, (H - warn.height) // 2)

# --- halo-маска: альфа ! , расширенная на HALO_PX ---
warn_alpha_full = Image.new("L", (W, H), 0)
warn_alpha_full.paste(warn.split()[3], warn_xy)
halo = warn_alpha_full.filter(ImageFilter.MaxFilter(HALO_PX * 2 + 1))  # dilate

# --- chevron на холсте, из его альфы вычитаем halo (стираем под треугольником) ---
chev_layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
chev_layer.paste(chevron, chev_xy)
r, g, b, a = chev_layer.split()
a = ImageChops.subtract(a, halo)  # там где halo — chevron стёрт
chev_layer = Image.merge("RGBA", (r, g, b, a))

# --- ! поверх ---
canvas = Image.new("RGBA", (W, H), (0, 0, 0, 0))
canvas.alpha_composite(chev_layer)
canvas.alpha_composite(warn, warn_xy)

canvas.save("blocked.png")
print(f"blocked.png: {W}x{H}px  pad={PAD_PT}pt  warn={int(WARN_SCALE*100)}%  halo={HALO_PX}px")
