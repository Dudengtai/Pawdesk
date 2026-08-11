"""Fix pet face dirt + feet margin; rebuild idle_blink from master.

Steps:
  1) Subtle muzzle clean (no big repainted nose)
  2) Scale content so paws sit at FOOT_Y with ~32px bottom margin
  3) Rebuild idle_blink from fixed master

Run:  python tools/fix_pet_face_and_feet.py
Then: python tools/pack_stretch_from_video.py  # bookend sit matches master
"""

from __future__ import annotations

import json
import math
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "assets" / "pets" / "cow-cat"
MASTER = OUT / "_master" / "base_sit.png"
MASTER_BAK = OUT / "_master" / "base_sit_pre_facefix.png"
SIZE = 256
FOOT_Y = 224
FPS = 30.0
IDLE_FRAMES = 120


def load_master() -> Image.Image:
    src = MASTER_BAK if MASTER_BAK.is_file() else MASTER
    im = Image.open(src).convert("RGBA")
    if im.size != (SIZE, SIZE):
        im = im.resize((SIZE, SIZE), Image.Resampling.LANCZOS)
    return im


def clean_muzzle(im: Image.Image) -> Image.Image:
    """Fill isolated dark dirt on cream muzzle only — keep original nose."""
    arr = np.array(im, dtype=np.uint8)
    out = arr.copy()
    r, g, b, a = out[:, :, 0], out[:, :, 1], out[:, :, 2], out[:, :, 3]
    luma = (r.astype(np.int16) + g.astype(np.int16) + b.astype(np.int16)) / 3
    iris = (
        (a > 200)
        & (g > 130)
        & (r > 85)
        & (b < 140)
        & (g >= r - 30)
        & (g > b + 15)
    )
    iris[:55, :] = False
    iris[120:, :] = False
    ys, xs = np.where(iris)
    if len(xs) < 20:
        cx, cy = 128.0, 100.0
    else:
        cx, cy = float(xs.mean()), float(ys.mean())
    nose_cy = cy + 20.0
    yy, xx = np.ogrid[:SIZE, :SIZE]
    muzzle = ((xx - cx) / 18.0) ** 2 + ((yy - nose_cy) / 14.0) ** 2 <= 1.0
    sample = muzzle & (a > 220) & (luma > 205)
    cream = (
        out[sample][:, :3].mean(axis=0).astype(np.float32)
        if sample.sum() > 10
        else np.array([248.0, 242.0, 228.0], dtype=np.float32)
    )
    pinkish = (r > 180) & (g > 120) & (g < 210) & (b > 120) & (b < 210) & (r > g)
    dirty = muzzle & (a > 180) & (luma < 85) & (~pinkish) & (xx > cx - 12)
    if dirty.any():
        noise = (np.random.default_rng(2).random(out[dirty, :3].shape) - 0.5) * 3.0
        out[dirty, 0] = np.clip(cream[0] + noise[:, 0], 0, 255).astype(np.uint8)
        out[dirty, 1] = np.clip(cream[1] + noise[:, 1], 0, 255).astype(np.uint8)
        out[dirty, 2] = np.clip(cream[2] + noise[:, 2], 0, 255).astype(np.uint8)
        out[dirty, 3] = 255
    out[out[:, :, 3] >= 210, 3] = 255
    return Image.fromarray(out, "RGBA")


