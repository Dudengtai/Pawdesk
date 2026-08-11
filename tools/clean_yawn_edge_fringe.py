"""Remove pink/purple chroma fringe on yawn sprites WITHOUT killing pink tongue.

Strategy:
  1) Hard-key only pure solid magenta (#FF00FF-ish).
  2) On silhouette ring (≤2px from transparent), kill semi-transparent purple crumbs.
  3) Replace remaining purple-biased edge pixels with nearest *interior* clean fur RGB.

Interior pink tongue sits away from the outer silhouette, so it is not rewritten.

Usage:
  python tools/clean_yawn_edge_fringe.py assets/pets/cow-cat/idle_cute
"""

from __future__ import annotations

import sys
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]


def dist_to_transparent(alpha: np.ndarray) -> np.ndarray:
    """Chebyshev distance to nearest transparent/border pixel (capped)."""
    h, w = alpha.shape
    opaque = alpha >= 8
    # multi-source BFS
    INF = 99
    dist = np.full((h, w), INF, dtype=np.int16)
    from collections import deque

    q: deque[tuple[int, int]] = deque()
    for y in range(h):
        for x in range(w):
            if not opaque[y, x]:
                dist[y, x] = 0
                q.append((x, y))
    # also treat image border of opaque as boundary-adjacent via transparent outside
    while q:
        x, y = q.popleft()
        d = int(dist[y, x])
        if d >= 3:
            continue
        for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            nx, ny = x + dx, y + dy
            if 0 <= nx < w and 0 <= ny < h and dist[ny, nx] > d + 1:
                dist[ny, nx] = d + 1
                q.append((nx, ny))
    return dist


def purple_score(r: int, g: int, b: int) -> int:
    return int(r) + int(b) - 2 * int(g)


def is_pure_magenta(r: int, g: int, b: int) -> bool:
    if r > 220 and b > 210 and g < 60 and r > g + 120 and b > g + 110:
        return True
    if r > 240 and g < 25 and b > 240:
        return True
    return False


def clean_frame(im: Image.Image) -> Image.Image:
    arr = np.asarray(im.convert("RGBA"), dtype=np.uint8).copy()
    h, w = arr.shape[:2]
    r = arr[:, :, 0].astype(np.int16)
    g = arr[:, :, 1].astype(np.int16)
    b = arr[:, :, 2].astype(np.int16)
    a = arr[:, :, 3]

    # 1) pure magenta key
    hard = (
        (r > 220)
        & (b > 210)
        & (g < 60)
        & (r > g + 120)
        & (b > g + 110)
    ) | ((r > 240) & (g < 25) & (b > 240))
    arr[hard] = (0, 0, 0, 0)
    a = arr[:, :, 3]
    r = arr[:, :, 0].astype(np.int16)
    g = arr[:, :, 1].astype(np.int16)
    b = arr[:, :, 2].astype(np.int16)

    dist = dist_to_transparent(a)
    edge = (a >= 8) & (dist <= 2)
    interior = (a >= 8) & (dist >= 3)

    sc = r + b - 2 * g

    # 2) drop semi-transparent purple crumbs on edge
    crumb = edge & (a < 100) & (sc > 15) & (g < 150)
    arr[crumb] = (0, 0, 0, 0)

    # recompute after kill
    a = arr[:, :, 3]
    r = arr[:, :, 0].astype(np.int16)
    g = arr[:, :, 1].astype(np.int16)
    b = arr[:, :, 2].astype(np.int16)
    dist = dist_to_transparent(a)
    edge = (a >= 8) & (dist <= 2)
    interior = (a >= 8) & (dist >= 3)
    sc = r + b - 2 * g

    # clean interior samples: not purple-biased
    clean_mask = interior & (sc < 22)
    clean_ys, clean_xs = np.where(clean_mask)
    if len(clean_xs) < 20:
        clean_ys, clean_xs = np.where(interior)

    # 3) for purple edge pixels, sample nearest clean interior RGB
    ey, ex = np.where(edge & (sc > 18))
    if len(ex) and len(clean_xs):
        # chunk to keep runtime sane
        pts = np.stack([clean_ys.astype(np.float32), clean_xs.astype(np.float32)], axis=1)
        for y, x in zip(ey.tolist(), ex.tolist()):
            # local search window first
            y0, y1 = max(0, y - 12), min(h, y + 13)
            x0, x1 = max(0, x - 12), min(w, x + 13)
            local = clean_mask[y0:y1, x0:x1]
            if np.any(local):
                ly, lx = np.where(local)
                # nearest in local
                d2 = (ly + y0 - y) ** 2 + (lx + x0 - x) ** 2
                i = int(np.argmin(d2))
                sy, sx = int(ly[i] + y0), int(lx[i] + x0)
            else:
                # fallback global nearest (rare)
                d2 = (clean_ys - y) ** 2 + (clean_xs - x) ** 2
                i = int(np.argmin(d2))
                sy, sx = int(clean_ys[i]), int(clean_xs[i])
            arr[y, x, 0] = arr[sy, sx, 0]
            arr[y, x, 1] = arr[sy, sx, 1]
            arr[y, x, 2] = arr[sy, sx, 2]
            # keep alpha but soft if still odd
            if int(arr[y, x, 3]) < 40:
                arr[y, x, 3] = 0

    # 4) final: any remaining edge with strong purple → transparent
    a = arr[:, :, 3]
    r = arr[:, :, 0].astype(np.int16)
    g = arr[:, :, 1].astype(np.int16)
    b = arr[:, :, 2].astype(np.int16)
    dist = dist_to_transparent(a)
    edge = (a >= 8) & (dist <= 1)
    sc = r + b - 2 * g
    kill = edge & (sc > 40) & (g < 100)
    arr[kill] = (0, 0, 0, 0)

    return Image.fromarray(arr, "RGBA")


def main() -> None:
    roots = [Path(p) for p in sys.argv[1:]] if len(sys.argv) > 1 else []
    if not roots:
        raise SystemExit("usage: clean_yawn_edge_fringe.py <dir>...")
    n = 0
    for root in roots:
        paths = [root] if root.is_file() else sorted(root.glob("*.png"))
        for p in paths:
            if not p.is_file():
                continue
            clean_frame(Image.open(p)).save(p)
            n += 1
    print(f"cleaned edge fringe on {n} png(s)")


if __name__ == "__main__":
    main()
