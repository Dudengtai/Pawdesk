"""Pack idle_cute yawn with seamless enter/exit to idle_blink sit identity.

Pipeline:
  exact sit hold → short sit↔near-closed video ease (mouth both closed) →
  continuous video yawn → reverse ease → exact sit hold

Bookend pixels are bitwise-identical to idle_blink/000 so runtime hard-cuts
into/out of the oneshot have zero pose jump. Mid-clip ease only blends near-
closed poses (avoids double-mouth ghosting of open+closed).
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

from clean_yawn_edge_fringe import clean_frame  # noqa: E402
from extract_video_frames import ANCHOR, SIZE  # noqa: E402
from pack_reminder_yawn_video import (  # noqa: E402
    mouth_open_score,
    pack_sit,
    sample_indices_by_motion,
)

ROOT = Path(__file__).resolve().parents[1]
VIDEO = ROOT / "assets" / "pets" / "cow-cat" / "_video" / "reminder_yawn.mp4"
SIT_PATH = ROOT / "assets" / "pets" / "cow-cat" / "idle_blink" / "000.png"
OUT = ROOT / "assets" / "pets" / "cow-cat" / "idle_cute"

FPS = 16.0
HOLD = 8  # exact sit each end (~0.5s)
EASE = 12  # geometric sit<->yawn morph frames (~0.75s, smooth size ramp)
MOTION = 44  # continuous video yawn frames


def _bbox(im: Image.Image):
    return im.split()[3].getbbox()


def morph_frame_toward_sit(frame: Image.Image, sit_bbox, t: float) -> Image.Image:
    """Geometrically ease a packed yawn frame toward the sit pose.

    No alpha blending (avoids mouth/face ghosting). We shrink the sprite's
    non-transparent region toward the sit bbox and lift the feet, anchoring on
    the foot line and horizontal center so the cat appears to settle down
    smoothly instead of popping between two sizes.

    t=0 -> frame unchanged (yawn pose); t=1 -> matches sit bbox footprint.
    """
    t = ease_in_out(max(0.0, min(1.0, t)))
    fb = _bbox(frame)
    if not fb:
        return frame.copy()
    fw, fh = fb[2] - fb[0], fb[3] - fb[1]
    sw, sh = sit_bbox[2] - sit_bbox[0], sit_bbox[3] - sit_bbox[1]
    if fw <= 0 or fh <= 0:
        return frame.copy()

    # Interpolated target footprint between current yawn bbox and sit bbox.
    tw = fw + (sw - fw) * t
    th = fh + (sh - fh) * t
    foot_y = fb[3] + (sit_bbox[3] - fb[3]) * t
    cx = (fb[0] + fb[2]) / 2.0 + (((sit_bbox[0] + sit_bbox[2]) / 2.0) - ((fb[0] + fb[2]) / 2.0)) * t

    # Crop the sprite content and resize to the interpolated footprint.
    content = frame.crop(fb)
    nw = max(1, int(round(tw)))
    nh = max(1, int(round(th)))
    content = content.resize((nw, nh), Image.Resampling.LANCZOS)

    out = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    ox = int(round(cx - nw / 2.0))
    oy = int(round(foot_y - nh))
    out.paste(content, (ox, oy), content)
    return out


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


def ease_in_out(t: float) -> float:
    t = max(0.0, min(1.0, t))
    return t * t * (3.0 - 2.0 * t)


def main() -> None:
    if not VIDEO.is_file():
        raise SystemExit(f"missing {VIDEO}")
    sit = Image.open(SIT_PATH).convert("RGBA")
    if sit.size != (SIZE, SIZE):
        sit = sit.resize((SIZE, SIZE), Image.Resampling.LANCZOS)

    arrs = list(iio.imiter(VIDEO, plugin="FFMPEG"))
    n = len(arrs)
    packed = [pack_sit(Image.fromarray(a).convert("RGBA")) for a in arrs]
    scores = [mouth_open_score(p) for p in packed]
    sm = scores[:]
    for i in range(2, n - 2):
        sm[i] = sum(scores[i - 2 : i + 3]) / 5.0
    peak = int(np.argmax(sm))
    start = int(np.argmin(sm[: max(5, n // 4)]))
    post = min(n - 1, max(peak + 5, int(n * 0.5)))
    end = post + int(np.argmin(sm[post:]))
    if end <= start + 20:
        start, end = 0, n - 1
    print(f"video={n} peak={peak} span={start}..{end}")

    idxs = sample_indices_by_motion(sm, start, end, MOTION)
    motion = [clean_frame(packed[i]) for i in idxs]
    first_v = motion[0]
    last_v = motion[-1]
    print(
        f"motion {len(motion)} first_score={sm[idxs[0]]:.3f} last_score={sm[idxs[-1]]:.3f}"
    )

    sit_bbox = _bbox(sit)

    frames: list[Image.Image] = []
    # 1) exact sit hold
    frames.extend(sit.copy() for _ in range(HOLD))
    # 2) geometric ease sit → first video (grow from sit footprint to yawn pose)
    for i in range(EASE):
        # t goes 1 → 0: start near sit geometry, ease out to full yawn frame.
        t = 1.0 - (i + 1) / (EASE + 1)
        frames.append(morph_frame_toward_sit(first_v, sit_bbox, t))
    # 3) continuous yawn motion
    frames.extend(motion)
    # 4) geometric ease last video → sit (shrink/settle back to sit footprint)
    for i in range(EASE):
        t = (i + 1) / (EASE + 1)
        frames.append(morph_frame_toward_sit(last_v, sit_bbox, t))
    # 5) exact sit hold
    frames.extend(sit.copy() for _ in range(HOLD))

    # Force absolute identity on outermost bookends (critical for hard-cut return)
    for i in range(HOLD):
        frames[i] = sit.copy()
        frames[-(i + 1)] = sit.copy()

    OUT.mkdir(parents=True, exist_ok=True)
    for old in OUT.glob("*.png"):
        old.unlink()
    files: list[str] = []
    for i, fr in enumerate(frames):
        fn = f"{i:02d}.png"
        fr.save(OUT / fn, optimize=True)
        files.append(fn)

    meta = {
        "name": "idle_cute",
        "frame_width": SIZE,
        "frame_height": SIZE,
        "frames": len(files),
        "fps": FPS,
        "loop": False,
        "anchor": {"x": 128, "y": 224},
        "files": files,
        "source": "video_reminder_yawn_fluid",
        "notes": (
            f"Seamless cute yawn: {HOLD}f exact sit (==idle_blink/000), "
            f"{EASE}f geometric morph (foot-anchored shrink, no blend/ghost), "
            f"{MOTION}f continuous video, symmetric reverse morph; "
            f"{len(files)/FPS:.2f}s @ {FPS}fps"
        ),
    }
    (OUT / "meta.json").write_text(
        json.dumps(meta, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )

    b = np.asarray(sit)
    a0 = np.asarray(frames[0])
    aL = np.asarray(frames[-1])
    print(f"wrote {len(files)}f cycle={len(files)/FPS:.2f}s")
    print(f"bookend identity err: start={np.abs(a0.astype(int)-b.astype(int)).mean():.4f} "
          f"end={np.abs(aL.astype(int)-b.astype(int)).mean():.4f}")


if __name__ == "__main__":
    main()
