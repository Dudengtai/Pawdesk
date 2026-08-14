"""Pack reminder_hop: sit → gather → launch → air → land → sit.

Usage:
  python tools/pack_reminder_hop.py
"""

from __future__ import annotations

import json
import shutil
from pathlib import Path

import cv2
import numpy as np
from PIL import Image, ImageFilter

ROOT = Path(__file__).resolve().parents[1]
PET = ROOT / "assets" / "pets" / "cow-cat"
SIT = PET / "idle_blink" / "000.png"
MASTER = PET / "_master"
SESSION = Path(
    r"C:\Users\lig76\.grok\sessions"
    r"\D%3A%5CAI%E7%BB%83%E4%B9%A0%E7%9B%AE%E5%BD%95%5CPawDesk"
    r"\019ffec4-f119-7803-93ec-6d7f2d90c479\images"
)
OUT = PET / "reminder_hop"
QA = ROOT / "target" / "_hop_pack"
SIZE = 256
FPS = 30.0

# Holds lined up with movement phases (t 0.18 / 0.40 / 0.70 / 0.88).
HOLD_SIT_IN = 3
HOLD_GATHER = 5
HOLD_LAUNCH = 9
HOLD_AIR = 12
HOLD_LAND = 7
HOLD_SIT_OUT = 5

# Sit face (idle_blink/000): lock 左黑右白 mask onto hop bodies.
SIT_FACE_CX, SIT_FACE_CY = 146.0, 78.0
SIT_FACE_RX, SIT_FACE_RY = 40.0, 46.0

SOURCES = {
    "gather": SESSION / "4.jpg",
    "launch": SESSION / "1.jpg",
    "air": SESSION / "3.jpg",
    "land": SESSION / "5.jpg",
}


def load_rgb(path: Path) -> np.ndarray:
    return np.array(Image.open(path).convert("RGB"))


def load_rgba(path: Path) -> np.ndarray:
    return np.array(Image.open(path).convert("RGBA"))


