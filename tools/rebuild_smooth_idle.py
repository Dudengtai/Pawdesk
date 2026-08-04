"""Rebuild smooth idle/action/pounce frames from a SINGLE base sprite.

Fixes flicker caused by blending mismatched AI frames (different pose/colors).
All motion is either eyelid paint on the same pixels, or gentle geometry on base.
"""

from __future__ import annotations

import json
import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "assets" / "pets" / "cow-cat"
MASTER = OUT / "_master" / "base_sit.png"
SIZE = 128
ANCHOR = {"x": 64, "y": 112}


def load_base() -> Image.Image:
    if not MASTER.is_file():
        raise SystemExit(f"missing {MASTER}")
    im = Image.open(MASTER).convert("RGBA")
    if im.size != (SIZE, SIZE):
        im = im.resize((SIZE, SIZE), Image.Resampling.LANCZOS)
    return im


def write_set(name: str, frames: list[Image.Image], fps: float, loop: bool) -> None:
    dest = OUT / name
    dest.mkdir(parents=True, exist_ok=True)
    for old in dest.glob("*.png"):
        old.unlink()
    files: list[str] = []
    for i, fr in enumerate(frames):
        fn = f"{i:02d}.png"
        fr.save(dest / fn, optimize=True)
        files.append(fn)
    meta = {
        "name": name,
        "frame_width": SIZE,
        "frame_height": SIZE,
        "frames": len(files),
        "fps": fps,
        "loop": loop,
        "anchor": ANCHOR,
        "files": files,
    }
    (dest / "meta.json").write_text(
        json.dumps(meta, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    print(f"wrote {name}: {len(files)}f loop={loop} @{fps}fps")


def find_eye_centroids(base: Image.Image) -> list[tuple[float, float, float]]:
    """Return list of (cx, cy, radius) for bright yellow-green eye blobs."""
    w, h = base.size
    px = base.load()
    eyes: list[tuple[int, int]] = []
    for y in range(int(h * 0.12), int(h * 0.55)):
        for x in range(int(w * 0.15), int(w * 0.85)):
            r, g, b, a = px[x, y]
            if a < 180:
                continue
            # yellow-green iris / sclera highlight
            if g > 140 and r > 100 and b < 120 and g >= r - 20:
                eyes.append((x, y))
            elif g > 180 and r > 150 and b < 100:
                eyes.append((x, y))
    if len(eyes) < 8:
        # fallback approximate positions for sitting cat
        return [(48.0, 42.0, 7.0), (78.0, 42.0, 7.0)]

    # cluster into 2 groups by x
    xs = sorted(eyes, key=lambda p: p[0])
    mid = xs[len(xs) // 2][0]
    left = [p for p in eyes if p[0] < mid]
    right = [p for p in eyes if p[0] >= mid]
    out = []
    for group in (left, right):
        if not group:
            continue
        cx = sum(p[0] for p in group) / len(group)
        cy = sum(p[1] for p in group) / len(group)
        # radius from spread
        dist = max(math.hypot(p[0] - cx, p[1] - cy) for p in group)
        out.append((cx, cy, max(5.0, min(10.0, dist * 1.2))))
    if len(out) < 2:
        return [(48.0, 42.0, 7.0), (78.0, 42.0, 7.0)]
    return out[:2]


def paint_closed_eyes(base: Image.Image, amount: float) -> Image.Image:
    """amount 0=open, 1=fully closed. Paints soft lids over detected eyes only."""
    amount = max(0.0, min(1.0, amount))
    if amount < 0.02:
        return base.copy()
    img = base.copy()
    overlay = Image.new("RGBA", img.size, (0, 0, 0, 0))
    draw = ImageDraw.Draw(overlay)
    for cx, cy, rad in find_eye_centroids(base):
        # eyelid: dark horizontal ellipse covering the eye
        rx = rad * 1.35
        ry = rad * (0.25 + 0.85 * amount)
        # sample fur color near eye for lid color
        px = base.load()
        samples = []
        for dy in range(-int(rad) - 4, -int(rad) + 2):
            for dx in range(-2, 3):
                x, y = int(cx + dx), int(cy + dy)
                if 0 <= x < base.size[0] and 0 <= y < base.size[1]:
                    r, g, b, a = px[x, y]
                    if a > 200 and max(r, g, b) < 80:
                        samples.append((r, g, b))
        if samples:
            fr = sum(s[0] for s in samples) // len(samples)
            fg = sum(s[1] for s in samples) // len(samples)
            fb = sum(s[2] for s in samples) // len(samples)
        else:
            fr, fg, fb = 18, 18, 22
        alpha = int(240 * amount)
        bbox = [cx - rx, cy - ry * 0.3, cx + rx, cy + ry * 1.1]
        draw.ellipse(bbox, fill=(fr, fg, fb, alpha))
        # soft crease line
        if amount > 0.5:
            draw.arc(
                [cx - rx * 0.9, cy - 1, cx + rx * 0.9, cy + ry * 0.6],
                start=200,
                end=340,
                fill=(fr // 2, fg // 2, fb // 2, int(180 * amount)),
                width=1,
            )
    overlay = overlay.filter(ImageFilter.GaussianBlur(radius=0.6))
    return Image.alpha_composite(img, overlay)


def shift(img: Image.Image, dx: int, dy: int) -> Image.Image:
    out = Image.new("RGBA", img.size, (0, 0, 0, 0))
    out.paste(img, (dx, dy), img)
    return out


def scale_about(img: Image.Image, s: float) -> Image.Image:
    w, h = img.size
    nw, nh = max(1, int(w * s)), max(1, int(h * s))
    scaled = img.resize((nw, nh), Image.Resampling.LANCZOS)
    out = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    out.paste(scaled, ((w - nw) // 2, (h - nh) // 2 + max(0, int((1 - s) * 4))), scaled)
    return out


def rotate_about(img: Image.Image, deg: float) -> Image.Image:
    return img.rotate(
        deg, resample=Image.Resampling.BICUBIC, expand=False, fillcolor=(0, 0, 0, 0)
    )


def main() -> None:
    base = load_base()
    open_e = base
    half = paint_closed_eyes(base, 0.45)
    closed = paint_closed_eyes(base, 1.0)

    # ── idle_blink: only 3 unique poses (open / half / closed).
    # Runtime holds open for ~3.5s then blinks (see PetController::tick_blink_hold).
    write_set("idle_blink", [open_e, half, closed], fps=1.0, loop=True)

    # ── one-shot actions: same base geometry only ──
    write_set(
        "idle_stretch",
        [
            open_e,
            scale_about(open_e, 1.02),
            scale_about(shift(open_e, 0, -2), 1.05),
            scale_about(shift(open_e, 0, -3), 1.07),
            scale_about(shift(open_e, 0, -2), 1.04),
            scale_about(open_e, 1.01),
            open_e,
        ],
        fps=8.0,
        loop=False,
    )
    write_set(
        "idle_cute",
        [
            open_e,
            shift(open_e, 0, -1),
            rotate_about(open_e, -3),
            paint_closed_eyes(rotate_about(open_e, -3), 0.3),
            rotate_about(open_e, -2),
            open_e,
        ],
        fps=8.0,
        loop=False,
    )
    write_set(
        "idle_tail_wag",
        [
            open_e,
            rotate_about(open_e, -1.5),
            rotate_about(open_e, 1.5),
            rotate_about(open_e, -1.5),
            rotate_about(open_e, 1.5),
            open_e,
        ],
        fps=10.0,
        loop=False,
    )
    write_set(
        "idle_sleep",
        [
            half,
            closed,
            shift(closed, 0, 1),
            closed,
            half,
            open_e,
        ],
        fps=4.0,
        loop=False,
    )
    write_set(
        "idle_watch",
        [
            open_e,
            shift(open_e, 1, 0),
            shift(open_e, 2, 0),
            shift(open_e, 1, 0),
            open_e,
            shift(open_e, -1, 0),
            shift(open_e, -2, 0),
            open_e,
        ],
        fps=5.0,
        loop=True,
    )

    # ── pounce: crouch / leap / land on SAME base (readable silhouette motion) ──
    crouch = scale_about(shift(open_e, 0, 6), 0.92)
    leap = scale_about(shift(open_e, 6, -12), 1.08)
    leap2 = scale_about(shift(open_e, 10, -8), 1.05)
    land = scale_about(shift(open_e, 2, 2), 0.96)
    write_set(
        "approaching",
        [open_e, crouch, leap, leap2, land, open_e],
        fps=14.0,
        loop=False,
    )

    write_set(
        "playing_interaction",
        [
            open_e,
            rotate_about(open_e, -4),
            shift(open_e, 0, -2),
            rotate_about(open_e, 4),
            paint_closed_eyes(open_e, 0.2),
            open_e,
        ],
        fps=10.0,
        loop=True,
    )
    write_set(
        "dragging",
        [
            rotate_about(open_e, -6),
            rotate_about(open_e, -3),
            rotate_about(open_e, 3),
            rotate_about(open_e, 6),
        ],
        fps=8.0,
        loop=True,
    )
    write_set(
        "edge_peek",
        [
            shift(open_e, 0, 18),
            shift(open_e, 0, 12),
            shift(open_e, 0, 8),
            shift(open_e, 0, 12),
        ],
        fps=4.0,
        loop=True,
    )
    write_set(
        "reminder_wave",
        [
            open_e,
            rotate_about(shift(open_e, 0, -1), -5),
            rotate_about(shift(open_e, 0, -2), 5),
            rotate_about(shift(open_e, 0, -1), -5),
            open_e,
            shift(open_e, 0, -1),
        ],
        fps=8.0,
        loop=True,
    )
    write_set(
        "reminder_feed",
        [
            open_e,
            scale_about(open_e, 1.03),
            scale_about(shift(open_e, 0, -2), 1.05),
            scale_about(open_e, 1.02),
            open_e,
        ],
        fps=8.0,
        loop=True,
    )
    print("smooth idle rebuilt from single base_sit — no cross-identity blends")


if __name__ == "__main__":
    main()
