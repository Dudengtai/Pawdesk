"""Add soft anti-aliased alpha edges to pet sprites (reduce staircase jaggies).

Does not change opaque interior colors — only feathers a ~1.5px alpha ramp
around the silhouette so desktop scaling looks cleaner.

Usage:
  python tools/soften_pet_edges.py
  python tools/soften_pet_edges.py assets/pets/cow-cat/idle_blink
"""

from __future__ import annotations

import sys
from pathlib import Path

import numpy as np
from PIL import Image, ImageFilter

ROOT = Path(__file__).resolve().parents[1]
DEFAULT = ROOT / "assets" / "pets" / "cow-cat"


def soft_edge_aa(arr: np.ndarray, radius: float = 1.15) -> np.ndarray:
    """Feather silhouette alpha with a short Gaussian ramp (anti-jaggy)."""
    solid = arr[:, :, 3] >= 128
    mask = Image.fromarray((solid.astype(np.uint8) * 255), mode="L")
    soft = mask.filter(ImageFilter.GaussianBlur(radius=radius))
    blur = np.array(soft, dtype=np.float32) / 255.0
    # Keep core fully opaque; only the outer fringe becomes soft.
    alpha = np.clip(blur, 0.0, 1.0)
    alpha = np.power(alpha, 0.78)
    new_a = (alpha * 255.0).astype(np.uint8)
    new_a = np.where(solid & (blur >= 0.88), np.uint8(255), new_a)
    # Do not invent opaque pixels far outside the body
    new_a = np.where((~solid) & (blur < 0.06), np.uint8(0), new_a)
    out = arr.copy()
    # Edge RGB: sample from nearest solid neighbor-ish by keeping original RGB
    # (hard-edge sprites already store fur color on boundary texels).
    out[:, :, 3] = new_a
    out[new_a == 0, :3] = 0
    return out


def process(path: Path) -> bool:
    im = Image.open(path).convert("RGBA")
    before = np.array(im)
    # Skip nearly-empty frames
    if (before[:, :, 3] > 8).sum() < 32:
        return False
    after = soft_edge_aa(before)
    if np.array_equal(before, after):
        return False
    Image.fromarray(after, "RGBA").save(path)
    return True


def main() -> None:
    roots = [Path(p) for p in sys.argv[1:]] if len(sys.argv) > 1 else [DEFAULT]
    n = ch = 0
    for root in roots:
        paths = [root] if root.is_file() else sorted(root.rglob("*.png"))
        for p in paths:
            if not p.is_file():
                continue
            # Skip non-frame masters that are RGB refs if needed — process all pet pngs
            n += 1
            if process(p):
                ch += 1
    print(f"soft-edged {ch}/{n} png(s)")


if __name__ == "__main__":
    main()
