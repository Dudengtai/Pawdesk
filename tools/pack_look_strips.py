"""Densify look_yaw via optical-flow in-betweens and pack look_pitch.

Yaw: keep the 7 authored keys, insert one Farneback in-between per gap → 13 frames.
Pitch: chroma-key + silhouette-align generated extremes onto the front master,
then flow-interpolate slight up/down.

Usage:
  python tools/pack_look_strips.py
"""

from __future__ import annotations

import json
import shutil
from pathlib import Path

import cv2
import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
YAW_DIR = ROOT / "assets" / "pets" / "cow-cat" / "look_yaw"
PITCH_DIR = ROOT / "assets" / "pets" / "cow-cat" / "look_pitch"
BACKUP = ROOT / "assets" / "pets" / "cow-cat" / "look_yaw" / "_keys7"
SESSION_IMG = Path(
    r"C:\Users\lig76\.grok\sessions"
    r"\D%3A%5CAI%E7%BB%83%E4%B9%A0%E7%9B%AE%E5%BD%95%5CPawDesk"
    r"\019ff864-261a-7110-a171-e5727a94663a\images"
)
SIZE = 256
PREVIEW = ROOT / "docs" / "mockups"


def load_rgba(path: Path) -> np.ndarray:
    return np.array(Image.open(path).convert("RGBA"))


def save_rgba(arr: np.ndarray, path: Path) -> None:
    Image.fromarray(arr, "RGBA").save(path)


def composite_rgb(rgba: np.ndarray, bg: tuple[int, int, int] = (48, 36, 48)) -> np.ndarray:
    a = rgba[:, :, 3:4].astype(np.float32) / 255.0
    rgb = rgba[:, :, :3].astype(np.float32)
    base = np.full_like(rgb, bg, dtype=np.float32)
    return (rgb * a + base * (1.0 - a)).astype(np.uint8)


def dense_flow(a_gray: np.ndarray, b_gray: np.ndarray) -> np.ndarray:
    """DIS flow: warp-only in-betweens (no A/B blend — blend ghosts faces)."""
    dis = cv2.DISOpticalFlow_create(cv2.DISOPTICAL_FLOW_PRESET_MEDIUM)
    return dis.calc(a_gray, b_gray, None)


def warp_rgba(rgba: np.ndarray, flow: np.ndarray, scale: float) -> np.ndarray:
    h, w = rgba.shape[:2]
    grid_x, grid_y = np.meshgrid(np.arange(w, dtype=np.float32), np.arange(h, dtype=np.float32))
    map_x = (grid_x + flow[:, :, 0] * scale).astype(np.float32)
    map_y = (grid_y + flow[:, :, 1] * scale).astype(np.float32)
    return cv2.remap(
        rgba,
        map_x,
        map_y,
        interpolation=cv2.INTER_LINEAR,
        borderMode=cv2.BORDER_CONSTANT,
        borderValue=(0, 0, 0, 0),
    )


def flow_inbetween(a: np.ndarray, b: np.ndarray, t: float = 0.5) -> np.ndarray:
    ga = cv2.cvtColor(composite_rgb(a), cv2.COLOR_RGB2GRAY)
    gb = cv2.cvtColor(composite_rgb(b), cv2.COLOR_RGB2GRAY)
    flow_ab = dense_flow(ga, gb)
    return warp_rgba(a, flow_ab, t)


def bbox_alpha(rgba: np.ndarray, thr: int = 16) -> tuple[int, int, int, int]:
    ys, xs = np.where(rgba[:, :, 3] >= thr)
    if xs.size == 0:
        return 0, 0, rgba.shape[1], rgba.shape[0]
    return int(xs.min()), int(ys.min()), int(xs.max()) + 1, int(ys.max()) + 1


