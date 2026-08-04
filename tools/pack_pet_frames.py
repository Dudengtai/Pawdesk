"""Chroma-key, center, resize to 128, pack pet animation folders + meta.json."""

from __future__ import annotations

import json
import os
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
IMG = Path(
    r"C:\Users\lig76\.grok\sessions\D%3A%5CAI%E7%BB%83%E4%B9%A0%E7%9B%AE%E5%BD%95%5CPawDesk\019fbda6-d039-7c31-8611-5f13cfd446e6\images"
)
OUT = ROOT / "assets" / "pets" / "cow-cat"
SIZE = 128
ANCHOR = {"x": 64, "y": 110}


def chroma_key(img: Image.Image) -> Image.Image:
    img = img.convert("RGBA")
    px = img.load()
    w, h = img.size
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            # Magenta / hot pink key
            if r > 170 and b > 160 and g < 170:
                px[x, y] = (0, 0, 0, 0)
            elif r > 200 and g < 140 and b > 170:
                px[x, y] = (0, 0, 0, 0)
            # fringe soft key near magenta
            elif r > 190 and b > 150 and g < 190 and abs(r - b) < 40:
                px[x, y] = (r, g, b, 0)
    return img


def pack_frame(src: Image.Image) -> Image.Image:
    img = chroma_key(src)
    bbox = img.getbbox()
    if bbox:
        img = img.crop(bbox)
    # pad square with vertical bias (paws lower)
    side = max(img.size[0], img.size[1], 1)
    # add 8% margin
    side = int(side * 1.12)
    sq = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    ox = (side - img.size[0]) // 2
    oy = int((side - img.size[1]) * 0.58)
    sq.paste(img, (ox, oy), img)
    return sq.resize((SIZE, SIZE), Image.Resampling.LANCZOS)


def load_n(n: int) -> Image.Image:
    path = IMG / f"{n}.jpg"
    if not path.exists():
        path = IMG / f"{n}.png"
    return Image.open(path)


def lerp_frames(a: Image.Image, b: Image.Image, t: float) -> Image.Image:
    a = a.convert("RGBA")
    b = b.convert("RGBA")
    if a.size != b.size:
        b = b.resize(a.size, Image.Resampling.LANCZOS)
    return Image.blend(a, b, t)


def expand_sequence(packed: list[Image.Image], target_len: int) -> list[Image.Image]:
    if len(packed) >= target_len:
        return packed[:target_len]
    if len(packed) == 1:
        return packed * target_len
    out: list[Image.Image] = []
    # walk keys with interpolation so length reaches target
    steps = target_len
    n_seg = len(packed)
    for i in range(steps):
        # ping-pong along keyframes for loop friendliness
        phase = (i / steps) * (n_seg - 1)
        i0 = int(phase)
        i1 = min(i0 + 1, n_seg - 1)
        t = phase - i0
        out.append(lerp_frames(packed[i0], packed[i1], t))
    return out


def write_set(name: str, key_ids: list[int], frames: int, fps: float, loop: bool = True) -> None:
    keys = [pack_frame(load_n(i)) for i in key_ids]
    seq = expand_sequence(keys, frames)
    # ping-pong expand for tail wag style loops if short
    if len(key_ids) >= 2 and frames >= 6:
        # rebuild as cycle through keys then reverse (excluding ends)
        cycle = keys + keys[-2:0:-1]
        seq = expand_sequence(cycle, frames)

    dest = OUT / name
    dest.mkdir(parents=True, exist_ok=True)
    # clear old pngs
    for old in dest.glob("*.png"):
        old.unlink()

    files = []
    for i, fr in enumerate(seq):
        fn = f"{i:02d}.png"
        fr.save(dest / fn)
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
    (dest / "meta.json").write_text(json.dumps(meta, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"wrote {name}: {len(files)} frames @ {fps}fps")


def main() -> None:
    # Keyframe map (source image numbers in session images/)
    write_set("idle_tail_wag", [1, 13, 1, 14, 1, 13, 11, 1], frames=8, fps=10)
    write_set("idle_stretch", [1, 3, 15, 3, 1, 3, 15, 1], frames=8, fps=8)
    write_set("idle_cute", [1, 4, 12, 4, 11, 4], frames=6, fps=8)
    write_set("idle_sleep", [5, 10, 5, 10, 5, 10], frames=6, fps=5)
    write_set("idle_watch", [1, 6, 17, 6, 1, 17, 6, 1], frames=8, fps=8)

    write_set("approaching", [7, 19, 7, 19, 7, 19, 7, 19], frames=8, fps=12)
    write_set("playing_interaction", [4, 12, 7, 12, 4, 11], frames=6, fps=10)
    write_set("edge_peek", [8, 8, 11, 8], frames=4, fps=4)
    write_set("dragging", [9, 9, 11, 9], frames=4, fps=8)
    write_set("reminder_wave", [16, 16, 4, 16, 1, 16], frames=6, fps=8)
    write_set("reminder_feed", [18, 18, 11, 18, 1, 18], frames=6, fps=8)

    # refresh master base
    master = OUT / "_master"
    master.mkdir(exist_ok=True)
    pack_frame(load_n(1)).save(master / "base_sit.png")
    print("done")


if __name__ == "__main__":
    main()
