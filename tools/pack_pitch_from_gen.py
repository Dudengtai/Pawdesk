"""Pack newly generated look-up / look-down stills into look_pitch.

Soft-keys magenta (no binary punch), aligns onto the 256 sit master, then
locks paws/tail/chest so only the head pose changes.
"""

from __future__ import annotations

import json
import shutil
from pathlib import Path

import cv2
import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
PET = ROOT / "assets" / "pets" / "cow-cat"
PITCH = PET / "look_pitch"
MASTER = PET / "idle_blink" / "000.png"
GEN_DIR = PITCH / "_gen"
PREVIEW = ROOT / "docs" / "mockups"
QA = ROOT / "target" / "_pitch_regen"
SIZE = 256
HEAD_LOCK_Y = 142
NECK = 20

# Session stills: 3=full down, 5=slight down, 1=slight up, 4=full up
SESSION = Path(
    r"C:\Users\lig76\.grok\sessions"
    r"\D%3A%5CAI%E7%BB%83%E4%B9%A0%E7%9B%AE%E5%BD%95%5CPawDesk"
    r"\019ffd91-4543-7492-b35d-015fdfb4e9da\images"
)
SOURCES = {
    0: SESSION / "3.jpg",
    1: SESSION / "5.jpg",
    3: SESSION / "1.jpg",
    4: SESSION / "4.jpg",
}


def load_rgba(path: Path) -> np.ndarray:
    return np.array(Image.open(path).convert("RGBA"))