def key_magenta(img: Image.Image) -> Image.Image:
    arr = np.array(img.convert("RGBA"))
    r, g, b = arr[:, :, 0].astype(np.int16), arr[:, :, 1].astype(np.int16), arr[:, :, 2].astype(np.int16)
    mag = (
        ((r > 140) & (b > 70) & (g < 120) & (r > g + 30) & (b > g - 10))
        | ((r > 170) & (b > 140) & (g < 180) & (r > g + 30) & (b > g + 20))
        | ((r > 190) & (b > 80) & (g < 90) & (r > g + 60))
    )
    arr[:, :, 3] = np.where(mag, 0, 255).astype(np.uint8)
    # largest component
    alpha = (arr[:, :, 3] >= 20).astype(np.uint8)
    n, labels, stats, _ = cv2.connectedComponentsWithStats(alpha, connectivity=8)
    if n > 1:
        # skip background label 0
        areas = stats[1:, cv2.CC_STAT_AREA]
        keep = 1 + int(np.argmax(areas))
        arr[:, :, 3] = np.where(labels == keep, arr[:, :, 3], 0)
    # despill magenta fringe
    a = arr[:, :, 3]
    edge = (a > 0) & (a < 255)
    # also pixels next to transparent
    kernel = np.ones((3, 3), np.uint8)
    near = cv2.dilate((a == 0).astype(np.uint8), kernel) > 0
    fringe = (a > 0) & near
    for c in (0, 2):
        arr[:, :, c] = np.where(fringe, np.minimum(arr[:, :, c], arr[:, :, 1] + 20), arr[:, :, c])
    return Image.fromarray(arr, "RGBA")


def align_to_ref(src: Image.Image, ref: np.ndarray) -> np.ndarray:
    """Scale+translate src silhouette to match ref bbox, then paste at SIZE."""
    src_a = np.array(src.convert("RGBA"))
    rx0, ry0, rx1, ry1 = bbox_alpha(ref)
    sx0, sy0, sx1, sy1 = bbox_alpha(src_a)
    crop = src_a[sy0:sy1, sx0:sx1]
    tw, th = rx1 - rx0, ry1 - ry0
    # match height primarily (sit pose is vertical)
    scale = th / max(crop.shape[0], 1)
    nw = max(1, int(round(crop.shape[1] * scale)))
    nh = max(1, int(round(crop.shape[0] * scale)))
    resized = np.array(Image.fromarray(crop, "RGBA").resize((nw, nh), Image.Resampling.LANCZOS))
    canvas = np.zeros_like(ref)
    # center on ref bbox
    cx = (rx0 + rx1) // 2
    cy = (ry0 + ry1) // 2
    x0 = cx - nw // 2
    y0 = cy - nh // 2
    # clip
    dx0 = max(0, -x0)
    dy0 = max(0, -y0)
    dx1 = min(nw, ref.shape[1] - x0)
    dy1 = min(nh, ref.shape[0] - y0)
    if dx1 > dx0 and dy1 > dy0:
        dest = canvas[y0 + dy0 : y0 + dy1, x0 + dx0 : x0 + dx1]
        piece = resized[dy0:dy1, dx0:dx1]
        pa = piece[:, :, 3:4].astype(np.float32) / 255.0
        dest[:, :, :3] = (piece[:, :, :3].astype(np.float32) * pa + dest[:, :, :3].astype(np.float32) * (1 - pa)).astype(
            np.uint8
        )
        dest[:, :, 3] = np.maximum(dest[:, :, 3], piece[:, :, 3])
        canvas[y0 + dy0 : y0 + dy1, x0 + dx0 : x0 + dx1] = dest
    return canvas


def contact_sheet(frames: list[np.ndarray], path: Path, cols: int | None = None) -> None:
    n = len(frames)
    cols = cols or n
    rows = (n + cols - 1) // cols
    h, w = frames[0].shape[:2]
    sheet = np.zeros((rows * h, cols * w, 4), dtype=np.uint8)
    for i, f in enumerate(frames):
        r, c = divmod(i, cols)
        sheet[r * h : (r + 1) * h, c * w : (c + 1) * w] = f
    path.parent.mkdir(parents=True, exist_ok=True)
    save_rgba(sheet, path)


