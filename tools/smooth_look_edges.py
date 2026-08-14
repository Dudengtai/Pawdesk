"""Smooth jaggy chroma-key silhouettes on look-strip frames.

look_pitch / look_diag were hard-keyed from magenta stills then resized, so the
ear and head outline stair-step. This rebuilds a short AA ramp around a lightly
smoothed contour without touching interior fur or the front master copy.

Usage:
  python tools/smooth_look_edges.py
  python tools/smooth_look_edges.py assets/pets/cow-cat/look_pitch
"""

from __future__ import annotations

import sys
from pathlib import Path

import cv2
import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
PET = ROOT / "assets" / "pets" / "cow-cat"
# Front pitch key is a copy of idle_blink/000 — leave it alone.
SKIP_NAMES = {"2.png"}


def load_rgba(path: Path) -> np.ndarray:
    return np.array(Image.open(path).convert("RGBA"))


def save_rgba(arr: np.ndarray, path: Path) -> None:
    Image.fromarray(arr, "RGBA").save(path)


def smooth_look_rgba(arr: np.ndarray, sigma: float = 0.60) -> np.ndarray:
    """Smooth 1px stairs; coverage from the 2× downsample is the AA ramp."""
    a = arr[:, :, 3].astype(np.float32)
    solid0 = (a >= 128).astype(np.float32)
    h, w = solid0.shape

    # 2× upsample → blur → area downsample: subpixel-smooth contour + 1px AA.
    up = cv2.resize(solid0, (w * 2, h * 2), interpolation=cv2.INTER_LINEAR)
    up = cv2.GaussianBlur(up, (0, 0), sigmaX=sigma * 2.0)
    cover = np.clip(cv2.resize(up, (w, h), interpolation=cv2.INTER_AREA), 0.0, 1.0)
    # Slight contrast so the core stays solid (matches idle_blink softness).
    new_a = np.clip(np.power(cover, 0.72), 0.0, 1.0)
    new_a = np.where(cover >= 0.90, 1.0, new_a)
    new_a = np.where(cover < 0.06, 0.0, new_a)

    rgb = arr[:, :, :3].astype(np.float32)
    interior = (cover >= 0.85).astype(np.float32)
    rgb_i = cv2.blur(rgb * interior[..., None], (3, 3))
    mask_b = cv2.blur(interior, (3, 3))
    inward = np.divide(
        rgb_i, mask_b[..., None], out=np.zeros_like(rgb), where=mask_b[..., None] > 1e-4
    )
    # Outer fringe: use inward fur so the dark line-art does not smear into a halo.
    outer = new_a < 0.55
    fill = (a < 20) | outer
    out_rgb = np.where(fill[..., None], inward, rgb)

    out = arr.copy()
    out[:, :, :3] = np.clip(out_rgb, 0, 255).astype(np.uint8)
    out[:, :, 3] = np.clip(new_a * 255.0, 0, 255).astype(np.uint8)
    out[out[:, :, 3] == 0, :3] = 0
    return out


def process_file(path: Path) -> bool:
    if path.name in SKIP_NAMES and path.parent.name == "look_pitch":
        return False
    before = load_rgba(path)
    if (before[:, :, 3] > 8).sum() < 32:
        return False
    after = smooth_look_rgba(before)
    if np.array_equal(before, after):
        return False
    save_rgba(after, path)
    return True


def main() -> None:
    if len(sys.argv) > 1:
        roots = [Path(p) for p in sys.argv[1:]]
    else:
        roots = [PET / "look_pitch", PET / "look_diag"]
    n = ch = 0
    for root in roots:
        paths = [root] if root.is_file() else sorted(root.glob("*.png"))
        for p in paths:
            if not p.is_file():
                continue
            n += 1
            if process_file(p):
                ch += 1
                print(f"smoothed {p.relative_to(ROOT)}")
    print(f"smoothed {ch}/{n} png(s)")


if __name__ == "__main__":
    main()
