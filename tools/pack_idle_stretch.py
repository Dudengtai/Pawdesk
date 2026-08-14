"""Pack idle_stretch: sit → turn → crouch → reach → mid → peak → reverse.

Usage:
  python tools/pack_idle_stretch.py
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
SIT = PET / "idle_blink" / "000.png"
MASTER = PET / "_master"
SESSION = Path(
    r"C:\Users\lig76\.grok\sessions"
    r"\D%3A%5CAI%E7%BB%83%E4%B9%A0%E7%9B%AE%E5%BD%95%5CPawDesk"
    r"\019ffe57-a1f3-7950-81eb-a3fd6b7b2163\images"
)
OUT = PET / "idle_stretch"
QA = ROOT / "target" / "_stretch_pack"
SIZE = 256
FPS = 50.0
# 110f @50 = 2.20s (clears runtime ACTION_MIN_SECS=2.2).
HOLD_SIT_IN = 4
HOLD_TURN = 8
HOLD_CROUCH = 9
HOLD_REACH = 10
HOLD_MID = 10
HOLD_PEAK = 28
HOLD_SIT_OUT = 4

# Session stills (edit-chain from sit_master_magenta).
SOURCES = {
    "turn": SESSION / "21.jpg",
    "crouch": SESSION / "22.jpg",
    "reach": SESSION / "24.jpg",
    "mid": SESSION / "25.jpg",
    "peak": SESSION / "26.jpg",
}

SIT_FACE_CX, SIT_FACE_CY = 146.0, 78.0


def load_rgb(path: Path) -> np.ndarray:
    return np.array(Image.open(path).convert("RGB"))


def load_rgba(path: Path) -> np.ndarray:
    return np.array(Image.open(path).convert("RGBA"))


def save_rgba(arr: np.ndarray, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(arr, "RGBA").save(path)


def soft_key_magenta(rgb: np.ndarray) -> np.ndarray:
    """Key magenta at native resolution. Do not downscale first (smears a dark halo)."""
    img = rgb[:, :, :3].astype(np.int16)
    r, g, b = img[:, :, 0], img[:, :, 1], img[:, :, 2]
    chroma = (r.astype(np.int32) + b.astype(np.int32)) // 2 - g.astype(np.int32)
    # Tighter than pack_pitch: JPEG ringing stays outside the fur.
    alpha = 1.0 - np.clip((chroma.astype(np.float32) - 70.0) / 50.0, 0.0, 1.0)
    bg = ((chroma > 80) & (g < 100) & (np.minimum(r, b) > 120)).astype(np.uint8)
    h, w = bg.shape
    mask = np.zeros((h + 2, w + 2), np.uint8)
    flood = bg.copy()
    for x, y in ((0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)):
        if flood[y, x]:
            cv2.floodFill(flood, mask, (x, y), 2, loDiff=0, upDiff=0)
    bg_cc = flood == 2
    # Eat the JPEG magenta ring (2–3px at 1024 ≈ 1px at 256).
    bg_cc = cv2.dilate(bg_cc.astype(np.uint8), np.ones((5, 5), np.uint8), iterations=1) > 0
    alpha = np.where(bg_cc, 0.0, alpha)

    out = np.zeros((h, w, 4), dtype=np.uint8)
    fringe = (alpha > 0.02) & (alpha < 0.92)
    nr = r.astype(np.float32)
    ng = g.astype(np.float32)
    nb = b.astype(np.float32)
    cap = ng + 12.0
    nr = np.where(fringe, np.minimum(nr, cap), nr)
    nb = np.where(fringe, np.minimum(nb, cap), nb)
    out[:, :, 0] = np.clip(nr, 0, 255).astype(np.uint8)
    out[:, :, 1] = np.clip(ng, 0, 255).astype(np.uint8)
    out[:, :, 2] = np.clip(nb, 0, 255).astype(np.uint8)
    out[:, :, 3] = np.clip(alpha * 255.0, 0, 255).astype(np.uint8)
    out[out[:, :, 3] == 0, :3] = 0
    return keep_largest(kill_dark_halo(out))


def kill_dark_halo(im: np.ndarray) -> np.ndarray:
    """Drop the semi-transparent gray/magenta ring JPEG leaves after keying."""
    out = im.copy()
    rgb = out[:, :, :3].astype(np.int16)
    lum = rgb.mean(axis=2)
    chroma = (rgb[:, :, 0].astype(np.int32) + rgb[:, :, 2].astype(np.int32)) // 2 - rgb[:, :, 1]
    a = out[:, :, 3]
    # Fur interior is a>=200. Dark or magenta crumbs below that are fringe.
    halo = (a > 0) & (a < 170) & ((lum < 95) | (chroma > 28))
    out[halo, 3] = 0
    out[out[:, :, 3] < 36, 3] = 0
    out[out[:, :, 3] == 0, :3] = 0
    return out


def keep_largest(im: np.ndarray) -> np.ndarray:
    solid = (im[:, :, 3] >= 24).astype(np.uint8)
    n, labels, stats, _ = cv2.connectedComponentsWithStats(solid, connectivity=8)
    if n > 1:
        keep = 1 + int(np.argmax(stats[1:, cv2.CC_STAT_AREA]))
        im = im.copy()
        im[labels != keep] = 0
    return im


def downscale_premul(im: np.ndarray, size: int = SIZE) -> np.ndarray:
    """Premultiplied LANCZOS so the 1024 hard cut becomes a 1px AA at 256."""
    if im.shape[0] == size and im.shape[1] == size:
        return im
    a = im[:, :, 3:4].astype(np.float32) / 255.0
    rgb = im[:, :, :3].astype(np.float32) * a
    packed = np.concatenate([rgb, im[:, :, 3:4].astype(np.float32)], axis=2)
    small = np.array(
        Image.fromarray(np.clip(packed, 0, 255).astype(np.uint8), "RGBA").resize(
            (size, size), Image.Resampling.LANCZOS
        ),
        dtype=np.float32,
    )
    aa = small[:, :, 3:4] / 255.0
    rgb = np.divide(small[:, :, :3], aa, out=np.zeros_like(small[:, :, :3]), where=aa > 1e-4)
    out = np.zeros((size, size, 4), dtype=np.uint8)
    out[:, :, :3] = np.clip(rgb, 0, 255).astype(np.uint8)
    out[:, :, 3] = np.clip(small[:, :, 3], 0, 255).astype(np.uint8)
    out[out[:, :, 3] < 20] = 0
    out[out[:, :, 3] == 0, :3] = 0
    return despill_rim(out)


def despill_rim(im: np.ndarray) -> np.ndarray:
    """Kill leftover magenta wire on the silhouette so the ink reads black."""
    out = im.copy()
    r = out[:, :, 0].astype(np.int16)
    g = out[:, :, 1].astype(np.int16)
    b = out[:, :, 2].astype(np.int16)
    a = out[:, :, 3]
    h, w = a.shape
    score = r + b - 2 * g
    opaque = a >= 16
    pad = np.pad(opaque.astype(np.uint8), 1, constant_values=0)
    clear_n = np.zeros((h, w), dtype=bool)
    for dy, dx in ((-1, 0), (1, 0), (0, -1), (0, 1)):
        clear_n |= opaque & (pad[1 + dy : 1 + dy + h, 1 + dx : 1 + dx + w] == 0)
    rim = clear_n & (score > 20) & (g < 140)
    kill = rim & ((a < 190) | ((g < 55) & (score > 28)))
    out[kill] = 0
    rest = rim & ~kill
    cap = np.minimum(g + 8, 255)
    out[:, :, 0] = np.where(rest, np.minimum(r, cap), out[:, :, 0]).astype(np.uint8)
    out[:, :, 2] = np.where(rest, np.minimum(b, cap), out[:, :, 2]).astype(np.uint8)
    out[out[:, :, 3] == 0, :3] = 0
    return out


def bbox_alpha(arr: np.ndarray, thr: int = 20) -> tuple[int, int, int, int]:
    ys, xs = np.where(arr[:, :, 3] >= thr)
    if xs.size == 0:
        return 0, 0, arr.shape[1], arr.shape[0]
    return int(xs.min()), int(ys.min()), int(xs.max()) + 1, int(ys.max()) + 1


def foot_lock(src: np.ndarray, sit: np.ndarray) -> np.ndarray:
    """Translate only: sit foot line. Never scale the cat down to fit."""
    _sx0, _sy0, _sx1, sit_y1 = bbox_alpha(sit)
    _kx0, _ky0, _kx1, src_y1 = bbox_alpha(src)
    dy = sit_y1 - src_y1
    canvas = np.zeros((SIZE, SIZE, 4), dtype=np.uint8)
    ys, xs = np.where(src[:, :, 3] > 0)
    if xs.size == 0:
        return canvas
    ny = ys + dy
    ok = (ny >= 0) & (ny < SIZE) & (xs >= 0) & (xs < SIZE)
    canvas[ny[ok], xs[ok]] = src[ys[ok], xs[ok]]
    return canvas


def detect_nose(arr: np.ndarray) -> tuple[float, float]:
    """Pink-nose centroid; fall back to upper-third alpha centroid."""
    rgb = arr[:, :, :3].astype(np.int16)
    a = arr[:, :, 3]
    r, g, b = rgb[:, :, 0], rgb[:, :, 1], rgb[:, :, 2]
    pink = (
        (a >= 40)
        & (r > 140)
        & (g > 70)
        & (g < 190)
        & (b < 170)
        & (r > g + 8)
        & (r > b)
    )
    ys, xs = np.where(pink)
    if xs.size >= 8:
        return float(xs.mean()), float(ys.mean())
    solid = a >= 40
    yy, xx = np.mgrid[: SIZE, : SIZE]
    # Head is usually the top 45% of the silhouette.
    y0, _, _, y1 = bbox_alpha(arr)
    head = solid & (yy < y0 + 0.45 * max(y1 - y0, 1))
    if head.any():
        return float(xx[head].mean()), float(yy[head].mean())
    return SIT_FACE_CX, SIT_FACE_CY


def hold(frame: np.ndarray, n: int) -> list[np.ndarray]:
    return [frame] * n


def contact_sheet(frames: list[np.ndarray], path: Path, bg: tuple[int, int, int] = (236, 236, 238)) -> None:
    h, w = frames[0].shape[:2]
    sheet = np.zeros((h, w * len(frames), 4), dtype=np.uint8)
    for i, f in enumerate(frames):
        a = f[:, :, 3:4].astype(np.float32) / 255.0
        rgb = f[:, :, :3].astype(np.float32)
        base = np.full_like(rgb, bg, dtype=np.float32)
        comp = (rgb * a + base * (1.0 - a)).astype(np.uint8)
        sheet[:, i * w : (i + 1) * w, :3] = comp
        sheet[:, i * w : (i + 1) * w, 3] = 255
    save_rgba(sheet, path)


def main() -> None:
    sit = load_rgba(SIT)
    if sit.shape[0] != SIZE:
        raise SystemExit(f"sit is {sit.shape[:2]}, expected {SIZE}")

    MASTER.mkdir(parents=True, exist_ok=True)
    QA.mkdir(parents=True, exist_ok=True)

    keyed: dict[str, np.ndarray] = {}
    noses: dict[str, tuple[float, float]] = {}
    slugs = {
        "turn": "01_turn",
        "crouch": "02_crouch",
        "reach": "03_reach",
        "mid": "04_mid",
        "peak": "05_peak",
    }
    for name, src in SOURCES.items():
        if not src.is_file():
            raise SystemExit(f"missing {src}")
        dest = MASTER / f"stretch_{slugs[name]}.jpg"
        shutil.copy2(src, dest)
        raw = downscale_premul(soft_key_magenta(load_rgb(src)))
        locked = foot_lock(raw, sit)
        save_rgba(locked, MASTER / f"stretch_{slugs[name]}_keyed.png")
        nose = detect_nose(locked)
        noses[name] = nose
        # Do not stamp the sit face: head has moved, a sit-sized ellipse
        # paints a second head on the chest (the old stretch ghost).
        keyed[name] = locked
        print(f"  {name}: nose=({nose[0]:.1f},{nose[1]:.1f})  bbox={bbox_alpha(locked)}")

    turn, crouch, reach = keyed["turn"], keyed["crouch"], keyed["reach"]
    mid, peak = keyed["mid"], keyed["peak"]

    frames = (
        hold(sit, HOLD_SIT_IN)
        + hold(turn, HOLD_TURN)
        + hold(crouch, HOLD_CROUCH)
        + hold(reach, HOLD_REACH)
        + hold(mid, HOLD_MID)
        + hold(peak, HOLD_PEAK)
        + hold(mid, HOLD_MID)
        + hold(reach, HOLD_REACH)
        + hold(crouch, HOLD_CROUCH)
        + hold(turn, HOLD_TURN)
        + hold(sit, HOLD_SIT_OUT)
    )

    OUT.mkdir(parents=True, exist_ok=True)
    for old in OUT.glob("*.png"):
        old.unlink()
    if (OUT / "meta.json").is_file():
        (OUT / "meta.json").unlink()

    files: list[str] = []
    for i, f in enumerate(frames):
        name = f"{i:03d}.png"
        save_rgba(f, OUT / name)
        files.append(name)

    if frames[0].shape != sit.shape or not np.array_equal(frames[0], sit):
        raise SystemExit("bookend frame 0 is not pixel-identical to idle_blink/000")
    if not np.array_equal(frames[-1], sit):
        raise SystemExit("bookend last sit hold is not pixel-identical to idle_blink/000")

    peak_start = (
        HOLD_SIT_IN + HOLD_TURN + HOLD_CROUCH + HOLD_REACH + HOLD_MID
    )
    peak_end = peak_start + HOLD_PEAK - 1
    meta = {
        "name": "idle_stretch",
        "frame_width": SIZE,
        "frame_height": SIZE,
        "frames": len(frames),
        "fps": FPS,
        "loop": False,
        "files": files,
        "peak_start": peak_start,
        "peak_end": peak_end,
        "source": "master_stretch_keys",
        "notes": "Sit bookend exact. 50fps holds. Body+face from keyed master keys. No sit-face stamp. No flow.",
    }
    (OUT / "meta.json").write_text(json.dumps(meta, indent=2), encoding="utf-8")

    keys = [sit, turn, crouch, reach, mid, peak]
    contact_sheet(keys, QA / "keys_gray.png")
    contact_sheet(keys, QA / "keys_desk.png", bg=(48, 72, 56))
    contact_sheet(keys, QA / "keys_white.png", bg=(255, 255, 255))
    contact_sheet(keys, QA / "keys_black.png", bg=(0, 0, 0))
    print(
        f"packed {len(frames)} frames @{FPS:.0f}fps  peak {peak_start}-{peak_end}  "
        f"dur={len(frames)/FPS:.2f}s  out={OUT}"
    )


if __name__ == "__main__":
    main()