def pack_yaw() -> list[np.ndarray]:
    BACKUP.mkdir(parents=True, exist_ok=True)
    keys = []
    for i in range(7):
        src = YAW_DIR / f"{i}.png"
        bak = BACKUP / f"{i}.png"
        if not bak.exists():
            shutil.copy2(src, bak)
        keys.append(load_rgba(bak if bak.exists() else src))

    frames: list[np.ndarray] = []
    for i, k in enumerate(keys):
        frames.append(k)
        if i + 1 < len(keys):
            frames.append(flow_inbetween(k, keys[i + 1], 0.5))

    # write 0..12
    for i, f in enumerate(frames):
        save_rgba(f, YAW_DIR / f"{i}.png")

    xs = [round(-1.0 + i * (2.0 / (len(frames) - 1)), 4) for i in range(len(frames))]
    meta = {
        "name": "look_yaw",
        "frame_width": SIZE,
        "frame_height": SIZE,
        "frames": len(frames),
        "fps": 24.0,
        "loop": False,
        "files": [f"{i}.png" for i in range(len(frames))],
        "look_x": xs,
        "look_y": [0.0] * len(frames),
        "notes": "13-frame yaw: 7 authored keys + 6 DIS warp-only in-betweens. 6=front master.",
    }
    (YAW_DIR / "meta.json").write_text(json.dumps(meta, indent=2), encoding="utf-8")
    contact_sheet(frames, PREVIEW / "look-yaw-strip.png")
    return frames


def pack_pitch(
    front: np.ndarray,
    down_src: Path,
    up_src: Path,
    slight_down_src: Path | None = None,
    slight_up_src: Path | None = None,
) -> list[np.ndarray]:
    down = align_to_ref(key_magenta(Image.open(down_src)), front)
    up = align_to_ref(key_magenta(Image.open(up_src)), front)
    slight_down = (
        align_to_ref(key_magenta(Image.open(slight_down_src)), front)
        if slight_down_src
        else flow_inbetween(front, down, 0.5)
    )
    slight_up = (
        align_to_ref(key_magenta(Image.open(slight_up_src)), front)
        if slight_up_src
        else flow_inbetween(front, up, 0.5)
    )
    frames = [down, slight_down, front, slight_up, up]
    PITCH_DIR.mkdir(parents=True, exist_ok=True)
    for i, f in enumerate(frames):
        save_rgba(f, PITCH_DIR / f"{i}.png")
    ys = [-1.0, -0.5, 0.0, 0.5, 1.0]
    meta = {
        "name": "look_pitch",
        "frame_width": SIZE,
        "frame_height": SIZE,
        "frames": len(frames),
        "fps": 24.0,
        "loop": False,
        "files": [f"{i}.png" for i in range(len(frames))],
        "look_x": [0.0] * len(frames),
        "look_y": ys,
        "notes": "Pitch strip. 0=look down ... 2=front master ... 4=look up.",
    }
    (PITCH_DIR / "meta.json").write_text(json.dumps(meta, indent=2), encoding="utf-8")
    contact_sheet(frames, PREVIEW / "look-pitch-strip.png")
    return frames


def main() -> None:
    yaw = pack_yaw()
    print(f"packed look_yaw {len(yaw)} frames")

    # Prefer newly generated pointy-ear pitch keys; fall back to earlier 25/26.
    down = next(p for p in (SESSION_IMG / "35.jpg", SESSION_IMG / "25.jpg") if p.exists())
    up = next(p for p in (SESSION_IMG / "30.jpg", SESSION_IMG / "26.jpg") if p.exists())
    slight_down = SESSION_IMG / "34.jpg" if (SESSION_IMG / "34.jpg").exists() else None
    slight_up = SESSION_IMG / "31.jpg" if (SESSION_IMG / "31.jpg").exists() else None
    print(f"pitch down source: {down.name}")
    print(f"pitch slight-down source: {slight_down.name if slight_down else 'flow'}")
    print(f"pitch slight-up source: {slight_up.name if slight_up else 'flow'}")
    print(f"pitch up source: {up.name}")
    front = load_rgba(BACKUP / "3.png") if (BACKUP / "3.png").exists() else yaw[6]
    pitch = pack_pitch(front, down, up, slight_down, slight_up)
    print(f"packed look_pitch {len(pitch)} frames")


if __name__ == "__main__":
    main()