def save_rgba(arr: np.ndarray, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(arr, "RGBA").save(path)


def soft_key_magenta(rgb: np.ndarray) -> np.ndarray:
    img = rgb[:, :, :3].astype(np.int16)
    r, g, b = img[:, :, 0], img[:, :, 1], img[:, :, 2]
    chroma = (r.astype(np.int32) + b.astype(np.int32)) // 2 - g.astype(np.int32)
    alpha = 1.0 - np.clip((chroma.astype(np.float32) - 70.0) / 50.0, 0.0, 1.0)
    bg = ((chroma > 80) & (g < 100) & (np.minimum(r, b) > 120)).astype(np.uint8)
    h, w = bg.shape
    mask = np.zeros((h + 2, w + 2), np.uint8)
    flood = bg.copy()
    for x, y in ((0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)):
        if flood[y, x]:
            cv2.floodFill(flood, mask, (x, y), 2, loDiff=0, upDiff=0)
    bg_cc = flood == 2
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
    out = im.copy()
    rgb = out[:, :, :3].astype(np.int16)
    lum = rgb.mean(axis=2)
    chroma = (rgb[:, :, 0].astype(np.int32) + rgb[:, :, 2].astype(np.int32)) // 2 - rgb[:, :, 1]
    a = out[:, :, 3]
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
    """Pink nose in the head band only — hop paws show pads that look pink."""
    x0, y0, x1, y1 = bbox_alpha(arr)
    head_y1 = y0 + int(0.42 * max(y1 - y0, 1))
    rgb = arr[:, :, :3].astype(np.int16)
    a = arr[:, :, 3]
    r, g, b = rgb[:, :, 0], rgb[:, :, 1], rgb[:, :, 2]
    yy, xx = np.mgrid[:SIZE, :SIZE]
    head = (a >= 40) & (yy >= y0) & (yy < head_y1)
    pink = (
        head
        & (r > 140)
        & (g > 70)
        & (g < 190)
        & (b < 170)
        & (r > g + 8)
        & (r > b)
    )
    ys, xs = np.where(pink)
    if xs.size >= 6:
        return float(xs.mean()), float(ys.mean())
    if head.any():
        return float(xx[head].mean()), float(yy[head].mean())
    return SIT_FACE_CX, SIT_FACE_CY


# Chin / neck on idle_blink/000. Hop heads stay frontal — swap the sit head on.
SIT_NECK_Y = 126


def head_centroid(im: np.ndarray, neck_y: int) -> tuple[float, float]:
    yy, xx = np.mgrid[:SIZE, :SIZE]
    m = (im[:, :, 3] >= 40) & (yy < neck_y)
    if not m.any():
        return SIT_FACE_CX, SIT_FACE_CY
    return float(xx[m].mean()), float(yy[m].mean())


def translate_rgba(im: np.ndarray, dx: int, dy: int) -> np.ndarray:
    out = np.zeros_like(im)
    ys, xs = np.where(im[:, :, 3] > 0)
    if xs.size == 0:
        return out
    ny = ys + dy
    nx = xs + dx
    ok = (ny >= 0) & (ny < SIZE) & (nx >= 0) & (nx < SIZE)
    out[ny[ok], nx[ok]] = im[ys[ok], xs[ok]]
    return out


def lock_sit_head(hop: np.ndarray, sit: np.ndarray, neck_y: int = SIT_NECK_Y) -> np.ndarray:
    """Overlay the sit head (左黑右白) on the hop body. No hole-punch."""
    hx, hy = head_centroid(hop, neck_y)
    sx, sy = head_centroid(sit, neck_y)
    dx = int(round(hx - sx))
    dy = int(round(hy - sy))

    sit_only = sit.copy()
    sit_only[neck_y:, :, :] = 0
    sit_head = translate_rgba(sit_only, dx, dy)
    a = sit_head[:, :, 3:4].astype(np.float32) / 255.0
    out = hop.astype(np.float32) * (1.0 - a) + sit_head.astype(np.float32) * a
    return np.clip(out, 0, 255).astype(np.uint8)


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
    slugs = {
        "gather": "01_gather",
        "launch": "02_launch",
        "air": "03_air",
        "land": "04_land",
    }
    planted = {"gather", "land"}
    for name, src in SOURCES.items():
        if not src.is_file():
            raise SystemExit(f"missing {src}")
        dest = MASTER / f"hop_{slugs[name]}.jpg"
        shutil.copy2(src, dest)
        raw = downscale_premul(soft_key_magenta(load_rgb(src)))
        frame = foot_lock(raw, sit) if name in planted else raw
        # Only the airborne launch needs a sit-head lock — gather/land already
        # match the master. Air reuses launch so the face never redraws mid-hop.
        if name == "launch":
            frame = lock_sit_head(frame, sit)
        save_rgba(frame, MASTER / f"hop_{slugs[name]}_keyed.png")
        x0, y0, x1, y1 = bbox_alpha(frame)
        if y0 <= 0 or y1 >= SIZE:
            raise SystemExit(f"{name} clips letterbox bbox={(x0, y0, x1, y1)}")
        keyed[name] = frame
        print(f"  {name}: bbox={bbox_alpha(frame)}")
    keyed["air"] = keyed["launch"].copy()
    save_rgba(keyed["air"], MASTER / "hop_03_air_keyed.png")

    gather, launch, air, land = keyed["gather"], keyed["launch"], keyed["air"], keyed["land"]

    frames = (
        hold(sit, HOLD_SIT_IN)
        + hold(gather, HOLD_GATHER)
        + hold(launch, HOLD_LAUNCH)
        + hold(air, HOLD_AIR)
        + hold(land, HOLD_LAND)
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

    if not np.array_equal(frames[0], sit):
        raise SystemExit("bookend frame 0 is not pixel-identical to idle_blink/000")
    if not np.array_equal(frames[-1], sit):
        raise SystemExit("bookend last sit hold is not pixel-identical to idle_blink/000")

    meta = {
        "name": "reminder_hop",
        "frame_width": SIZE,
        "frame_height": SIZE,
        "frames": len(frames),
        "fps": FPS,
        "loop": False,
        "anchor": {"x": 128, "y": 220},
        "files": files,
        "source": "master_hop_keys",
        "notes": "Sit bookend exact. Sit face (左黑右白) stamped on hop bodies. No old-cat fallback.",
    }
    (OUT / "meta.json").write_text(json.dumps(meta, indent=2), encoding="utf-8")

    keys = [sit, gather, launch, air, land]
    contact_sheet(keys, QA / "keys_gray.png")
    contact_sheet(keys, QA / "keys_desk.png", bg=(48, 72, 56))
    print(f"packed {len(frames)} frames @{FPS:.0f}fps  dur={len(frames) / FPS:.2f}s  out={OUT}")


if __name__ == "__main__":
    main()
