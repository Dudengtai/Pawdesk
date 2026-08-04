"""Import a desktop screenshot/reference as the pet base sprite.

Removes light gray/white edge-connected background (keeps white fur),
packs tightly to 128x128 with crisp cartoon edges, and regenerates all
cow-cat animation folders with subtle transform variants.
"""

from __future__ import annotations

import json
import sys
from collections import deque
from pathlib import Path

from PIL import Image, ImageEnhance, ImageFilter

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "assets" / "pets" / "cow-cat"
SIZE = 128
ANCHOR = {"x": 64, "y": 112}


def is_magenta_key(r: int, g: int, b: int) -> bool:
    """Pure / near magenta-pink chroma key (and hot pink gen backgrounds)."""
    if r > 160 and b > 140 and g < 170 and r > g + 35 and b > g + 25:
        return True
    if r > 200 and g < 140 and b > 160:
        return True
    # hot pink flats (R high, G mid-low, B high-mid)
    if r > 210 and 40 < g < 160 and b > 160 and r > g + 50:
        return True
    return False


def is_light_bg(r: int, g: int, b: int) -> bool:
    """Light gray / white / soft checkerboard background."""
    mx = max(r, g, b)
    mn = min(r, g, b)
    if mx - mn > 30:
        return False
    if mx >= 205:
        return True
    if mx >= 170 and (r + g + b) / 3 >= 180:
        return True
    return False


def is_bg(r: int, g: int, b: int) -> bool:
    return is_magenta_key(r, g, b) or is_light_bg(r, g, b)


def remove_bg_edge_flood(img: Image.Image) -> Image.Image:
    img = img.convert("RGBA")
    w, h = img.size
    px = img.load()

    # First: hard chroma-key all magenta/pink (not only edge-connected).
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if is_magenta_key(r, g, b):
                px[x, y] = (0, 0, 0, 0)

    mask = [[is_bg(*px[x, y][:3]) and px[x, y][3] > 0 for x in range(w)] for y in range(h)]
    # Rebuild mask for remaining light bg still opaque
    mask = []
    for y in range(h):
        row = []
        for x in range(w):
            r, g, b, a = px[x, y]
            row.append(a > 0 and is_light_bg(r, g, b))
        mask.append(row)

    seen = [[False] * w for _ in range(h)]
    dq: deque[tuple[int, int]] = deque()

    def try_push(x: int, y: int) -> None:
        if 0 <= x < w and 0 <= y < h and not seen[y][x] and mask[y][x]:
            seen[y][x] = True
            dq.append((x, y))

    for x in range(w):
        try_push(x, 0)
        try_push(x, h - 1)
    for y in range(h):
        try_push(0, y)
        try_push(w - 1, y)

    while dq:
        x, y = dq.popleft()
        px[x, y] = (0, 0, 0, 0)
        for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            try_push(x + dx, y + dy)

    # Kill remaining key-color / light fringe adjacent to transparent
    for _ in range(3):
        kill: list[tuple[int, int]] = []
        for y in range(h):
            for x in range(w):
                r, g, b, a = px[x, y]
                if a == 0:
                    continue
                if not is_bg(r, g, b) and not (
                    r > 195 and g > 195 and b > 195 and abs(r - g) < 14
                ):
                    # also soft pink fringe
                    if not (r > 180 and b > 140 and g < 190 and r > g + 20):
                        continue
                for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    nx, ny = x + dx, y + dy
                    if 0 <= nx < w and 0 <= ny < h and px[nx, ny][3] == 0:
                        kill.append((x, y))
                        break
        for x, y in kill:
            px[x, y] = (0, 0, 0, 0)
    return img


