"""Pack idle_stretch from stretch.mp4 as a fluid one-shot.

- Magenta key only (never key black fur)
- Foot-lock pack to 256
- Subsample to ~N frames @ 30fps
- Bookend blend to master base_sit so sit → action → sit is seamless with crossfade

Run:  python tools/pack_stretch_from_video.py
"""

from __future__ import annotations

import json
from pathlib import Path

import imageio.v3 as iio
import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "assets" / "pets" / "cow-cat"
VIDEO = OUT / "_video" / "stretch.mp4"
MASTER = OUT / "_master" / "base_sit.png"
SIZE = 256
# Match facefix master: keep paws above canvas edge so scale/restore won't clip feet.
FOOT_Y = 224
ANCHOR = {"x": 128, "y": FOOT_Y}
# ~2.4s one-shot at 30fps (readable; runtime may stretch slightly)
TARGET_FRAMES = 72
FPS = 30.0
BOOKEND = 6  # blend first/last frames toward sit identity


def is_magenta_bg(r: int, g: int, b: int) -> bool:
    if r > 150 and b > 130 and g < 190 and r > g + 25 and b > g + 15:
        return True
    if r > 190 and g < 160 and b > 140 and r > g + 30:
        return True
    if r > 200 and 80 < g < 180 and b > 160 and r > g + 40 and abs(r - b) < 80:
        return True
    return False


def key_frame(im: Image.Image) -> Image.Image:
    im = im.convert("RGBA")
    px = im.load()
    w, h = im.size
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if is_magenta_bg(r, g, b):
                px[x, y] = (0, 0, 0, 0)
    # fringe next to transparent
    for _ in range(2):
        kill: list[tuple[int, int]] = []
        for y in range(h):
            for x in range(w):
                r, g, b, a = px[x, y]
                if a == 0 or max(r, g, b) < 90:
                    continue
                if not is_magenta_bg(r, g, b) and not (
                    r > 160 and b > 100 and g < 200 and r > g + 10
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


def pack_sit(im: Image.Image) -> Image.Image:
    """Pack full-body (incl. long horizontal stretch) with feet near FOOT_Y."""
    im = key_frame(im)
    bbox = im.getbbox()
    if not bbox:
        return Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    im = im.crop(bbox)
    # Tighter margin so a long left-facing stretch still fills width.
    margin = int(SIZE * 0.03)
    max_w = SIZE - margin * 2
    max_h = SIZE - margin * 2
    scale = min(max_w / im.size[0], max_h / im.size[1])
    nw = max(1, int(im.size[0] * scale))
    nh = max(1, int(im.size[1] * scale))
    im = im.resize((nw, nh), Image.Resampling.LANCZOS)
    canvas = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    # Center horizontally (body stretches left-right in frame).
    ox = (SIZE - nw) // 2
    # Lock bottom of content near FOOT_Y (stable desk contact)
    oy = FOOT_Y - nh
    oy = max(margin // 2, min(oy, SIZE - nh - margin // 2))
    canvas.paste(im, (ox, oy), im)
    return canvas


def blend_rgba(a: Image.Image, b: Image.Image, t: float) -> Image.Image:
    """Premultiplied-ish lerp in float."""
    t = max(0.0, min(1.0, t))
    aa = np.asarray(a.convert("RGBA"), dtype=np.float32)
    bb = np.asarray(b.convert("RGBA"), dtype=np.float32)
    a_a = aa[:, :, 3:4] / 255.0
    b_a = bb[:, :, 3:4] / 255.0
    ar = aa[:, :, :3] * a_a
    br = bb[:, :, :3] * b_a
    out_rgb = ar * (1 - t) + br * t
    out_a = a_a * (1 - t) + b_a * t
    out = np.zeros_like(aa)
    mask = out_a[:, :, 0] > 1e-4
    out[mask, :3] = out_rgb[mask] / out_a[mask]
    out[:, :, 3:4] = out_a * 255.0
    return Image.fromarray(np.clip(out, 0, 255).astype(np.uint8), "RGBA")


def main() -> None:
    if not VIDEO.is_file():
        raise SystemExit(f"missing {VIDEO}")
    sit = Image.open(MASTER).convert("RGBA")
    if sit.size != (SIZE, SIZE):
        sit = sit.resize((SIZE, SIZE), Image.Resampling.LANCZOS)

    arrs = list(iio.imiter(VIDEO, plugin="FFMPEG"))
    n = len(arrs)
    print(f"video frames: {n}")
    if n < 8:
        raise SystemExit("too few frames")

    # Sample evenly; drop a few head/tail frames if model freezes on sit
    start_i = max(0, int(n * 0.02))
    end_i = min(n - 1, int(n * 0.98))
    span = max(1, end_i - start_i)
    idxs = [
        start_i + int(round(i * span / max(1, TARGET_FRAMES - 1)))
        for i in range(TARGET_FRAMES)
    ]

    frames: list[Image.Image] = []
    for i in idxs:
        im = Image.fromarray(arrs[i]).convert("RGBA")
        frames.append(pack_sit(im))

    # Bookend: first BOOKEND ease from master sit → first action frame
    # last BOOKEND ease last action → master sit
    for i in range(BOOKEND):
        t = (i + 1) / (BOOKEND + 1)
        frames[i] = blend_rgba(sit, frames[i], t)
    for i in range(BOOKEND):
        idx = len(frames) - 1 - i
        t = (i + 1) / (BOOKEND + 1)  # 1 at end means full sit
        # at last frame t_bookend -> almost sit; force last = sit
        frames[idx] = blend_rgba(frames[idx], sit, t)
    frames[0] = sit.copy()
    frames[-1] = sit.copy()

    dest = OUT / "idle_stretch"
    dest.mkdir(parents=True, exist_ok=True)
    for old in dest.glob("*.png"):
        old.unlink()
    files = []
    for i, fr in enumerate(frames):
        fn = f"{i:02d}.png"
        fr.save(dest / fn, optimize=True)
        files.append(fn)
    meta = {
        "name": "idle_stretch",
        "frame_width": SIZE,
        "frame_height": SIZE,
        "frames": len(files),
        "fps": FPS,
        "loop": False,
        "anchor": ANCHOR,
        "files": files,
        "source": "video_stretch_v3_side_left",
        "notes": "Side profile face LEFT: sit→horizontal stretch across desk→return sit",
        "facing": "left",
    }
    (dest / "meta.json").write_text(
        json.dumps(meta, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    print(f"wrote idle_stretch: {len(files)}f @{FPS}fps bookend={BOOKEND}")


if __name__ == "__main__":
    main()