def save_rgba(arr: np.ndarray, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(arr, "RGBA").save(path)


def composite(arr: np.ndarray, bg: tuple[int, int, int]) -> np.ndarray:
    a = arr[:, :, 3:4].astype(np.float32) / 255.0
    rgb = arr[:, :, :3].astype(np.float32)
    base = np.full_like(rgb, bg, dtype=np.float32)
    return (rgb * a + base * (1.0 - a)).astype(np.uint8)


def soft_key_magenta(rgb: np.ndarray) -> np.ndarray:
    """Soft-key flat magenta. Returns uint8 RGBA with a coverage ramp, not 0/255."""
    img = rgb[:, :, :3].astype(np.int16)
    r, g, b = img[:, :, 0], img[:, :, 1], img[:, :, 2]
    # Magenta chroma: both R and B sit well above G.
    chroma = (r.astype(np.int32) + b.astype(np.int32)) // 2 - g.astype(np.int32)
    # Fur never has this (black/cream/pink-ear max out far lower).
    # 55 = keep, 130 = fully kill. Ramp in between = AA.
    alpha = 1.0 - np.clip((chroma.astype(np.float32) - 55.0) / 75.0, 0.0, 1.0)
    # Corner flood: anything reachable through high-magenta is background.
    bg = ((chroma > 90) & (g < 90) & (np.minimum(r, b) > 140)).astype(np.uint8)
    h, w = bg.shape
    mask = np.zeros((h + 2, w + 2), np.uint8)
    flood = bg.copy()
    for x, y in ((0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)):
        if flood[y, x]:
            cv2.floodFill(flood, mask, (x, y), 2, loDiff=0, upDiff=0)
    bg_cc = flood == 2
    # Dilate bg a hair so JPEG ringing on the key is eaten.
    bg_cc = cv2.dilate(bg_cc.astype(np.uint8), np.ones((3, 3), np.uint8), iterations=1) > 0
    alpha = np.where(bg_cc, np.minimum(alpha, 0.15), alpha)
    alpha = np.where(bg_cc & (chroma > 110), 0.0, alpha)

    out = np.zeros((h, w, 4), dtype=np.uint8)
    # Despill: pull R/B down toward G on remaining fringe.
    fringe = (alpha > 0.02) & (alpha < 0.92)
    nr = r.astype(np.float32)
    ng = g.astype(np.float32)
    nb = b.astype(np.float32)
    cap = ng + 18.0
    nr = np.where(fringe, np.minimum(nr, cap), nr)
    nb = np.where(fringe, np.minimum(nb, cap), nb)
    out[:, :, 0] = np.clip(nr, 0, 255).astype(np.uint8)
    out[:, :, 1] = np.clip(ng, 0, 255).astype(np.uint8)
    out[:, :, 2] = np.clip(nb, 0, 255).astype(np.uint8)
    out[:, :, 3] = np.clip(alpha * 255.0, 0, 255).astype(np.uint8)
    out[out[:, :, 3] == 0, :3] = 0

    # Keep largest opaque component (the cat).
    solid = (out[:, :, 3] >= 24).astype(np.uint8)
    n, labels, stats, _ = cv2.connectedComponentsWithStats(solid, connectivity=8)
    if n > 1:
        keep = 1 + int(np.argmax(stats[1:, cv2.CC_STAT_AREA]))
        out[labels != keep] = 0
    return out


def bbox_alpha(arr: np.ndarray, thr: int = 20) -> tuple[int, int, int, int]:
    ys, xs = np.where(arr[:, :, 3] >= thr)
    if xs.size == 0:
        return 0, 0, arr.shape[1], arr.shape[0]
    return int(xs.min()), int(ys.min()), int(xs.max()) + 1, int(ys.max()) + 1


def align_to_master(src: np.ndarray, master: np.ndarray) -> np.ndarray:
    """Fit src silhouette into master's bbox, 256 canvas."""
    rx0, ry0, rx1, ry1 = bbox_alpha(master)
    sx0, sy0, sx1, sy1 = bbox_alpha(src)
    crop = src[sy0:sy1, sx0:sx1]
    th = ry1 - ry0
    scale = th / max(crop.shape[0], 1)
    nw = max(1, int(round(crop.shape[1] * scale)))
    nh = max(1, int(round(crop.shape[0] * scale)))
    resized = np.array(Image.fromarray(crop, "RGBA").resize((nw, nh), Image.Resampling.LANCZOS))
    canvas = np.zeros_like(master)
    cx = (rx0 + rx1) // 2
    cy = (ry0 + ry1) // 2
    x0 = cx - nw // 2
    y0 = cy - nh // 2
    dx0 = max(0, -x0)
    dy0 = max(0, -y0)
    dx1 = min(nw, master.shape[1] - x0)
    dy1 = min(nh, master.shape[0] - y0)
    if dx1 > dx0 and dy1 > dy0:
        canvas[y0 + dy0 : y0 + dy1, x0 + dx0 : x0 + dx1] = resized[dy0:dy1, dx0:dx1]
    return canvas


def lock_body(look: np.ndarray, master: np.ndarray) -> np.ndarray:
    """Master from mid-chest down; generated head above, soft neck blend."""
    h = master.shape[0]
    out = look.copy().astype(np.float32)
    mst = master.astype(np.float32)
    yy = np.arange(h, dtype=np.float32)
    t = np.clip((yy - (HEAD_LOCK_Y - NECK)) / (2.0 * NECK), 0.0, 1.0)
    w = t[:, None, None]
    # Premul blend
    a0 = out[:, :, 3:4] / 255.0
    b0 = mst[:, :, 3:4] / 255.0
    oa = a0 * (1.0 - w) + b0 * w
    rgb = out[:, :, :3] * a0 * (1.0 - w) + mst[:, :, :3] * b0 * w
    out[:, :, :3] = np.divide(rgb, oa, out=np.zeros_like(rgb), where=oa > 1e-4)
    out[:, :, 3:4] = oa * 255.0
    out[out[:, :, 3] < 1, :3] = 0
    return np.clip(out, 0, 255).astype(np.uint8)


def contact_sheet(frames: list[np.ndarray], path: Path) -> None:
    h, w = frames[0].shape[:2]
    sheet = np.zeros((h, w * len(frames), 4), dtype=np.uint8)
    for i, f in enumerate(frames):
        sheet[:, i * w : (i + 1) * w] = f
    save_rgba(sheet, path)


def main() -> None:
    GEN_DIR.mkdir(parents=True, exist_ok=True)
    QA.mkdir(parents=True, exist_ok=True)
    master = load_rgba(MASTER)
    frames: list[np.ndarray] = [None] * 5  # type: ignore
    frames[2] = master
    save_rgba(master, PITCH / "2.png")

    gray = (236, 236, 238)
    for idx, src in SOURCES.items():
        if not src.exists():
            raise SystemExit(f"missing gen source {src}")
        dest = GEN_DIR / f"{idx}.jpg"
        shutil.copy2(src, dest)
        keyed = soft_key_magenta(np.array(Image.open(src).convert("RGB")))
        aligned = align_to_master(keyed, master)
        locked = lock_body(aligned, master)
        save_rgba(locked, PITCH / f"{idx}.png")
        frames[idx] = locked
        Image.fromarray(composite(locked, gray)).save(QA / f"{idx}_gray.png")
        Image.fromarray(composite(locked, (40, 80, 50))).save(QA / f"{idx}_desk.png")
        head = composite(locked[8:128, 64:204], gray)
        Image.fromarray(head).resize(
            (head.shape[1] * 3, head.shape[0] * 3), Image.Resampling.NEAREST
        ).save(QA / f"{idx}_head3x.png")
        print(f"packed look_pitch/{idx}.png from {src.name}")

    meta = {
        "name": "look_pitch",
        "frame_width": SIZE,
        "frame_height": SIZE,
        "frames": 5,
        "fps": 24.0,
        "loop": False,
        "files": [f"{i}.png" for i in range(5)],
        "look_x": [0.0] * 5,
        "look_y": [-1.0, -0.5, 0.0, 0.5, 1.0],
        "notes": "Regenerated pitch: 0=look down, 2=front master, 4=look up. Soft-keyed from 1024 magenta stills.",
    }
    (PITCH / "meta.json").write_text(json.dumps(meta, indent=2), encoding="utf-8")
    contact_sheet(frames, PREVIEW / "look-pitch-strip.png")
    Image.fromarray(composite(np.array(Image.open(PREVIEW / "look-pitch-strip.png").convert("RGBA")), gray)).save(
        QA / "strip_gray.png"
    )
    print("wrote", PREVIEW / "look-pitch-strip.png")


if __name__ == "__main__":
    main()
