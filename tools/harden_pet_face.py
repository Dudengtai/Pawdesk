"""Deep face fix: original nose RGB + opaque alpha. Never paint geometric nose.

Run:  python tools/harden_pet_face.py
Then: python tools/build_lively_pet.py
      python tools/fix_animation_mouths.py
      (optional: python tools/pack_stretch_from_video.py, then fix_animation_mouths.py)
"""

from __future__ import annotations

import json
from pathlib import Path

import numpy as np
from PIL import Image, ImageFilter

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "assets" / "pets" / "cow-cat"
MASTER = OUT / "_master" / "base_sit.png"
BAK = OUT / "_master" / "base_sit_pre_facefix.png"
SIZE = 256
FOOT_Y = 224


def solidify(arr: np.ndarray) -> np.ndarray:
    out = arr.copy()
    a = out[:, :, 3]
    solid = a > 20
    pad = np.pad(solid, 1, constant_values=False)
    interior = (
        pad[1:-1, 1:-1]
        & pad[:-2, 1:-1]
        & pad[2:, 1:-1]
        & pad[1:-1, :-2]
        & pad[1:-1, 2:]
    )
    out[interior, 3] = 255
    out[a >= 80, 3] = 255
    return out


def seal_muzzle_alpha(arr: np.ndarray) -> np.ndarray:
    """Force muzzle region fully opaque — keep original RGB (natural nose)."""
    out = arr.copy()
    r, g, b, a = out[:, :, 0], out[:, :, 1], out[:, :, 2], out[:, :, 3]
    iris = (
        (a > 200)
        & (g > 130)
        & (r > 85)
        & (b < 140)
        & (g >= r - 30)
        & (g > b + 15)
    )
    iris[:50, :] = False
    iris[125:, :] = False
    ys, xs = np.where(iris)
    if len(xs) < 12:
        cx, cy = 128.0, 90.0
    else:
        cx, cy = float(xs.mean()), float(ys.mean())
    yy, xx = np.mgrid[:SIZE, :SIZE]
    muzzle = ((xx - cx) / 16.0) ** 2 + ((yy - (cy + 16)) / 14.0) ** 2 <= 1.0
    muzzle = muzzle & (a > 40)
    out[muzzle, 3] = 255
    return out


def nearest_rgb(arr: np.ndarray, x: int, y: int, ys: np.ndarray, xs: np.ndarray):
    if len(ys) == 0:
        return None
    dx = xs.astype(np.int32) - x
    dy = ys.astype(np.int32) - y
    j = int(np.argmin(dx * dx + dy * dy))
    return tuple(int(v) for v in arr[ys[j], xs[j], :3])


def _dilate(mask: np.ndarray, radius: int) -> np.ndarray:
    size = radius * 2 + 1
    return np.asarray(Image.fromarray((mask.astype(np.uint8) * 255), "L").filter(ImageFilter.MaxFilter(size))) > 127


def _erode(mask: np.ndarray, radius: int) -> np.ndarray:
    size = radius * 2 + 1
    return np.asarray(Image.fromarray((mask.astype(np.uint8) * 255), "L").filter(ImageFilter.MinFilter(size))) > 127


def repair_muzzle(arr: np.ndarray) -> Image.Image:
    """Fill broken/transparent mouth pixels so the line is opaque on any background."""
    out = arr.copy()
    r, g, b, a = out[:, :, 0], out[:, :, 1], out[:, :, 2], out[:, :, 3]
    iris = (
        (a > 200)
        & (g > 130)
        & (r > 85)
        & (b < 140)
        & (g >= r - 30)
        & (g > b + 15)
    )
    iris[:50, :] = False
    iris[125:, :] = False
    ys, xs = np.where(iris)
    if len(xs) < 12:
        cx, cy = 128.0, 90.0
    else:
        cx, cy = float(xs.mean()), float(ys.mean())

    yy, xx = np.mgrid[:SIZE, :SIZE]
    muzzle = ((xx - cx) / 18.0) ** 2 + ((yy - (cy + 16)) / 14.0) ** 2 <= 1.0
    muzzle |= ((xx - cx) / 24.0) ** 2 + ((yy - (cy + 16)) / 20.0) ** 2 <= 1.0

    luma = out[:, :, :3].mean(axis=2).astype(np.float32)
    line = muzzle & (out[:, :, 3] >= 100) & (luma < 145)
    line |= muzzle & (out[:, :, 3] < 100) & (luma < 100)
    closed_line = _erode(_dilate(line, 2), 2)
    line_grow = closed_line & (out[:, :, 3] < 255) & muzzle

    opaque = np.where(out[:, :, 3] >= 250)
    opaque_luma = luma[opaque]
    dark = opaque[0][opaque_luma < 145]
    dark_x = opaque[1][opaque_luma < 145]
    cream = opaque[0][opaque_luma >= 145]
    cream_x = opaque[1][opaque_luma >= 145]

    fy, fx = np.where(muzzle & (out[:, :, 3] < 255))
    for y, x in zip(fy.tolist(), fx.tolist()):
        if line_grow[y, x]:
            col = nearest_rgb(arr, x, y, dark, dark_x)
        else:
            own_luma = float(out[y, x, :3].mean())
            use_dark = own_luma < 145 if own_luma > 0 else False
            if not use_dark:
                dark_d = ((dark - y) ** 2 + (dark_x - x) ** 2).min() if len(dark) else 10**9
                cream_d = ((cream - y) ** 2 + (cream_x - x) ** 2).min() if len(cream) else 10**9
                use_dark = dark_d < cream_d
            col = nearest_rgb(arr, x, y, dark, dark_x) if use_dark else nearest_rgb(arr, x, y, cream, cream_x)
        if col is not None:
            out[y, x, :3] = col
        out[y, x, 3] = 255
    return Image.fromarray(out, "RGBA")