def ensure_bottom_margin(im: Image.Image, foot_y: int = FOOT_Y) -> Image.Image:
    arr = np.array(im)
    ys, xs = np.where(arr[:, :, 3] > 12)
    if len(ys) == 0:
        return im
    content = im.crop(
        (int(xs.min()), int(ys.min()), int(xs.max()) + 1, int(ys.max()) + 1)
    )
    max_h, max_w = foot_y - 8, SIZE - 16
    cw, ch = content.size
    scale = min(max_w / cw, max_h / ch, 1.0)
    nw, nh = max(1, int(cw * scale)), max(1, int(ch * scale))
    content = content.resize((nw, nh), Image.Resampling.LANCZOS)
    canvas = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    canvas.paste(content, ((SIZE - nw) // 2, foot_y - nh), content)
    return canvas


def paint_lid(base: Image.Image, amount: float) -> Image.Image:
    amount = max(0.0, min(1.0, amount))
    if amount < 0.02:
        return base.copy()
    arr = np.array(base)
    r, g, b, a = arr[:, :, 0], arr[:, :, 1], arr[:, :, 2], arr[:, :, 3]
    iris = (
        (a > 200)
        & (g > 130)
        & (r > 85)
        & (b < 140)
        & (g >= r - 30)
        & (g > b + 15)
    )
    iris[:55, :] = False
    iris[120:, :] = False
    ys, xs = np.where(iris)
    if len(xs) < 16:
        return base.copy()
    mid = 0.5 * (xs.min() + xs.max())
    out = arr.copy()
    for cond in (xs < mid, xs >= mid):
        if not np.any(cond):
            continue
        xx, yy = xs[cond], ys[cond]
        cx, cy = float(xx.mean()), float(yy.mean())
        rx = float(max(6.0, (xx.max() - xx.min()) * 0.55 + 2))
        ry = float(max(5.5, (yy.max() - yy.min()) * 0.55 + 2))
        for x, y in zip(xx.tolist(), yy.tolist()):
            v = ((y + 0.5 - cy) / max(ry, 1.0) + 1.0) * 0.5
            if v <= amount * 1.05:
                out[y, x, 0], out[y, x, 1], out[y, x, 2], out[y, x, 3] = 18, 18, 20, 255
    return Image.fromarray(out, "RGBA")


def blink_env(u: float) -> float:
    u = max(0.0, min(1.0, u))
    if u < 0.32:
        return (u / 0.32) ** 2
    if u < 0.48:
        return 1.0
    return (1.0 - (u - 0.48) / 0.52) ** 2


def rebuild_idle_blink(base: Image.Image) -> None:
    events = [(0.30, 0.028), (0.38, 0.022), (0.78, 0.030)]
    dest = OUT / "idle_blink"
    dest.mkdir(parents=True, exist_ok=True)
    for old in dest.glob("*.png"):
        old.unlink()
    files = []
    for i in range(IDLE_FRAMES):
        t = i / IDLE_FRAMES
        breath = 0.01 * math.sin(t * math.tau * 2.0)
        w, h = base.size
        sy, sx = 1.0 + breath, 1.0 - breath * 0.45
        nw, nh = max(1, int(w * sx)), max(1, int(h * sy))
        sc = base.resize((nw, nh), Image.Resampling.LANCZOS)
        fr = Image.new("RGBA", (w, h), (0, 0, 0, 0))
        fr.paste(sc, ((w - nw) // 2, FOOT_Y - int(FOOT_Y * sy)), sc)
        amt = 0.0
        for c, half in events:
            if abs(t - c) < half:
                u = (t - (c - half)) / (2 * half)
                amt = max(amt, blink_env(u))
        if amt > 0.02:
            fr = paint_lid(fr, amt)
        fn = f"{i:03d}.png"
        fr.save(dest / fn, optimize=True)
        files.append(fn)
    meta = {
        "name": "idle_blink",
        "frame_width": SIZE,
        "frame_height": SIZE,
        "frames": len(files),
        "fps": FPS,
        "loop": True,
        "anchor": {"x": 128, "y": FOOT_Y},
        "files": files,
        "source": "facefix_v2_subtle",
        "notes": "Subtle muzzle clean + foot margin; no repainted mega-nose",
    }
    (dest / "meta.json").write_text(
        json.dumps(meta, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    print(f"  idle_blink: {len(files)}f")


def main() -> None:
    raw = load_master()
    if not MASTER_BAK.is_file():
        Image.open(MASTER).convert("RGBA").save(MASTER_BAK)
        print(f"backup → {MASTER_BAK.name}")
    fixed = ensure_bottom_margin(clean_muzzle(raw), FOOT_Y)
    fixed.save(MASTER)
    bg = Image.new("RGBA", fixed.size, (255, 0, 255, 255))
    bg.paste(fixed, (0, 0), fixed)
    bg.convert("RGB").save(OUT / "_master" / "base_sit_magenta.jpg", quality=95)
    a = np.array(fixed)
    ys = np.where(a[:, :, 3] > 12)[0]
    print(f"master ok bottom={ys.max()} margin={SIZE - 1 - ys.max()}")
    rebuild_idle_blink(fixed)
    print("done — next: python tools/pack_stretch_from_video.py")


if __name__ == "__main__":
    main()
