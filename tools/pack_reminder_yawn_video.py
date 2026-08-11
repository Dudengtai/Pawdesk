"""Pack fluid reminder_wave from continuous yawn VIDEO.

No keyframe morphs, no mid-frame blending (those caused 残影/ghosting).
Only nearest continuous video frames + pure closed-sit holds at ends.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import imageio.v3 as iio
import numpy as np
from PIL import Image

_TOOLS = Path(__file__).resolve().parent
if str(_TOOLS) not in sys.path:
    sys.path.insert(0, str(_TOOLS))

from extract_video_frames import ANCHOR, SIZE  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
VIDEO = ROOT / "assets" / "pets" / "cow-cat" / "_video" / "reminder_yawn.mp4"
MASTER_SIT = ROOT / "assets" / "pets" / "cow-cat" / "_master" / "base_sit.png"
OUT = ROOT / "assets" / "pets" / "cow-cat" / "reminder_wave"

FPS = 16.0
# Motion frames from video (plus closed holds)
MOTION = 52
CLOSED_HOLD = 3  # pure sit at both ends (no blend)
FOOT_Y = 224


def is_pure_magenta_bg(r: int, g: int, b: int) -> bool:
    if r > 210 and b > 200 and g < 70 and r > g + 100 and b > g + 90:
        return True
    if r > 230 and b > 220 and g < 100 and abs(r - b) < 50:
        return True
    if r > 240 and g < 30 and b > 240:
        return True
    if r > 200 and b > 180 and g < 55 and r > g + 90 and b > g + 80:
        return True
    return False


def key_frame(im: Image.Image) -> Image.Image:
    im = im.convert("RGBA")
    px = im.load()
    w, h = im.size
    for y in range(h):
        for x in range(w):
            r, g, b, a = px[x, y]
            if is_pure_magenta_bg(r, g, b):
                px[x, y] = (0, 0, 0, 0)
    for _ in range(2):
        kill: list[tuple[int, int]] = []
        for y in range(h):
            for x in range(w):
                r, g, b, a = px[x, y]
                if a == 0 or g > 95:
                    continue
                if not (r > 185 and b > 165 and g < 85 and r > g + 70 and b > g + 55):
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


def mouth_open_score(im: Image.Image) -> float:
    a = np.asarray(im.convert("RGBA"), dtype=np.float32)
    h, w = a.shape[:2]
    roi = a[int(h * 0.14) : int(h * 0.50), int(w * 0.30) : int(w * 0.70)]
    if roi.size == 0:
        return 0.0
    r, g, b, al = roi[:, :, 0], roi[:, :, 1], roi[:, :, 2], roi[:, :, 3]
    m = al > 40
    pink = m & (r > 140) & (g < 170) & (b < 190) & (r > g + 20) & (r > b)
    dark = m & (((r + g + b) / 3) < 70) & (((r + g + b) / 3) > 10)
    return float(pink.mean() * 2.5 + dark.mean())


def sample_indices_by_motion(scores: list[float], start: int, end: int, count: int) -> list[int]:
    """Spend more output frames where mouth openness changes fastest.

    Continuous, monotonic in source time — no reverse jumps, no blending.
    """
    seg = np.asarray(scores[start : end + 1], dtype=np.float64)
    # motion weight = |delta score| + small base so static holds still advance
    d = np.abs(np.diff(seg, prepend=seg[0]))
    # smooth motion
    if len(d) > 5:
        k = np.ones(5) / 5
        d = np.convolve(d, k, mode="same")
    w = d + 0.08 * (d.max() + 1e-6)
    # cumulative arc-length style
    cdf = np.cumsum(w)
    cdf = cdf / cdf[-1]
    out: list[int] = []
    for i in range(count):
        t = i / max(1, count - 1)
        j = int(np.searchsorted(cdf, t, side="left"))
        j = max(0, min(len(seg) - 1, j))
        out.append(start + j)
    # enforce non-decreasing indices (continuity)
    for i in range(1, len(out)):
        if out[i] < out[i - 1]:
            out[i] = out[i - 1]
    return out


def main() -> None:
    if not VIDEO.is_file():
        raise SystemExit(f"missing {VIDEO}")

    sit = Image.open(MASTER_SIT).convert("RGBA")
    if sit.size != (SIZE, SIZE):
        sit = sit.resize((SIZE, SIZE), Image.Resampling.LANCZOS)

    arrs = list(iio.imiter(VIDEO, plugin="FFMPEG"))
    n = len(arrs)
    print(f"video frames: {n}")
    packed = [pack_sit(Image.fromarray(a).convert("RGBA")) for a in arrs]
    scores = [mouth_open_score(p) for p in packed]
    sm = scores[:]
    for i in range(2, n - 2):
        sm[i] = sum(scores[i - 2 : i + 3]) / 5.0
    peak = int(np.argmax(sm))
    print(f"peak={peak} score={sm[peak]:.4f}")

    # Full usable span: first low → last low around peak
    start = 0
    # find earliest frame that is still relatively closed in first 20%
    pre = sm[: max(2, int(n * 0.25))]
    start = int(np.argmin(pre))
    # end: most closed in last 30% after peak
    post_lo = min(n - 1, max(peak + 3, int(n * 0.55)))
    end = post_lo + int(np.argmin(sm[post_lo:]))
    if end <= start + 20:
        start, end = 0, n - 1
    print(f"span [{start}..{end}] peak={peak}")

    idxs = sample_indices_by_motion(sm, start, end, MOTION)
    print(f"motion samples: {idxs[0]} → {idxs[len(idxs)//2]} → {idxs[-1]}")

    # Build: pure closed holds + nearest video frames only (NO blend)
    frames: list[Image.Image] = []
    for _ in range(CLOSED_HOLD):
        frames.append(sit.copy())
    for i in idxs:
        frames.append(packed[i])
    for _ in range(CLOSED_HOLD):
        frames.append(sit.copy())

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
        "source": "video_reminder_yawn_fluid",
        "notes": (
            f"Nearest video frames only (no morph/blend); closed holds={CLOSED_HOLD}; "
            f"{len(files)/FPS:.2f}s/cycle @{FPS}fps"
        ),
    }
    (OUT / "meta.json").write_text(
        json.dumps(meta, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    print(f"wrote reminder_wave: {len(files)}f @{FPS}fps cycle={len(files)/FPS:.2f}s")


if __name__ == "__main__":
    main()