def fit_feet(im: Image.Image) -> Image.Image:
    arr = np.array(im)
    ys, xs = np.where(arr[:, :, 3] > 12)
    content = im.crop(
        (int(xs.min()), int(ys.min()), int(xs.max()) + 1, int(ys.max()) + 1)
    )
    max_h, max_w = FOOT_Y - 8, SIZE - 16
    cw, ch = content.size
    sc = min(max_w / cw, max_h / ch, 1.0)
    nw, nh = max(1, int(round(cw * sc))), max(1, int(round(ch * sc)))
    content = content.resize((nw, nh), Image.Resampling.LANCZOS)
    canvas = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    canvas.paste(content, ((SIZE - nw) // 2, FOOT_Y - nh), content)
    fixed = Image.fromarray(seal_muzzle_alpha(solidify(np.array(canvas))))
    return repair_muzzle(np.array(fixed))


def blink_env(u: float) -> float:
    u = max(0.0, min(1.0, u))
    if u < 0.32:
        return (u / 0.32) ** 2
    if u < 0.48:
        return 1.0
    return (1.0 - (u - 0.48) / 0.52) ** 2


def paint_lid(base: Image.Image, amount: float) -> Image.Image:
    amount = max(0.0, min(1.0, amount))
    if amount < 0.02:
        return base.copy()
    a = np.array(base)
    r, g, b, al = a[:, :, 0], a[:, :, 1], a[:, :, 2], a[:, :, 3]
    iris = (
        (al > 200)
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
    out = a.copy()
    for cond in (xs < mid, xs >= mid):
        if not np.any(cond):
            continue
        xx, yy = xs[cond], ys[cond]
        ecy = float(yy.mean())
        ry = float(max(5.5, (yy.max() - yy.min()) * 0.55 + 2))
        for x, y in zip(xx.tolist(), yy.tolist()):
            v = ((y + 0.5 - ecy) / max(ry, 1.0) + 1.0) * 0.5
            if v <= amount * 1.06:
                out[y, x] = (22, 22, 24, 255)
    return Image.fromarray(solidify(out))


def main() -> None:
    if not BAK.is_file():
        raise SystemExit(f"missing {BAK}")
    raw = Image.open(BAK).convert("RGBA")
    if raw.size != (SIZE, SIZE):
        raw = raw.resize((SIZE, SIZE), Image.Resampling.LANCZOS)

    fixed = fit_feet(Image.fromarray(seal_muzzle_alpha(solidify(np.array(raw)))))
    fixed.save(MASTER)
    bg = Image.new("RGBA", fixed.size, (255, 0, 255, 255))
    bg.paste(fixed, (0, 0), fixed)
    bg.convert("RGB").save(OUT / "_master" / "base_sit_magenta.jpg", quality=95)

    dest = OUT / "idle_blink"
    dest.mkdir(parents=True, exist_ok=True)
    for old in dest.glob("*.png"):
        old.unlink()
    files = []
    events = [(0.32, 0.026), (0.40, 0.020), (0.80, 0.028)]
    for i in range(120):
        t = i / 120.0
        amt = 0.0
        for c, h in events:
            if abs(t - c) < h:
                u = (t - (c - h)) / (2 * h)
                amt = max(amt, blink_env(u))
        fr = paint_lid(fixed, amt) if amt > 0.02 else fixed.copy()
        fn = f"{i:03d}.png"
        fr.save(dest / fn, optimize=True)
        files.append(fn)
    (dest / "meta.json").write_text(
        json.dumps(
            {
                "name": "idle_blink",
                "frame_width": SIZE,
                "frame_height": SIZE,
                "frames": 120,
                "fps": 30.0,
                "loop": True,
                "anchor": {"x": 128, "y": FOOT_Y},
                "files": files,
                "source": "original_nose_opaque_v4",
                "notes": "Original painted nose RGB; alpha seal only; no geometric nose",
            },
            indent=2,
            ensure_ascii=False,
        )
        + "\n",
        encoding="utf-8",
    )
    print("idle_blink ok")

    dest = OUT / "idle_watch"
    dest.mkdir(parents=True, exist_ok=True)
    for old in dest.glob("*.png"):
        old.unlink()
    fixed.save(dest / "00.png")
    fixed.save(dest / "01.png")
    (dest / "meta.json").write_text(
        json.dumps(
            {
                "name": "idle_watch",
                "frame_width": SIZE,
                "frame_height": SIZE,
                "frames": 2,
                "fps": 1.0,
                "loop": True,
                "anchor": {"x": 128, "y": FOOT_Y},
                "files": ["00.png", "01.png"],
                "source": "static",
            },
            indent=2,
            ensure_ascii=False,
        )
        + "\n",
        encoding="utf-8",
    )
    print("idle_watch static; code maps Watching → idle_blink")
    print("done")


if __name__ == "__main__":
    main()