def despeckle_alpha(img: Image.Image, min_comp: int = 12) -> Image.Image:
    """Drop tiny opaque islands (noise) not connected to the main body."""
    w, h = img.size
    px = img.load()
    seen = [[False] * w for _ in range(h)]
    keep: set[tuple[int, int]] = set()
    best: list[tuple[int, int]] = []

    for y in range(h):
        for x in range(w):
            if seen[y][x] or px[x, y][3] < 16:
                continue
            comp: list[tuple[int, int]] = []
            dq: deque[tuple[int, int]] = deque([(x, y)])
            seen[y][x] = True
            while dq:
                cx, cy = dq.popleft()
                if px[cx, cy][3] < 16:
                    continue
                comp.append((cx, cy))
                for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    nx, ny = cx + dx, cy + dy
                    if 0 <= nx < w and 0 <= ny < h and not seen[ny][nx]:
                        seen[ny][nx] = True
                        if px[nx, ny][3] >= 16:
                            dq.append((nx, ny))
            if len(comp) > len(best):
                best = comp
            if len(comp) >= min_comp:
                keep.update(comp)

    # Always keep the largest component even if small
    keep.update(best)
    for y in range(h):
        for x in range(w):
            if (x, y) not in keep:
                px[x, y] = (0, 0, 0, 0)
    return img


def polish_edges(img: Image.Image) -> Image.Image:
    """Slight alpha cleanup + contrast so black outlines read cleanly on desktop."""
    img = img.convert("RGBA")
    # Mild unsharp after downscale helps line art
    rgb = img.convert("RGB")
    a = img.getchannel("A")
    rgb = ImageEnhance.Contrast(rgb).enhance(1.08)
    rgb = ImageEnhance.Sharpness(rgb).enhance(1.25)
    # Threshold very low alpha dust
    a = a.point(lambda v: 0 if v < 18 else (255 if v > 230 else v))
    out = rgb.convert("RGBA")
    out.putalpha(a)
    return out


def pack_frame(src: Image.Image, size: int = SIZE) -> Image.Image:
    img = remove_bg_edge_flood(src)
    img = despeckle_alpha(img)
    bbox = img.getbbox()
    if not bbox:
        raise SystemExit("empty after bg remove — check source contrast")
    img = img.crop(bbox)

    # Tight square: ~4% margin so the cat fills the window (was ~10% → tiny).
    side = max(img.size[0], img.size[1], 1)
    side = int(side * 1.04)
    sq = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    ox = (side - img.size[0]) // 2
    # Bias slightly down so feet sit near bottom of hit box
    oy = int((side - img.size[1]) * 0.62)
    sq.paste(img, (ox, oy), img)

    # Downscale via high-quality intermediate for crisp cartoon lines
    hi = sq.resize((size * 2, size * 2), Image.Resampling.LANCZOS)
    out = hi.resize((size, size), Image.Resampling.LANCZOS)
    return polish_edges(out)


def shift(img: Image.Image, dx: int, dy: int) -> Image.Image:
    out = Image.new("RGBA", img.size, (0, 0, 0, 0))
    out.paste(img, (dx, dy), img)
    return out


