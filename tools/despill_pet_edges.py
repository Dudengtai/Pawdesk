"""Remove magenta/pink chroma-key spill from pet sprite edges.

AI video / image-gen often leaves a purple-pink halo on black fur after
background removal. That halo is fine on a black desktop but looks wrong
(and can make the silhouette hard to read) on light or colored wallpapers.

This script:
  1) Hard-keys remaining bright magenta/pink.
  2) On the silhouette edge, kills pure spill pixels (magenta with no fur).
  3) Despills remaining edge pixels toward nearby clean fur colors.
  4) Softens 1px AA using despilled colors (no purple fringe).

Usage:
  python tools/despill_pet_edges.py
  python tools/despill_pet_edges.py assets/pets/cow-cat/idle_blink
"""

from __future__ import annotations

import sys
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
DEFAULT = ROOT / "assets" / "pets" / "cow-cat"

# Neighbors for edge / local average (4-connected + diagonals).
_N8 = (
    (-1, -1),
    (0, -1),
    (1, -1),
    (-1, 0),
    (1, 0),
    (-1, 1),
    (0, 1),
    (1, 1),
)


def is_hard_magenta(r: int, g: int, b: int) -> bool:
    """Solid chroma-key leftovers (never black fur)."""
    if r > 150 and b > 130 and g < 190 and r > g + 25 and b > g + 15:
        return True
    if r > 190 and g < 160 and b > 140 and r > g + 30:
        return True
    if r > 200 and 80 < g < 180 and b > 160 and r > g + 40:
        return True
    return False


def spill_score(r: int, g: int, b: int) -> int:
    """How magenta-biased a pixel is. Higher = more spill."""
    return int(r) + int(b) - 2 * int(g)


def is_spill(r: int, g: int, b: int, a: int) -> bool:
    """Purple/pink cast that should not appear on tuxedo fur edges."""
    if a < 8:
        return False
    if is_hard_magenta(r, g, b):
        return True
    sc = spill_score(r, g, b)
    # Dark-purple halo typical on black fur after magenta key:
    # e.g. (127,4,134), (95,20,98), (60,8,65), (45,1,29)
    if g < 120 and r > 20 and b > 20 and sc > 28:
        return True
    if g < 100 and r > 30 and b > 30 and min(r, b) > g + 18:
        return True
    # Mid pink fringe
    if r > 90 and b > 70 and g < 130 and r > g + 25 and b > g + 10:
        return True
    # Strong R-B elevation vs G even if darker
    if sc > 45 and g < 140 and max(r, b) > 40:
        return True
    return False


