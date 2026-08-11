"""Pack reminder_wave as a slow, looping yawn from reminder_meow.mp4.

Cadence (default): ease open → long hold on wide open → ease close, ~7s/cycle @ 6fps.
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

from extract_video_frames import ANCHOR, SIZE, pack_sit  # noqa: E402

ROOT = Path(__file__).resolve().parents[1]
VIDEO = ROOT / "assets" / "pets" / "cow-cat" / "_video" / "reminder_meow.mp4"
OUT = ROOT / "assets" / "pets" / "cow-cat" / "reminder_wave"

# Slow yawn: 6 fps feels lazy; ~7s full open-hold-close loop.
FPS = 6.0
RISE_N = 16
HOLD_N = 14  # dwell on wide open
# close = RISE_N - 1 → total ≈ 16+14+15 = 45 → 7.5s


def pink_mouth_score(arr: np.ndarray) -> float:
    """Tongue/oral pink fraction (magenta bg excluded)."""
    a = arr.astype(np.float32)
    if a.ndim == 2:
        return 0.0
    r, g, b = a[:, :, 0], a[:, :, 1], a[:, :, 2]
    mag = (r > 150) & (b > 130) & (g < 190) & (r > g + 25)
    pink = (r > 100) & (g < 140) & (b < 150) & (r > g + 15) & (r > b) & (~mag)
    # Prefer pink in upper half (face), not belly
    h = pink.shape[0]
    face = pink[: int(h * 0.62), :]
    return float(face.mean())


def smooth(scores: list[float], k: int = 5) -> list[float]:
    if not scores:
        return scores
    x = np.asarray(scores, dtype=np.float64)
    ker = np.ones(k) / k
    pad = k // 2
    xp = np.pad(x, (pad, pad), mode="edge")
    y = np.convolve(xp, ker, mode="valid")
    return y.tolist()


def ease_in_out(t: float) -> float:
    t = max(0.0, min(1.0, t))
    return t * t * (3.0 - 2.0 * t)


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
    out[mask, :3] = out_rgb[mask] / out_a[mask]
    out[:, :, 3:4] = out_a * 255.0
    return Image.fromarray(np.clip(out, 0, 255).astype(np.uint8), "RGBA")


def pick_yawn_indices(n: int, scores: list[float]) -> list[int]:
    """One yawn: low-open → peak open → hold → back to low-open."""
    sm = smooth(scores, 7)
    peak = int(np.argmax(sm))

    # Prefer a peak not glued to the first frame (model freeze).
    # Search global top-3 peaks and pick the one with best local contrast.
    order = list(np.argsort(sm)[::-1])
    candidates = []
    for idx in order[:12]:
        lo = max(0, idx - 25)
        local_min = min(sm[lo : idx + 1]) if idx > lo else sm[idx]
        contrast = sm[idx] - local_min
        if idx >= 8:  # skip pure intro freeze
            candidates.append((contrast, sm[idx], int(idx)))
    if candidates:
        candidates.sort(reverse=True)
        peak = candidates[0][2]

    # Start: most closed moment in [peak-55, peak-8]
    lo = max(0, peak - 55)
    hi = max(lo + 1, peak - 8)
    start = lo + int(np.argmin(sm[lo:hi]))

    if peak - start < 10:
        start = max(0, peak - 28)

    rise: list[int] = []
    span = max(1, peak - start)
    for i in range(RISE_N):
        t = i / max(1, RISE_N - 1)
        u = ease_in_out(t)
        # Bias time spent near the open end (yawn stretch)
        u = u ** 0.85
        rise.append(start + int(round(u * span)))

    hold: list[int] = []
    for i in range(HOLD_N):
        # Tiny jaw micro-motion around peak while holding
        phase = i / max(1, HOLD_N)
        wobble = int(round(3.0 * np.sin(phase * np.pi * 2.0)))
        # Stay on the more-open side of peak when possible
        hold.append(int(np.clip(peak + wobble, 0, n - 1)))

    close = list(reversed(rise[:-1]))
    idxs = rise + hold + close
    print(
        f"peak={peak} start={start} rise={len(rise)} hold={len(hold)} "
        f"close={len(close)} total={len(idxs)} cycle≈{len(idxs)/FPS:.1f}s"
    )
    return idxs


def main() -> None:
    if not VIDEO.is_file():
        raise SystemExit(f"missing {VIDEO}")

    arrs = list(iio.imiter(VIDEO, plugin="FFMPEG"))
    n = len(arrs)
    if n < 24:
        raise SystemExit(f"too few frames: {n}")

    scores = [pink_mouth_score(a) for a in arrs]
    print(
        f"video={n} score min={min(scores):.5f} max={max(scores):.5f} "
        f"argmax={int(np.argmax(scores))}"
    )

    idxs = pick_yawn_indices(n, scores)

    frames: list[Image.Image] = []
    for i in idxs:
        im = Image.fromarray(arrs[i]).convert("RGBA")
        frames.append(pack_sit(im))

    # Soft loop: last frames ease toward first (closed-ish pose)
    book = 5
    first = frames[0].copy()
    for i in range(book):
        idx = len(frames) - 1 - i
        t = (i + 1) / (book + 1)
        frames[idx] = blend_rgba(frames[idx], first, t)

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
        "source": "video_reminder_meow_slow_yawn",
        "notes": (
            f"Slow yawn loop for forced rest: ease open → hold → ease close, "
            f"{len(files)/FPS:.1f}s/cycle @ {FPS}fps"
        ),
    }
    (OUT / "meta.json").write_text(
        json.dumps(meta, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    print(f"wrote reminder_wave: {len(files)}f @{FPS}fps loop=True")


if __name__ == "__main__":
    main()
