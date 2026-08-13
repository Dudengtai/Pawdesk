"""Pack idle_yawn: sit → authored yawn keys → peak hold → reverse.

Usage:
  python tools/pack_idle_yawn.py
"""

from __future__ import annotations

import json
from collections import deque
from pathlib import Path

import numpy as np
from PIL import Image, ImageFilter

ROOT = Path(__file__).resolve().parents[1]
SIT = ROOT / "assets" / "pets" / "cow-cat" / "idle_blink" / "000.png"
PEAK = ROOT / "assets" / "pets" / "cow-cat" / "_master" / "yawn_peak.png"
SESSION = Path(
    r"C:\Users\lig76\.grok\sessions"
    r"\D%3A%5CAI%E7%BB%83%E4%B9%A0%E7%9B%AE%E5%BD%95%5CPawDesk"
    r"\019ffa01-cc3a-7743-b877-ca9e2ad7ecef\images"
)
OUT = ROOT / "assets" / "pets" / "cow-cat" / "idle_yawn"
SIZE = 256
# Sit head (idle_blink/000): x 96–195, y 18–129. Mouth may drop to ~155.
FACE_CX, FACE_CY = 146.0, 78.0
FACE_RX, FACE_RY = 42.0, 50.0
MOUTH = (114, 72, 178, 150)  # x0,y0,x1,y1 extra for open jaw


def load_rgba(path: Path) -> np.ndarray:
    return np.array(Image.open(path).convert("RGBA"))


