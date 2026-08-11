"""Fill interior transparent/half-alpha pixels across all pet animation frames.

Keeps silhouette antialiasing intact, but makes internal mouth/nose/muzzle
details opaque so they look the same on light and dark desktops.

Run:  python tools/fix_animation_mouths.py
"""

from __future__ import annotations

from collections import deque
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "assets" / "pets" / "cow-cat"
SKIP_PARTS = {"_master", "_video"}


def nearest_rgb(
    arr: np.ndarray, x: int, y: int, ys: np.ndarray, xs: np.ndarray
):
    if len(ys) == 0:
        return None
    dx = xs.astype(np.int32) - x
    dy = ys.astype(np.int32) - y
    j = int(np.argmin(dx * dx + dy * dy))
    return tuple(int(v) for v in arr[ys[j], xs[j], :3])


def border_background(mask: np.ndarray) -> np.ndarray:
    h, w = mask.shape
    seen = np.zeros_like(mask, dtype=bool)
    q: deque[tuple[int, int]] = deque()
    for x in range(w):
        for y in (0, h - 1):
            if not mask[y, x] and not seen[y, x]:
                seen[y, x] = True
                q.append((x, y))
    for y in range(h):
        for x in (0, w - 1):
            if not mask[y, x] and not seen[y, x]:
                seen[y, x] = True
                q.append((x, y))
    while q:
        x, y = q.popleft()
        for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            nx, ny = x + dx, y + dy
            if 0 <= nx < w and 0 <= ny < h and not mask[ny, nx] and not seen[ny, nx]:
                seen[ny, nx] = True
                q.append((nx, ny))
    return seen


def repair_frame(arr: np.ndarray) -> Image.Image:
    out = arr.copy()
    a = out[:, :, 3]
    solid = a > 20
    bg = border_background(solid)

    holes = (~solid) & (~bg)
    hy, hx = np.where(holes)
    oy, ox = np.where(out[:, :, 3] >= 250)
    for y, x in zip(hy.tolist(), hx.tolist()):
        col = nearest_rgb(out, x, y, oy, ox)
        if col is not None:
            out[y, x, :3] = col
        out[y, x, 3] = 255

    partial = (a > 0) & (a < 255)
    pad = np.pad(solid, 1, constant_values=False)
    neighbor_solid = (
        pad[:-2, 1:-1]
        + pad[2:, 1:-1]
        + pad[1:-1, :-2]
        + pad[1:-1, 2:]
        + pad[:-2, :-2]
        + pad[:-2, 2:]
        + pad[2:, :-2]
        + pad[2:, 2:]
    )
    interior_partial = partial & (neighbor_solid >= 5)
    py, px = np.where(interior_partial)
    for y, x in zip(py.tolist(), px.tolist()):
        col = nearest_rgb(out, x, y, oy, ox) if float(out[y, x, :3].mean()) < 145 else None
        if col is not None:
            out[y, x, :3] = col
        out[y, x, 3] = 255

    return Image.fromarray(out, "RGBA")


def main() -> None:
    changed = 0
    files = 0
    for meta in sorted(OUT.glob("*/meta.json")):
        clip = meta.parent
        if any(part in SKIP_PARTS for part in clip.parts):
            continue
        for p in sorted(clip.glob("*.png")):
            files += 1
            arr = np.array(Image.open(p).convert("RGBA"))
            fixed = repair_frame(arr)
            if not np.array_equal(arr, np.array(fixed)):
                fixed.save(p, optimize=True)
                changed += 1
    print(f"fixed {changed}/{files} animation frames")


if __name__ == "__main__":
    main()
