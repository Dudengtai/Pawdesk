"""Remove residual square/magenta backgrounds from pet PNGs.

Preserves white fur: only chroma-keys magenta, then keeps the largest
opaque connected component. Does NOT flood-fill pure white.

Usage:
  python tools/clean_sprite_bg.py
  python tools/clean_sprite_bg.py assets/pets/cow-cat/idle_tail_wag
"""

from __future__ import annotations

import sys
from collections import deque
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
DEFAULT = ROOT / "assets" / "pets" / "cow-cat"


def is_magenta(r: int, g: int, b: int) -> bool:
    if r > 160 and b > 150 and g < 180 and r > g + 40 and b > g + 40:
        return True
    if r > 200 and g < 130 and b > 180:
        return True
    return False


def clean(img: Image.Image) -> Image.Image:
    img = img.convert("RGBA")
    w, h = img.size
    px = img.load()
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if is_magenta(r, g, b):
                px[x, y] = (0, 0, 0, 0)

    mask = [[px[x, y][3] >= 20 for x in range(w)] for y in range(h)]
    seen = [[False] * w for _ in range(h)]
    best: list[tuple[int, int]] = []
    for y in range(h):
        for x in range(w):
            if not mask[y][x] or seen[y][x]:
                continue
            comp: list[tuple[int, int]] = []
            dq = deque([(x, y)])
            seen[y][x] = True
            while dq:
                cx, cy = dq.popleft()
                comp.append((cx, cy))
                for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    nx, ny = cx + dx, cy + dy
                    if 0 <= nx < w and 0 <= ny < h and not seen[ny][nx] and mask[ny][nx]:
                        seen[ny][nx] = True
                        dq.append((nx, ny))
            if len(comp) > len(best):
                best = comp
    keep = set(best)
    for y in range(h):
        for x in range(w):
            if (x, y) not in keep:
                px[x, y] = (0, 0, 0, 0)
    return img


def main() -> None:
    if len(sys.argv) > 1:
        roots = [Path(p) for p in sys.argv[1:]]
    else:
        roots = [DEFAULT]
    n = 0
    for root in roots:
        paths = [root] if root.is_file() else sorted(root.rglob("*.png"))
        for p in paths:
            if not p.is_file():
                continue
            clean(Image.open(p)).save(p)
            n += 1
    print(f"cleaned {n} png(s)")


if __name__ == "__main__":
    main()