def save_rgba(arr: np.ndarray, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(arr, "RGBA").save(path)


def key_black_edge(src: np.ndarray) -> np.ndarray:
    """Flood near-black from the border only — keep interior black fur."""
    im = src.copy()
    if im.shape[0] != SIZE or im.shape[1] != SIZE:
        im = np.array(Image.fromarray(im, "RGBA").resize((SIZE, SIZE), Image.Resampling.LANCZOS))
    rgb = im[:, :, :3].astype(np.int16)
    lum = rgb.mean(axis=2)
    cand = (lum < 28) & (rgb.max(axis=2) < 42)
    h, w = cand.shape
    seen = np.zeros((h, w), dtype=bool)
    q: deque[tuple[int, int]] = deque()
    for x in range(w):
        for y in (0, h - 1):
            if cand[y, x]:
                seen[y, x] = True
                q.append((x, y))
    for y in range(h):
        for x in (0, w - 1):
            if cand[y, x] and not seen[y, x]:
                seen[y, x] = True
                q.append((x, y))
    while q:
        x, y = q.popleft()
        im[y, x, 3] = 0
        for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            nx, ny = x + dx, y + dy
            if 0 <= nx < w and 0 <= ny < h and not seen[ny, nx] and cand[ny, nx]:
                seen[ny, nx] = True
                q.append((nx, ny))
    return im


def kill_dark_halo(im: np.ndarray) -> np.ndarray:
    """Drop the semi-transparent gray ring left by JPEG + black-key."""
    out = im.copy()
    rgb = out[:, :, :3].astype(np.int16)
    lum = rgb.mean(axis=2)
    a = out[:, :, 3]
    # Dark, not-very-opaque edge = leftover bg, not black fur (fur is a>=200).
    halo = (a > 0) & (a < 170) & (lum < 85) & (rgb.max(axis=2) < 110)
    out[halo, 3] = 0
    out[out[:, :, 3] == 0, :3] = 0
    return out


def face_keep_mask(h: int, w: int) -> np.ndarray:
    yy, xx = np.ogrid[:h, :w]
    face = ((xx - FACE_CX) / FACE_RX) ** 2 + ((yy - FACE_CY) / FACE_RY) ** 2 <= 1.0
    x0, y0, x1, y1 = MOUTH
    mouth = (xx >= x0) & (xx < x1) & (yy >= y0) & (yy < y1)
    return face | mouth


def is_mouth_color(rgb: np.ndarray) -> np.ndarray:
    """Open-jaw fill: pink tongue / dark cavity / cream muzzle. Not gray halo."""
    r, g, b = rgb[:, :, 0].astype(np.int16), rgb[:, :, 1].astype(np.int16), rgb[:, :, 2].astype(np.int16)
    pink = (r > 110) & (g > 40) & (g < 170) & (b < 140) & (r > b + 15) & (r > g)
    cavity = (r < 90) & (g < 70) & (b < 70) & (np.maximum(r, g) >= b)
    teeth = (r > 180) & (g > 170) & (b > 150) & (np.abs(r - g) < 40)
    # No cream: JPEG halo is beige and would leak outside the sit outline.
    return pink | cavity | teeth


def silhouette_rim(alpha: np.ndarray, px: int = 4) -> np.ndarray:
    """Pixels that sit on the sit-master outline (keep these exact)."""
    solid = (alpha >= 16).astype(np.uint8) * 255
    im = Image.fromarray(solid, mode="L")
    grow = np.array(im.filter(ImageFilter.MaxFilter(px * 2 + 1))) > 0
    shrink = np.array(im.filter(ImageFilter.MinFilter(px * 2 + 1))) > 0
    return grow & ~shrink


def stamp_face(sit: np.ndarray, yawn: np.ndarray) -> np.ndarray:
    """Sit body + sit outline. Only interior face / open mouth from the yawn drawing."""
    src = kill_dark_halo(yawn)
    sit_solid = sit[:, :, 3] >= 16
    face = face_keep_mask(SIZE, SIZE)
    mouth = is_mouth_color(src) & face & (src[:, :, 3] >= 24)
    interior = face & sit_solid & (src[:, :, 3] >= 40)
    keep = interior | mouth
    keep[silhouette_rim(sit[:, :, 3], 4)] = False
    keep_u8 = (keep.astype(np.uint8) * 255)
    soft = np.array(Image.fromarray(keep_u8, mode="L").filter(ImageFilter.GaussianBlur(radius=0.8)))
    t = (soft.astype(np.float32) / 255.0)[..., None]
    t = t * (src[:, :, 3:4].astype(np.float32) / 255.0)
    out = sit.astype(np.float32) * (1.0 - t) + src.astype(np.float32) * t
    out = np.clip(out, 0, 255).astype(np.uint8)
    # Body, tail, ears outline: sit wins everywhere we did not keep.
    out[~keep & ~mouth] = sit[~keep & ~mouth]
    return out


def hold(frame: np.ndarray, n: int) -> list[np.ndarray]:
    return [frame] * n


def main() -> None:
    sit = load_rgba(SIT)
    peak_src = load_rgba(PEAK)
    if peak_src.shape[0] != SIZE:
        peak_src = np.array(
            Image.fromarray(peak_src, "RGBA").resize((SIZE, SIZE), Image.Resampling.LANCZOS)
        )
    peak = stamp_face(sit, key_black_edge(peak_src))
    early = stamp_face(sit, key_black_edge(load_rgba(SESSION / "4.jpg")))
    mid = stamp_face(sit, key_black_edge(load_rgba(SESSION / "2.jpg")))
    save_rgba(early, ROOT / "assets" / "pets" / "cow-cat" / "_master" / "yawn_early.png")
    save_rgba(mid, ROOT / "assets" / "pets" / "cow-cat" / "_master" / "yawn_mid.png")
    save_rgba(peak, ROOT / "assets" / "pets" / "cow-cat" / "_master" / "yawn_peak_stamped.png")

    # No DIS in-betweens: flow ghosts faces. 30fps is holds of clean keys.
    frames = (
        hold(sit, 3)
        + hold(early, 8)
        + hold(mid, 8)
        + hold(peak, 36)
        + hold(mid, 8)
        + hold(early, 8)
        + hold(sit, 6)
    )

    OUT.mkdir(parents=True, exist_ok=True)
    for old in OUT.glob("*.png"):
        old.unlink()
    files = []
    for i, f in enumerate(frames):
        name = f"{i:03d}.png"
        save_rgba(f, OUT / name)
        files.append(name)

    peak_start = 3 + 8 + 8
    peak_end = peak_start + 36 - 1
    meta = {
        "name": "idle_yawn",
        "frame_width": SIZE,
        "frame_height": SIZE,
        "frames": len(frames),
        "fps": 30.0,
        "loop": False,
        "files": files,
        "peak_start": peak_start,
        "peak_end": peak_end,
        "source": "master_yawn_face_stamp",
        "notes": "Sit body locked. Only cleaned face stamped. No flow, no JPEG halo.",
    }
    (OUT / "meta.json").write_text(json.dumps(meta, indent=2), encoding="utf-8")
    print(f"packed {len(frames)} frames @30fps  peak {peak_start}-{peak_end}  dur={len(frames)/30:.2f}s")


if __name__ == "__main__":
    main()