def scale_about_center(img: Image.Image, s: float) -> Image.Image:
    w, h = img.size
    nw, nh = max(1, int(w * s)), max(1, int(h * s))
    scaled = img.resize((nw, nh), Image.Resampling.LANCZOS)
    out = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    out.paste(scaled, ((w - nw) // 2, (h - nh) // 2), scaled)
    return out


def rotate_about(img: Image.Image, deg: float) -> Image.Image:
    return img.rotate(
        deg, resample=Image.Resampling.BICUBIC, expand=False, fillcolor=(0, 0, 0, 0)
    )


def dim(img: Image.Image, factor: float) -> Image.Image:
    r, g, b, a = img.split()
    rgb = Image.merge("RGB", (r, g, b))
    rgb = ImageEnhance.Brightness(rgb).enhance(factor)
    out = rgb.convert("RGBA")
    out.putalpha(a)
    return out


def write_set(name: str, frames: list[Image.Image], fps: float, loop: bool = True) -> None:
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
    print(f"wrote {name}: {len(files)} frames @ {fps}fps")


def main() -> None:
    src_path = Path(
        sys.argv[1]
        if len(sys.argv) > 1
        else r"C:\Users\lig76\Desktop\ScreenShot_2026-08-04_084011_409.png"
    )
    if not src_path.is_file():
        # Fall back to saved master source
        alt = OUT / "_master" / "base_sit_source.png"
        if alt.is_file():
            src_path = alt
        else:
            raise SystemExit(f"source not found: {src_path}")

    base = pack_frame(Image.open(src_path))
    master = OUT / "_master"
    master.mkdir(exist_ok=True)
    base.save(master / "base_sit.png")
    try:
        Image.open(src_path).save(master / "base_sit_source.png")
    except Exception:
        pass
    print("master base saved", base.size)

    px = list(base.getdata())
    a0 = sum(1 for p in px if p[3] == 0)
    opaque = [p for p in px if p[3] > 20]
    xs = [i % SIZE for i, p in enumerate(px) if p[3] > 20]
    ys = [i // SIZE for i, p in enumerate(px) if p[3] > 20]
    print(
        f"base alpha: a0={a0}/{len(px)} opaque={len(opaque)} "
        f"bbox=({min(xs)},{min(ys)})-({max(xs)},{max(ys)}) "
        f"corner={base.getpixel((0, 0))}"
    )

    # Gentle breathing / bob — small motion so silhouette stays readable
    idle_bob = [
        base,
        shift(base, 0, -1),
        shift(base, 0, -2),
        shift(base, 0, -1),
        base,
        shift(base, 0, 1),
        base,
        shift(base, 0, -1),
    ]
    write_set("idle_tail_wag", idle_bob, 6)
    write_set(
        "idle_stretch",
        [
            base,
            scale_about_center(base, 1.02),
            scale_about_center(shift(base, 0, -2), 1.04),
            scale_about_center(shift(base, 0, -3), 1.05),
            scale_about_center(shift(base, 0, -2), 1.03),
            scale_about_center(base, 1.01),
            base,
            shift(base, 0, 1),
        ],
        6,
    )
    write_set(
        "idle_cute",
        [
            base,
            shift(base, 0, -1),
            scale_about_center(base, 1.02),
            shift(base, 0, -1),
            base,
            shift(base, 1, 0),
        ],
        6,
    )
    write_set(
        "idle_sleep",
        [
            dim(shift(base, 0, 2), 0.94),
            dim(shift(base, 0, 2), 0.90),
            dim(shift(base, 0, 3), 0.92),
            dim(shift(base, 0, 2), 0.90),
            dim(shift(base, 0, 2), 0.94),
            dim(shift(base, 0, 1), 0.96),
        ],
        3,
    )
    write_set(
        "idle_watch",
        [
            base,
            shift(base, 1, 0),
            shift(base, 2, 0),
            shift(base, 1, -1),
            base,
            shift(base, -1, 0),
            shift(base, -2, 0),
            shift(base, -1, -1),
        ],
        6,
    )
    write_set(
        "approaching",
        [
            scale_about_center(base, 0.94),
            scale_about_center(base, 0.96),
            scale_about_center(base, 0.98),
            scale_about_center(base, 1.00),
            scale_about_center(base, 1.02),
            scale_about_center(base, 1.04),
            scale_about_center(base, 1.02),
            scale_about_center(base, 1.00),
        ],
        12,
    )
    write_set(
        "playing_interaction",
        [
            base,
            rotate_about(base, -3),
            rotate_about(shift(base, 0, -1), 3),
            rotate_about(base, -2),
            base,
            shift(base, 0, -1),
        ],
        9,
    )
    write_set(
        "edge_peek",
        [
            shift(base, 0, 16),
            shift(base, 0, 10),
            shift(base, 0, 6),
            shift(base, 0, 10),
        ],
        4,
    )
    write_set(
        "dragging",
        [
            rotate_about(base, -5),
            rotate_about(base, -2),
            rotate_about(base, 2),
            rotate_about(base, 5),
        ],
        8,
    )
    write_set(
        "reminder_wave",
        [
            base,
            rotate_about(shift(base, 0, -1), -4),
            rotate_about(shift(base, 0, -2), 4),
            rotate_about(shift(base, 0, -1), -4),
            base,
            shift(base, 0, -1),
        ],
        7,
    )
    write_set(
        "reminder_feed",
        [
            base,
            scale_about_center(base, 1.03),
            scale_about_center(shift(base, 0, -2), 1.05),
            scale_about_center(base, 1.03),
            base,
            shift(base, 0, 1),
        ],
        7,
    )
    print("all sets written from", src_path)


if __name__ == "__main__":
    main()
