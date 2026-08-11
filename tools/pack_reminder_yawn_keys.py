"""Build reminder_wave: closed → yawn (pink tongue) → slow close → loop.

Keys (magenta or transparent sit sprites under _master/):
  closed = base_sit.png
  start  = yawn_01_start_magenta.jpg
  half   = yawn_02_half_magenta.jpg
  full   = yawn_03_full_magenta.jpg  (wide open + vivid pink tongue)
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np
from PIL import Image

_TOOLS = Path(__file__).resolve().parent
if str(_TOOLS) not in sys.path:
    sys.path.insert(0, str(_TOOLS))

from extract_video_frames import ANCHOR, SIZE  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
MASTER = ROOT / "assets" / "pets" / "cow-cat" / "_master"
OUT = ROOT / "assets" / "pets" / "cow-cat" / "reminder_wave"

FPS = 12.0
N_CLOSED = 4
N_OPEN = 12
N_HOLD = 10
N_CLOSE = 18  # slower close
FOOT_Y = 224


def ease_out(t: float) -> float:
    t = max(0.0, min(1.0, t))
    return 1.0 - (1.0 - t) ** 2.0


def ease_in(t: float) -> float:
    t = max(0.0, min(1.0, t))
    return t**1.55


def is_pure_magenta_bg(r: int, g: int, b: int) -> bool:
    """Solid generator magenta only — must NOT match pink tongue.

    Pink tongue is ~ (255, 110, 155): g is mid, so require very low green.
    """
    if r > 210 and b > 200 and g < 70 and r > g + 100 and b > g + 90:
        return True
    if r > 230 and b > 220 and g < 100 and abs(r - b) < 50:
        return True
    if r > 240 and g < 30 and b > 240:
        return True
    return False


def key_magenta_preserve_tongue(im: Image.Image) -> Image.Image:
    """Remove flat magenta backdrop without punching out pink tongue."""
    im = im.convert("RGBA")
    px = im.load()
    w, h = im.size
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if is_pure_magenta_bg(r, g, b):
                px[x, y] = (0, 0, 0, 0)
    # Soft fringe: only near-pure-magenta next to already transparent
    for _ in range(2):
        kill: list[tuple[int, int]] = []
        for y in range(h):
            for x in range(w):
                r, g, b, a = px[x, y]
                if a == 0 or g > 90:
                    continue
                if not (
                    r > 180
                    and b > 160
                    and g < 90
                    and r > g + 60
                    and b > g + 50
                ):
                    continue
                for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    nx, ny = x + dx, y + dy
                    if 0 <= nx < w and 0 <= ny < h and px[nx, ny][3] == 0:
                        kill.append((x, y))
                        break
        for x, y in kill:
            px[x, y] = (0, 0, 0, 0)
    return im


def pack_yawn(im: Image.Image) -> Image.Image:
    im = key_magenta_preserve_tongue(im)
    bbox = im.getbbox()
    if not bbox:
        return Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    im = im.crop(bbox)
    margin = int(SIZE * 0.04)
    max_w = SIZE - margin * 2
    max_h = SIZE - margin * 2
    scale = min(max_w / im.size[0], max_h / im.size[1])
    nw = max(1, int(im.size[0] * scale))
    nh = max(1, int(im.size[1] * scale))
    im = im.resize((nw, nh), Image.Resampling.LANCZOS)
    canvas = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    ox = (SIZE - nw) // 2
    oy = FOOT_Y - nh
    oy = max(margin // 2, min(oy, SIZE - nh - margin // 2))
    canvas.paste(im, (ox, oy), im)
    return canvas


def blend_rgba(a: Image.Image, b: Image.Image, t: float) -> Image.Image:
    t = max(0.0, min(1.0, t))
    aa = np.asarray(a.convert("RGBA"), dtype=np.float32)
    bb = np.asarray(b.convert("RGBA"), dtype=np.float32)
    a_a = aa[:, :, 3:4] / 255.0
    b_a = bb[:, :, 3:4] / 255.0
    out_rgb = aa[:, :, :3] * a_a * (1.0 - t) + bb[:, :, :3] * b_a * t
    out_a = a_a * (1.0 - t) + b_a * t
    out = np.zeros_like(aa)
    mask = out_a[:, :, 0] > 1e-4
    out[mask, :3] = out_rgb[mask] / np.maximum(out_a[mask], 1e-6)
    out[:, :, 3:4] = out_a * 255.0
    return Image.fromarray(np.clip(out, 0, 255).astype(np.uint8), "RGBA")


def load_key(path: Path, *, already_rgba_sit: bool = False) -> Image.Image:
    im = Image.open(path).convert("RGBA")
    if already_rgba_sit and im.size == (SIZE, SIZE):
        return im.copy()
    return pack_yawn(im)


def sample_chain(keys: list[Image.Image], n: int, ease) -> list[Image.Image]:
    if n <= 0:
        return []
    if len(keys) == 1:
        return [keys[0].copy() for _ in range(n)]
    segs = len(keys) - 1
    out: list[Image.Image] = []
    for i in range(n):
        t = ease(i / max(1, n - 1)) if n > 1 else 0.0
        f = t * segs
        si = min(segs - 1, int(f))
        local = f - si
        out.append(blend_rgba(keys[si], keys[si + 1], local))
    return out


def main() -> None:
    closed_path = MASTER / "base_sit.png"
    start_path = MASTER / "yawn_01_start_magenta.jpg"
    half_path = MASTER / "yawn_02_half_magenta.jpg"
    full_path = MASTER / "yawn_03_full_magenta.jpg"
    for p in (closed_path, start_path, half_path, full_path):
        if not p.is_file():
            raise SystemExit(f"missing key: {p}")

    closed = load_key(closed_path, already_rgba_sit=True)
    start = load_key(start_path)
    half = load_key(half_path)
    full = load_key(full_path)

    frames: list[Image.Image] = []
    frames.extend(closed.copy() for _ in range(N_CLOSED))
    frames.extend(sample_chain([closed, start, half, full], N_OPEN, ease_out))
    for i in range(N_HOLD):
        # micro life: barely breathe between full and half
        wobble = 0.05 if (i % 4 == 0) else 0.0
        frames.append(blend_rgba(full, half, wobble))
    frames.extend(sample_chain([full, half, start, closed], N_CLOSE, ease_in))

    OUT.mkdir(parents=True, exist_ok=True)
    for old in OUT.glob("*.png"):
        old.unlink()

    files: list[str] = []
    for i, fr in enumerate(frames):
        fn = f"{i:02d}.png"
        fr.save(OUT / fn, optimize=True)
        files.append(fn)

    meta = {
        "name": "reminder_wave",
        "frame_width": SIZE,
        "frame_height": SIZE,
        "frames": len(files),
        "fps": FPS,
        "loop": True,
        "anchor": ANCHOR,
        "files": files,
        "source": "yawn_keys_pink_tongue",
        "notes": (
            f"Closed→yawn (pink tongue)→hold→slow close; "
            f"{len(files)/FPS:.2f}s/cycle @{FPS}fps"
        ),
    }
    (OUT / "meta.json").write_text(
        json.dumps(meta, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    print(
        f"wrote reminder_wave: {len(files)}f @{FPS}fps "
        f"cycle={len(files)/FPS:.2f}s "
        f"(closed={N_CLOSED} open={N_OPEN} hold={N_HOLD} close={N_CLOSE})"
    )


if __name__ == "__main__":
    main()
