"""Smooth one-shot idle returns so they settle into idle_blink/000.

The authored tails still contained visible jumps after the geometric morph:
- idle_cute 75 -> 76 (residual pose snaps to exact sit)
- idle_stretch 70 -> 71 (large 135x244 pose hard-cuts to 121x217 sit)

This script only rewrites the return tail frames and bookends:
- idle_cute: keep 00-71, replace 72-75 with a foot-anchored settle, force 75-83 == base.
- idle_stretch: force 00 == base, keep 01-65, replace 66-71 with a settle ending on base.

Usage:
  python tools/smooth_oneshot_returns.py
"""

from __future__ import annotations

import json
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
PET = ROOT / "assets" / "pets" / "cow-cat"
BASE = PET / "idle_blink" / "000.png"
SIZE = 256


def _bbox(im: Image.Image):
    a = np.asarray(im.convert("RGBA"))
    ys, xs = np.where(a[:, :, 3] > 8)
    if len(xs) == 0:
        return None
    return (int(xs.min()), int(ys.min()), int(xs.max()), int(ys.max()))


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
    out[mask, :3] = out_rgb[mask] / np.maximum(out_a[mask], 1e-6)
    out[:, :, 3:4] = out_a * 255.0
    return Image.fromarray(np.clip(out, 0, 255).astype(np.uint8), "RGBA")


def settle_frame(start: Image.Image, target: Image.Image, raw_t: float) -> Image.Image:
    """Foot-anchored geometric morph plus premultiplied settle into target.

    raw_t=0 leaves start unchanged; raw_t=1 returns target exactly. The
    geometric step handles the size ramp, and the premultiplied step absorbs
    residual alpha/shape differences without exposing a second face.
    """
    if raw_t <= 0.0:
        return start.copy()
    if raw_t >= 1.0:
        return target.copy()

    t = ease_in_out(raw_t)
    sb = _bbox(start)
    tb = _bbox(target)
    if sb is None or tb is None:
        return start.copy()

    content = start.crop(sb)
    fw, fh = sb[2] - sb[0], sb[3] - sb[1]
    sw, sh = tb[2] - tb[0], tb[3] - tb[1]
    tw = fw + (sw - fw) * t
    th = fh + (sh - fh) * t
    foot_y = sb[3] + (tb[3] - sb[3]) * t
    cx = (sb[0] + sb[2]) / 2.0 + (((tb[0] + tb[2]) / 2.0) - ((sb[0] + sb[2]) / 2.0)) * t

    nw = max(1, int(round(tw)))
    nh = max(1, int(round(th)))
    content = content.resize((nw, nh), Image.Resampling.LANCZOS)

    geom = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    ox = int(round(cx - nw / 2.0))
    oy = int(round(foot_y - nh))
    geom.paste(content, (ox, oy), content)
    return blend_rgba(geom, target, t)


def main() -> None:
    base = Image.open(BASE).convert("RGBA")
    if base.size != (SIZE, SIZE):
        base = base.resize((SIZE, SIZE), Image.Resampling.LANCZOS)

    # idle_cute: 84 frames; original morph ran 64-75 then exact sit 76-83.
    cute_dir = PET / "idle_cute"
    cute_files = sorted(p.name for p in cute_dir.glob("*.png"))
    assert len(cute_files) == 84, f"unexpected idle_cute frame count: {len(cute_files)}"
    existing = [Image.open(cute_dir / f).convert("RGBA") for f in cute_files]
    cute = existing[:72]  # 00-71 untouched
    start = existing[71]
    for k in range(1, 5):
        cute.append(settle_frame(start, base, k / 4.0))
    # 75-83: exact sit bookend (75 is the new settled tail endpoint).
    cute.extend(base.copy() for _ in range(8))
    for i, fr in enumerate(cute):
        fr.save(cute_dir / f"{i:02d}.png", optimize=True)
    cute_meta = json.loads((cute_dir / "meta.json").read_text(encoding="utf-8"))
    cute_meta["notes"] = (
        "Seamless cute yawn: 8f exact sit (==idle_blink/000), 12f geometric morph, "
        "44f video, reverse morph 64-71, 4f smooth settle 72-75 ending on exact sit, "
        "exact sit 76-83; 5.25s @ 16.0fps. Tail rewritten by smooth_oneshot_returns_v1"
    )
    (cute_dir / "meta.json").write_text(
        json.dumps(cute_meta, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )

    # idle_stretch: 72 frames; keep clean prefix + reverse through 65, settle 66-71.
    stretch_dir = PET / "idle_stretch"
    stretch_files = sorted(p.name for p in stretch_dir.glob("*.png"))
    assert len(stretch_files) == 72, f"unexpected idle_stretch frame count: {len(stretch_files)}"
    existing = [Image.open(stretch_dir / f).convert("RGBA") for f in stretch_files]
    stretch = [base.copy()] + existing[1:66]  # 00 forced base, 01-65 untouched
    start = existing[65]
    for k in range(1, 7):
        stretch.append(settle_frame(start, base, k / 6.0))
    # Last settle frame is exact base; guard it explicitly.
    stretch[-1] = base.copy()
    for i, fr in enumerate(stretch):
        fr.save(stretch_dir / f"{i:02d}.png", optimize=True)
    stretch_meta = json.loads((stretch_dir / "meta.json").read_text(encoding="utf-8"))
    stretch_meta["notes"] = (
        "Stretch out + reverse-return (no AI sit-up morph). Clean prefix 0..55, "
        "reversed home, 6f smooth settle 66-71 ending on idle_blink/000; "
        "soft edge AA. Tail rewritten by smooth_oneshot_returns_v1"
    )
    (stretch_dir / "meta.json").write_text(
        json.dumps(stretch_meta, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )

    print("rewrote idle_cute tail (72-75 settle, 75-83 exact sit)")
    print("rewrote idle_stretch tail (00 base, 66-71 settle, 71 exact sit)")


if __name__ == "__main__":
    main()
