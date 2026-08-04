"""High-quality pet rebuild: single identity, stable anchors, 256px sprites.

Sources (session images, edit-chained from master 31):
  31 = base sit open
  35 = blink closed (same sit)
  32 = cute head-tilt
  34 = stretch
  33 = pounce leap
"""

from __future__ import annotations

import json
from pathlib import Path

from PIL import Image, ImageFilter

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "assets" / "pets" / "cow-cat"
MASTER_DIR = OUT / "_master"
SESS = Path(
    r"C:\Users\lig76\.grok\sessions\D%3A%5CAI%E7%BB%83%E4%B9%A0%E7%9B%AE%E5%BD%95%5CPawDesk\019fbda6-d039-7c31-8611-5f13cfd446e6\images"
)
SIZE = 256
# Feet sit near this Y in the canvas (keeps sit poses from hopping).
FOOT_Y = 236
ANCHOR = {"x": 128, "y": 220}


def is_magenta(r: int, g: int, b: int) -> bool:
    if r > 160 and b > 140 and g < 180 and r > g + 30 and b > g + 20:
        return True
    if r > 200 and g < 150 and b > 150:
        return True
    if r > 210 and 40 < g < 170 and b > 150 and r > g + 40:
        return True
    return False


def key_magenta(im: Image.Image) -> Image.Image:
    im = im.convert("RGBA")
    px = im.load()
    w, h = im.size
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if is_magenta(r, g, b):
                px[x, y] = (0, 0, 0, 0)
    # fringe cleanup: near-magenta next to transparent
    for _ in range(2):
        kill = []
        for y in range(h):
            for x in range(w):
                r, g, b, a = px[x, y]
                if a == 0:
                    continue
                if not (
                    is_magenta(r, g, b)
                    or (r > 180 and b > 120 and g < 160 and r > g + 25)
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


def pack_sit(im: Image.Image, size: int = SIZE) -> Image.Image:
    """Pack full-body sit with feet locked to FOOT_Y for stable idle."""
    im = key_magenta(im)
    bbox = im.getbbox()
    if not bbox:
        raise SystemExit("empty after key")
    im = im.crop(bbox)
    # Fit inside size with margin, keep aspect
    margin = int(size * 0.04)
    max_w = size - margin * 2
    max_h = size - margin * 2
    scale = min(max_w / im.size[0], max_h / im.size[1])
    nw = max(1, int(im.size[0] * scale))
    nh = max(1, int(im.size[1] * scale))
    im = im.resize((nw, nh), Image.Resampling.LANCZOS)
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    ox = (size - nw) // 2
    # Align bottom of sprite near FOOT_Y
    oy = FOOT_Y - nh
    oy = max(margin // 2, min(oy, size - nh - margin // 2))
    canvas.paste(im, (ox, oy), im)
    return canvas


def pack_action(im: Image.Image, size: int = SIZE) -> Image.Image:
    """Center-fit any pose (stretch/pounce) with small margin."""
    im = key_magenta(im)
    bbox = im.getbbox()
    if not bbox:
        raise SystemExit("empty action")
    im = im.crop(bbox)
    margin = int(size * 0.06)
    max_w = size - margin * 2
    max_h = size - margin * 2
    scale = min(max_w / im.size[0], max_h / im.size[1])
    nw = max(1, int(im.size[0] * scale))
    nh = max(1, int(im.size[1] * scale))
    im = im.resize((nw, nh), Image.Resampling.LANCZOS)
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    ox = (size - nw) // 2
    oy = (size - nh) // 2 + int(size * 0.04)
    canvas.paste(im, (ox, oy), im)
    return canvas


def blend(a: Image.Image, b: Image.Image, t: float) -> Image.Image:
    t = max(0.0, min(1.0, t))
    return Image.blend(a.convert("RGBA"), b.convert("RGBA"), t)


def write_set(name: str, frames: list[Image.Image], fps: float, loop: bool) -> None:
    dest = OUT / name
    dest.mkdir(parents=True, exist_ok=True)
    for old in dest.glob("*.png"):
        old.unlink()
    files = []
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
    print(f"  {name}: {len(files)}f @{fps}fps loop={loop}")


def main() -> None:
    base_src = SESS / "31.jpg"
    blink_src = SESS / "35.jpg"
    cute_src = SESS / "32.jpg"
    stretch_src = SESS / "34.jpg"
    pounce_src = SESS / "33.jpg"
    for p in (base_src, blink_src, cute_src, stretch_src, pounce_src):
        if not p.is_file():
            raise SystemExit(f"missing {p}")

    print("packing sit-stable frames…")
    sit = pack_sit(Image.open(base_src))
    blink = pack_sit(Image.open(blink_src))
    # Soft half-blink from same-canvas blend (no silhouette jump)
    half = blend(sit, blink, 0.55)

    cute = pack_sit(Image.open(cute_src))
    stretch = pack_action(Image.open(stretch_src))
    pounce = pack_action(Image.open(pounce_src))

    MASTER_DIR.mkdir(parents=True, exist_ok=True)
    sit.save(MASTER_DIR / "base_sit.png")
    Image.open(base_src).save(MASTER_DIR / "base_sit_source.png")
    # Also save 128 preview for tools that expect 128
    sit.resize((128, 128), Image.Resampling.LANCZOS).save(MASTER_DIR / "base_sit_128.png")

    print("writing animation sets…")
    # Blink: runtime uses frames 0/1/2 with long hold (PetController::tick_blink_hold)
    write_set("idle_blink", [sit, half, blink], fps=1.0, loop=True)

    # Cute one-shot: gentle ease in/out via blends on sit-stable canvas
    write_set(
        "idle_cute",
        [
            sit,
            blend(sit, cute, 0.35),
            blend(sit, cute, 0.7),
            cute,
            blend(sit, cute, 0.7),
            blend(sit, cute, 0.35),
            sit,
        ],
        fps=10.0,
        loop=False,
    )

    write_set(
        "idle_stretch",
        [
            sit,
            blend(sit, stretch, 0.3),
            blend(sit, stretch, 0.65),
            stretch,
            stretch,
            blend(sit, stretch, 0.5),
            sit,
        ],
        fps=9.0,
        loop=False,
    )

    # Tail / sleep as soft blink-based one-shots (same sit canvas)
    write_set(
        "idle_tail_wag",
        [
            sit,
            blend(sit, cute, 0.2),
            sit,
            blend(sit, cute, 0.15),
            sit,
            blend(sit, cute, 0.2),
            sit,
        ],
        fps=8.0,
        loop=False,
    )
    write_set(
        "idle_sleep",
        [
            half,
            blink,
            blink,
            half,
            sit,
        ],
        fps=3.0,
        loop=False,
    )
    write_set(
        "idle_watch",
        [
            sit,
            blend(sit, cute, 0.25),
            blend(sit, cute, 0.4),
            blend(sit, cute, 0.25),
            sit,
            blend(sit, cute, 0.2),
            sit,
        ],
        fps=5.0,
        loop=True,
    )

    # Pounce storyboard (motion-synced in PetController::tick_pounce_synced):
    #   0 sit → 1 crouch → 2 takeoff → 3-4 air → 5 land → 6-7 recover
    # Prefer AI keyframes when present (edit-chained from master); no single hard cut.
    crouch_src = SESS / "37.jpg"  # crouch
    takeoff_src = SESS / "36.jpg"  # takeoff
    land_src = SESS / "38.jpg"  # land
    # Fallback if renumbered: try 36/37/38 in any order via pack_action
    crouch = pack_action(Image.open(crouch_src)) if crouch_src.is_file() else sit
    takeoff = pack_action(Image.open(takeoff_src)) if takeoff_src.is_file() else pounce
    land = pack_action(Image.open(land_src)) if land_src.is_file() else crouch
    # Mid-air hold: pounce key (33) + slight vertical bias copies for 2 frames
    air1 = pounce
    air2 = _shift(pounce, 0, -4)

    write_set(
        "approaching",
        [
            sit,  # 0 wind-up still
            crouch,  # 1 coil
            takeoff,  # 2 leave ground
            air1,  # 3 peak
            air2,  # 4 still airborne
            land,  # 5 touchdown
            land,  # 6 hold land (avoid ghost blend with sit)
            sit,  # 7 recover
        ],
        fps=16.0,
        loop=False,
    )

    write_set(
        "playing_interaction",
        [
            sit,
            blend(sit, cute, 0.5),
            cute,
            blend(sit, cute, 0.6),
            half,
            sit,
        ],
        fps=10.0,
        loop=True,
    )
    write_set(
        "dragging",
        [blend(sit, cute, 0.15), sit, blend(sit, cute, 0.2), sit],
        fps=8.0,
        loop=True,
    )
    write_set(
        "edge_peek",
        [
            # shift sit down for peek
            _shift(sit, 0, 40),
            _shift(sit, 0, 28),
            _shift(sit, 0, 20),
            _shift(sit, 0, 28),
        ],
        fps=4.0,
        loop=True,
    )
    write_set(
        "reminder_wave",
        [
            sit,
            blend(sit, cute, 0.45),
            cute,
            blend(sit, cute, 0.45),
            sit,
            half,
        ],
        fps=8.0,
        loop=True,
    )
    write_set(
        "reminder_feed",
        [
            sit,
            blend(sit, cute, 0.5),
            cute,
            half,
            sit,
        ],
        fps=8.0,
        loop=True,
    )
    print("done — 256px quality pet from chained master")


def _shift(im: Image.Image, dx: int, dy: int) -> Image.Image:
    out = Image.new("RGBA", im.size, (0, 0, 0, 0))
    out.paste(im, (dx, dy), im)
    return out


if __name__ == "__main__":
    main()
