"""Pack cow-cat head portrait → assets/tray/icon.png.

Reads assets/tray/_gen/head_portrait.jpg (magenta-keyed head),
keys the plate, crops tight, and composites a 64×64 circular avatar
that stays readable on light and dark taskbars.

Usage:
  python tools/pack_tray_icon.py
"""

from __future__ import annotations

from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "assets" / "tray" / "_gen" / "head_portrait.jpg"
OUT = ROOT / "assets" / "tray" / "icon.png"
SRC_PNG = ROOT / "assets" / "tray" / "icon_source.png"
PREVIEW = ROOT / "assets" / "tray" / "_gen" / "preview.png"

SIZE = 64
PLATE = (255, 248, 245, 255)  # #FFF8F5, same family as the old tray plate
RING = (28, 25, 23, 70)


def is_magenta(r: int, g: int, b: int) -> bool:
    if r > 160 and b > 150 and g < 180 and r > g + 40 and b > g + 40:
        return True
    if r > 200 and g < 130 and b > 180:
        return True
    if r > 180 and b > 160 and g < 120:
        return True
    # Hot pink plate used on this portrait (#FF00FF-ish).
    if r > 200 and b > 180 and g < 90:
        return True
    return False


def key_magenta(img: Image.Image) -> Image.Image:
    arr = np.array(img.convert("RGBA"), dtype=np.uint8)
    r, g, b = arr[:, :, 0].astype(np.int16), arr[:, :, 1].astype(np.int16), arr[:, :, 2].astype(np.int16)
    mag = (
        ((r > 160) & (b > 150) & (g < 180) & (r > g + 40) & (b > g + 40))
        | ((r > 200) & (g < 130) & (b > 180))
        | ((r > 180) & (b > 160) & (g < 120))
        | ((r > 200) & (b > 180) & (g < 90))
    )
    # Soft key: leftover pink fringe (high R+B, low G) near already-keyed pixels.
    spill = (r + b - 2 * g > 80) & (g < 140) & (r > 80) & (b > 60)
    arr[mag, 3] = 0
    # Knock down fringe alpha instead of leaving a magenta halo.
    fringe = spill & ~mag
    arr[fringe, 3] = np.minimum(arr[fringe, 3], np.uint8(24))
    # Despill remaining edge: pull toward luminance of nearby fur.
    alpha = arr[:, :, 3]
    edge = (alpha > 20) & (alpha < 250) & (r + b - 2 * g > 40)
    if edge.any():
        gray = np.clip((r + g + b) // 3, 0, 255).astype(np.uint8)
        arr[edge, 0] = gray[edge]
        arr[edge, 1] = gray[edge]
        arr[edge, 2] = gray[edge]
    arr[arr[:, :, 3] < 12] = (0, 0, 0, 0)
    return Image.fromarray(arr, "RGBA")


def tight_crop(img: Image.Image, pad_frac: float = 0.04) -> Image.Image:
    a = np.array(img)[:, :, 3]
    ys, xs = np.where(a > 16)
    if len(xs) == 0:
        return img
    x0, x1 = int(xs.min()), int(xs.max()) + 1
    y0, y1 = int(ys.min()), int(ys.max()) + 1
    w, h = x1 - x0, y1 - y0
    pad = int(max(w, h) * pad_frac)
    x0 = max(0, x0 - pad)
    y0 = max(0, y0 - pad)
    x1 = min(img.width, x1 + pad)
    y1 = min(img.height, y1 + pad)
    return img.crop((x0, y0, x1, y1))


def circular_plate(size: int) -> Image.Image:
    plate = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    draw = ImageDraw.Draw(plate)
    inset = 1
    draw.ellipse((inset, inset, size - 1 - inset, size - 1 - inset), fill=PLATE)
    # Soft inner ring so the plate reads on both light and dark bars.
    draw.ellipse(
        (inset, inset, size - 1 - inset, size - 1 - inset),
        outline=RING,
        width=max(1, size // 32),
    )
    return plate


def compose_icon(head: Image.Image, size: int = SIZE) -> Image.Image:
    # Work at 4× then downscale for clean AA.
    hi = size * 4
    canvas = Image.new("RGBA", (hi, hi), (0, 0, 0, 0))
    plate = circular_plate(hi)
    canvas.alpha_composite(plate)

    # Head fills most of the plate; ears may kiss the rim.
    target = int(hi * 0.94)
    hw, hh = head.size
    scale = target / max(hw, hh)
    nw, nh = max(1, int(hw * scale)), max(1, int(hh * scale))
    resized = head.resize((nw, nh), Image.Resampling.LANCZOS)
    ox = (hi - nw) // 2
    # Bias slightly down so ears have room and the face sits in the optical center.
    oy = (hi - nh) // 2 + int(hi * 0.02)
    canvas.alpha_composite(resized, (ox, oy))

    # Clip anything far outside the circle so stray whiskers don't look like dirt
    # at 16px, but keep a few pixels of overflow for the ear tips.
    mask = Image.new("L", (hi, hi), 0)
    md = ImageDraw.Draw(mask)
    overflow = int(hi * 0.02)
    md.ellipse((-overflow, -overflow, hi - 1 + overflow, hi - 1 + overflow), fill=255)
    mask = mask.filter(ImageFilter.GaussianBlur(radius=hi * 0.01))
    out = Image.new("RGBA", (hi, hi), (0, 0, 0, 0))
    out.paste(canvas, (0, 0))
    rgba = np.array(out)
    ma = np.array(mask)
    rgba[:, :, 3] = (rgba[:, :, 3].astype(np.uint16) * ma.astype(np.uint16) // 255).astype(np.uint8)
    out = Image.fromarray(rgba, "RGBA")
    return out.resize((size, size), Image.Resampling.LANCZOS)


def preview_sheet(icon: Image.Image) -> Image.Image:
    """16 / 32 / 64 on light and dark bars, for visual QA."""
    sizes = (16, 32, 64)
    cell = 80
    rows = 2
    cols = len(sizes)
    sheet = Image.new("RGBA", (cell * cols, cell * rows), (0, 0, 0, 0))
    bgs = ((240, 240, 240, 255), (32, 32, 32, 255))
    for r, bg in enumerate(bgs):
        for c, s in enumerate(sizes):
            tile = Image.new("RGBA", (cell, cell), bg)
            scaled = icon.resize((s, s), Image.Resampling.LANCZOS)
            tile.alpha_composite(scaled, ((cell - s) // 2, (cell - s) // 2))
            sheet.paste(tile, (c * cell, r * cell))
    return sheet


def main() -> None:
    if not SRC.exists():
        raise SystemExit(f"missing source: {SRC}")
    keyed = key_magenta(Image.open(SRC))
    cropped = tight_crop(keyed)
    SRC_PNG.parent.mkdir(parents=True, exist_ok=True)
    cropped.save(SRC_PNG)
    icon = compose_icon(cropped, SIZE)
    icon.save(OUT)
    preview_sheet(icon).save(PREVIEW)
    print(f"wrote {OUT} {icon.size}  source={SRC_PNG.name}  preview={PREVIEW}")


if __name__ == "__main__":
    main()
