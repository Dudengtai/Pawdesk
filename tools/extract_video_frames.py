"""Extract frames from a pet video, chroma-key MAGENTA only, pack to 256 sprites.

IMPORTANT: Never key pure black — tuxedo fur is black and would become holes.
Background removal is edge-connected magenta/pink flood-fill only.
"""

from __future__ import annotations

import argparse
import json
from collections import deque
from pathlib import Path

import imageio.v3 as iio
from PIL import Image

SIZE = 256
FOOT_Y = 236
ANCHOR = {"x": 128, "y": 220}


def is_magenta_bg(r: int, g: int, b: int) -> bool:
    """Magenta / hot pink solid backdrop only — never black fur."""
    # classic chroma magenta
    if r > 150 and b > 130 and g < 190 and r > g + 25 and b > g + 15:
        return True
    if r > 190 and g < 160 and b > 140 and r > g + 30:
        return True
    # soft pink key sometimes used by video models
    if r > 200 and 80 < g < 180 and b > 160 and r > g + 40 and abs(r - b) < 80:
        return True
    return False


def key_frame(im: Image.Image) -> Image.Image:
    """Remove only edge-connected magenta background; preserve black fur."""
    im = im.convert("RGBA")
    w, h = im.size
    px = im.load()

    # 1) Hard-key obvious magenta everywhere (not black)
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if is_magenta_bg(r, g, b):
                px[x, y] = (0, 0, 0, 0)

    # 2) Flood from edges for remaining near-magenta connected to border
    mask = [[False] * w for _ in range(h)]
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if a == 0:
                continue
            # only candidates that look like key color (not fur)
            if is_magenta_bg(r, g, b) or (
                r > 170 and b > 110 and g < 200 and r > g + 15 and b > g
            ):
                mask[y][x] = True

    seen = [[False] * w for _ in range(h)]
    dq: deque[tuple[int, int]] = deque()

    def push(x: int, y: int) -> None:
        if 0 <= x < w and 0 <= y < h and not seen[y][x] and mask[y][x]:
            seen[y][x] = True
            dq.append((x, y))

    for x in range(w):
        push(x, 0)
        push(x, h - 1)
    for y in range(h):
        push(0, y)
        push(w - 1, y)

    while dq:
        x, y = dq.popleft()
        px[x, y] = (0, 0, 0, 0)
        for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
            push(x + dx, y + dy)

    # 3) Soft fringe: only near-magenta next to already-transparent
    for _ in range(2):
        kill: list[tuple[int, int]] = []
        for y in range(h):
            for x in range(w):
                r, g, b, a = px[x, y]
                if a == 0:
                    continue
                # do NOT remove dark/black pixels (fur)
                if max(r, g, b) < 90:
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
    im = key_frame(im)
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


def extract(video: Path, target_count: int) -> list[Image.Image]:
    arrs = list(iio.imiter(video, plugin="FFMPEG"))
    if not arrs:
        raise SystemExit(f"no frames in {video}")
    n = len(arrs)
    if target_count <= 0 or target_count >= n:
        idxs = list(range(n))
    else:
        idxs = [int(round(i * (n - 1) / max(1, target_count - 1))) for i in range(target_count)]
    out = []
    for i in idxs:
        a = arrs[i]
        im = Image.fromarray(a).convert("RGBA")
        out.append(pack_sit(im))
    return out


def write_set(
    name: str, frames: list[Image.Image], fps: float, loop: bool, out_root: Path
) -> None:
    dest = out_root / name
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
        "source": "video_extract",
    }
    (dest / "meta.json").write_text(
        json.dumps(meta, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    print(f"wrote {name}: {len(files)}f @{fps}fps loop={loop}")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--video", required=True)
    ap.add_argument("--name", required=True)
    ap.add_argument("--frames", type=int, default=30)
    ap.add_argument("--fps", type=float, default=30.0)
    ap.add_argument("--loop", action="store_true")
    ap.add_argument(
        "--out-root",
        default=str(Path(__file__).resolve().parents[1] / "assets" / "pets" / "cow-cat"),
    )
    args = ap.parse_args()
    frames = extract(Path(args.video), args.frames)
    write_set(args.name, frames, args.fps, args.loop, Path(args.out_root))


if __name__ == "__main__":
    main()