def despill_rgb(r: int, g: int, b: int) -> tuple[int, int, int]:
    """Pull R/B down toward G so chroma magenta cast is removed."""
    # Luminance without purple boost (ignore inflated R/B).
    lum = int(0.15 * min(r, b) + 0.70 * g + 0.15 * min(r, b))
    # Aggressive cap: R/B may not sit far above G.
    cap = g + max(6, g // 5)
    nr = min(r, cap)
    nb = min(b, cap)
    # Collapse toward neutral gray of same green-weighted luma.
    nr = (nr + lum * 2) // 3
    ng = (g + lum * 2) // 3
    nb = (nb + lum * 2) // 3
    # Equalize remaining R/B imbalance (kills residual purple/cyan).
    mid = (nr + nb) // 2
    nr = (nr + mid) // 2
    nb = (nb + mid) // 2
    # Black-fur edges must stay dark and near-neutral.
    if lum < 90:
        target = max(0, min(lum, (g + min(r, b)) // 2))
        nr = min(nr, target + 8)
        ng = min(ng, target + 8)
        nb = min(nb, target + 8)
        # Final clamp: |R-B| small, both near G.
        avg = (nr + ng + nb) // 3
        nr = (nr + avg * 2) // 3
        ng = (ng + avg * 2) // 3
        nb = (nb + avg * 2) // 3
    return (
        max(0, min(255, nr)),
        max(0, min(255, ng)),
        max(0, min(255, nb)),
    )


def fix_rgba(arr: np.ndarray) -> np.ndarray:
    """In-place-style fix; returns new RGBA uint8 array."""
    out = arr.copy()
    h, w = out.shape[:2]
    r = out[:, :, 0].astype(np.int16)
    g = out[:, :, 1].astype(np.int16)
    b = out[:, :, 2].astype(np.int16)
    a = out[:, :, 3].astype(np.int16)

    # 1) Hard-key pure magenta anywhere.
    hard = (
        ((r > 150) & (b > 130) & (g < 190) & (r > g + 25) & (b > g + 15))
        | ((r > 190) & (g < 160) & (b > 140) & (r > g + 30))
        | ((r > 200) & (g > 80) & (g < 180) & (b > 160) & (r > g + 40))
    ) & (a > 0)
    out[hard] = (0, 0, 0, 0)
    r, g, b, a = (
        out[:, :, 0].astype(np.int16),
        out[:, :, 1].astype(np.int16),
        out[:, :, 2].astype(np.int16),
        out[:, :, 3].astype(np.int16),
    )

    # Spill mask (match is_spill thresholds)
    score = r + b - 2 * g
    spill = (a > 8) & (
        ((g < 120) & (r > 20) & (b > 20) & (score > 28))
        | ((g < 100) & (r > 30) & (b > 30) & (np.minimum(r, b) > g + 18))
        | ((r > 90) & (b > 70) & (g < 130) & (r > g + 25) & (b > g + 10))
        | ((score > 45) & (g < 140) & (np.maximum(r, b) > 40))
    )

    if not spill.any():
        return out

    # Transparent neighbors (for edge detection)
    opaque = a >= 20
    has_clear_n = np.zeros((h, w), dtype=bool)
    for dx, dy in _N8:
        ys = np.arange(h) + dy
        xs = np.arange(w) + dx
        # clip-safe shift
        src = opaque
        shifted = np.ones_like(opaque)  # out-of-bounds treat as clear? use False for opaque
        # manual pad
        padded = np.pad(opaque.astype(np.uint8), 1, constant_values=0)
        sy = slice(1 + dy, 1 + dy + h)
        sx = slice(1 + dx, 1 + dx + w)
        neigh = padded[sy, sx].astype(bool)
        has_clear_n |= opaque & (~neigh)

    edge_spill = spill & has_clear_n

    # 2) Kill edge pixels that are almost pure spill (little real fur chroma).
    pure = edge_spill & (score > 55) & (g < 70)
    pure |= edge_spill & (score > 80)
    # Thin 1px purple wire on silhouette: dark + strong magenta bias
    pure |= edge_spill & (score > 40) & (g < 40) & (np.maximum(r, b) > g + 25)
    out[pure, 3] = 0
    spill = spill & (out[:, :, 3] > 8)
    edge_spill = spill & has_clear_n

    # 3) For remaining spill: average clean neighbors, else mathematical despill.
    r = out[:, :, 0].astype(np.int16)
    g = out[:, :, 1].astype(np.int16)
    b = out[:, :, 2].astype(np.int16)
    a = out[:, :, 3].astype(np.int16)
    score = r + b - 2 * g
    spill = (a > 8) & (
        ((g < 120) & (r > 20) & (b > 20) & (score > 28))
        | ((g < 100) & (r > 30) & (b > 30) & (np.minimum(r, b) > g + 18))
        | ((r > 90) & (b > 70) & (g < 130) & (r > g + 25) & (b > g + 10))
        | ((score > 45) & (g < 140) & (np.maximum(r, b) > 40))
    )

    clean = (a >= 128) & (~spill)
    ys, xs = np.where(spill)
    for y, x in zip(ys.tolist(), xs.tolist()):
        cr = cg = cb = 0
        n = 0
        for dx, dy in _N8:
            ny, nx = y + dy, x + dx
            if 0 <= ny < h and 0 <= nx < w and clean[ny, nx]:
                cr += int(out[ny, nx, 0])
                cg += int(out[ny, nx, 1])
                cb += int(out[ny, nx, 2])
                n += 1
        aa = int(out[y, x, 3])
        if n > 0:
            nr, ng, nb = cr // n, cg // n, cb // n
            # Prefer clean neighbors; light blend with despilled original.
            od = despill_rgb(int(out[y, x, 0]), int(out[y, x, 1]), int(out[y, x, 2]))
            nr = (nr * 8 + od[0] * 2) // 10
            ng = (ng * 8 + od[1] * 2) // 10
            nb = (nb * 8 + od[2] * 2) // 10
        else:
            nr, ng, nb = despill_rgb(
                int(out[y, x, 0]), int(out[y, x, 1]), int(out[y, x, 2])
            )
        # Always run mathematical despill once more for safety.
        nr, ng, nb = despill_rgb(nr, ng, nb)
        # Outer-edge residual chroma → drop rather than leave a colored wire.
        if has_clear_n[y, x] and spill_score(nr, ng, nb) > 28 and ng < 55:
            out[y, x] = (0, 0, 0, 0)
            continue
        if spill_score(nr, ng, nb) > 70 and ng < 55:
            out[y, x] = (0, 0, 0, 0)
        else:
            out[y, x] = (nr, ng, nb, aa)

    # 4) Final pass: anything still chroma-biased → despill; edge leftovers → kill.
    r = out[:, :, 0].astype(np.int16)
    g = out[:, :, 1].astype(np.int16)
    b = out[:, :, 2].astype(np.int16)
    a = out[:, :, 3]
    score = r + b - 2 * g
    left = (a > 8) & (score > 28) & (g < 120) & (np.maximum(r, b) > 20)
    ys, xs = np.where(left)
    for y, x in zip(ys.tolist(), xs.tolist()):
        nr, ng, nb = despill_rgb(int(r[y, x]), int(g[y, x]), int(b[y, x]))
        if has_clear_n[y, x] and spill_score(nr, ng, nb) > 24 and ng < 60:
            out[y, x] = (0, 0, 0, 0)
        else:
            out[y, x, 0] = nr
            out[y, x, 1] = ng
            out[y, x, 2] = nb

    return out


def process_file(path: Path) -> bool:
    im = Image.open(path).convert("RGBA")
    before = np.array(im)
    after = fix_rgba(before)
    if np.array_equal(before, after):
        return False
    Image.fromarray(after, "RGBA").save(path)
    return True


def main() -> None:
    if len(sys.argv) > 1:
        roots = [Path(p) for p in sys.argv[1:]]
    else:
        roots = [DEFAULT]

    changed = 0
    total = 0
    for root in roots:
        paths = [root] if root.is_file() else sorted(root.rglob("*.png"))
        for p in paths:
            if not p.is_file():
                continue
            # Skip non-sprite masters that are intentional RGB refs if any
            total += 1
            if process_file(p):
                changed += 1
    print(f"despilled {changed}/{total} png(s)")


if __name__ == "__main__":
    main()
