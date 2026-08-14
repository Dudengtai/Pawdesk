"""Rebuild look-down keys onto the idle master silhouette.

look_pitch 0/1 were independent generations. Their outline is a jagged chroma-key
stroke that reads as 毛刺 on a light desktop. This keeps the look-down face and
replaces every edge (ears, back, tail, paws) with idle_blink/000.

Usage:
  python tools/rebuild_look_down.py
"""

from __future__ import annotations

from pathlib import Path

import cv2
import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
PET = ROOT / "assets" / "pets" / "cow-cat"
PITCH = PET / "look_pitch"
MASTER = PET / "idle_blink" / "000.png"
PREVIEW = ROOT / "docs" / "mockups"

HEAD_Y1 = 138
NECK_BLEND = 22
ERODE_PX = 5


def load_rgba(path: Path) -> np.ndarray:
    return np.array(Image.open(path).convert("RGBA"))


def save_rgba(arr: np.ndarray, path: Path) -> None:
    Image.fromarray(arr, "RGBA").save(path)


def body_centroid(arr: np.ndarray, y0: int = 160) -> tuple[float, float] | None:
    ys, xs = np.where(arr[y0:, :, 3] >= 16)
    if xs.size < 8:
        return None
    return float(xs.mean()), float(ys.mean() + y0)


def align_to_master(look: np.ndarray, master: np.ndarray) -> np.ndarray:
    lc, mc = body_centroid(look), body_centroid(master)
    if not lc or not mc:
        return look
    dx = int(np.clip(round(mc[0] - lc[0]), -8, 8))
    dy = int(np.clip(round(mc[1] - lc[1]), -6, 6))
    if dx == 0 and dy == 0:
        return look
    m = np.float32([[1, 0, dx], [0, 1, dy]])
    return cv2.warpAffine(
        look, m, (look.shape[1], look.shape[0]), flags=cv2.INTER_LINEAR, borderValue=(0, 0, 0, 0)
    )


def graft_look_head(look: np.ndarray, master: np.ndarray) -> np.ndarray:
    """Master body + silhouette; look-down face interior only."""
    look = align_to_master(look, master)
    h = master.shape[0]
    out = master.copy().astype(np.float32)
    lk = look.astype(np.float32)

    k = cv2.getStructuringElement(cv2.MORPH_ELLIPSE, (ERODE_PX * 2 + 1, ERODE_PX * 2 + 1))
    interior = cv2.erode((look[:, :, 3] >= 140).astype(np.uint8), k)
    master_solid = master[:, :, 3] >= 140

    yy = np.arange(h, dtype=np.float32)
    fade0 = float(HEAD_Y1 - NECK_BLEND)
    fade1 = float(HEAD_Y1)
    t = np.clip((yy - fade0) / max(fade1 - fade0, 1.0), 0.0, 1.0)
    head_w = (1.0 - t)[:, None]

    copy = (interior > 0) & master_solid
    wgt = cv2.GaussianBlur(copy.astype(np.float32) * head_w, (0, 0), 1.1)
    for c in range(3):
        out[:, :, c] = out[:, :, c] * (1.0 - wgt) + lk[:, :, c] * wgt
    out[:, :, 3] = master[:, :, 3].astype(np.float32)
    out[out[:, :, 3] == 0, :3] = 0
    return np.clip(out, 0, 255).astype(np.uint8)


def contact_sheet(frames: list[np.ndarray], path: Path) -> None:
    h, w = frames[0].shape[:2]
    sheet = np.zeros((h, w * len(frames), 4), dtype=np.uint8)
    for i, f in enumerate(frames):
        sheet[:, i * w : (i + 1) * w] = f
    path.parent.mkdir(parents=True, exist_ok=True)
    save_rgba(sheet, path)


def main() -> None:
    raise SystemExit(
        "look_pitch 0/1 were regenerated from new stills. "
        "Use tools/pack_pitch_from_gen.py — do not graft the old keys."
    )
    master = load_rgba(MASTER)
    src_dir = PITCH / "_source"
    frames: list[np.ndarray] = []
    for i in range(5):
        if i in (0, 1):
            src = src_dir / f"{i}.png"
            look = load_rgba(src if src.exists() else PITCH / f"{i}.png")
            built = graft_look_head(look, master)
            save_rgba(built, PITCH / f"{i}.png")
            print(f"grafted look_pitch/{i}.png")
            frames.append(built)
        else:
            frames.append(load_rgba(PITCH / f"{i}.png"))
    contact_sheet(frames, PREVIEW / "look-pitch-strip.png")


if __name__ == "__main__":
    main()
