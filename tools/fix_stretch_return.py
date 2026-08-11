"""Rebuild idle_stretch return path without multi-head morph ghosts.

AI video return (sit-up morph) produces double/triple faces. We keep the clean
sit→stretch→hold prefix, then reverse that path back to sit so the one-shot
bookends match base_sit with no dual-pose frames.

Also re-applies soft edge AA after rewrite.

Usage:
  python tools/fix_stretch_return.py
"""

from __future__ import annotations

import json
from pathlib import Path

import numpy as np
from PIL import Image

ROOT = Path(__file__).resolve().parents[1]
DIR = ROOT / "assets" / "pets" / "cow-cat" / "idle_stretch"
MASTER = ROOT / "assets" / "pets" / "cow-cat" / "_master" / "base_sit.png"
TARGET = 72
# Last frame that is still a clean single-body horizontal stretch (before sit-up morph).
CLEAN_END = 55


def load(i: int) -> np.ndarray:
    return np.array(Image.open(DIR / f"{i:02d}.png").convert("RGBA"), dtype=np.uint8)


def soft_edge_aa(arr: np.ndarray, radius: float = 1.15) -> np.ndarray:
    """Feather silhouette alpha with a short Gaussian ramp (anti-jaggy)."""
    from PIL import ImageFilter

    solid = arr[:, :, 3] >= 128
    mask = Image.fromarray((solid.astype(np.uint8) * 255), mode="L")
    soft = mask.filter(ImageFilter.GaussianBlur(radius=radius))
    blur = np.array(soft, dtype=np.float32) / 255.0
    alpha = np.power(np.clip(blur, 0.0, 1.0), 0.78)
    new_a = (alpha * 255.0).astype(np.uint8)
    new_a = np.where(solid & (blur >= 0.88), np.uint8(255), new_a)
    new_a = np.where((~solid) & (blur < 0.06), np.uint8(0), new_a)
    out = arr.copy()
    out[:, :, 3] = new_a
    out[new_a == 0, :3] = 0
    return out


def resample_sequence(frames: list[np.ndarray], n: int) -> list[np.ndarray]:
    if len(frames) == n:
        return frames
    if len(frames) == 1:
        return [frames[0].copy() for _ in range(n)]
    out: list[np.ndarray] = []
    last = len(frames) - 1
    for i in range(n):
        t = i * last / (n - 1)
        i0 = int(np.floor(t))
        i1 = min(i0 + 1, last)
        u = t - i0
        if u < 1e-6:
            out.append(frames[i0].copy())
        else:
            # Premultiplied blend between path keyframes (same-ish pose neighbors).
            a = frames[i0].astype(np.float32)
            b = frames[i1].astype(np.float32)
            aa = a[:, :, 3:4] / 255.0
            ba = b[:, :, 3:4] / 255.0
            ar = a[:, :, :3] * aa
            br = b[:, :, :3] * ba
            oa = aa * (1 - u) + ba * u
            rgb = ar * (1 - u) + br * u
            out_f = np.zeros_like(a)
            mask = oa[:, :, 0] > 1e-6
            out_f[mask, :3] = rgb[mask] / oa[mask]
            out_f[:, :, 3:4] = oa * 255.0
            out.append(np.clip(out_f, 0, 255).astype(np.uint8))
    return out


def main() -> None:
    existing = [load(i) for i in range(TARGET)]
    # Guard CLEAN_END
    cut = min(CLEAN_END, len(existing) - 1)
    # Prefer master sit for bookends when available
    if MASTER.is_file():
        base = np.array(Image.open(MASTER).convert("RGBA"), dtype=np.uint8)
    else:
        base = existing[0].copy()

    # Clean path: sit → stretch → hold (0..cut), then reverse back to sit.
    outbound = existing[: cut + 1]
    # Drop the duplicated peak frame at the joint.
    ret = list(reversed(outbound[:-1]))
    path = outbound + ret
    # Force exact sit identity on ends.
    path[0] = base.copy()
    path[-1] = base.copy()

    rebuilt = resample_sequence(path, TARGET)
    rebuilt[0] = base.copy()
    rebuilt[-1] = base.copy()

    # Soft edges on every frame
    for i, fr in enumerate(rebuilt):
        fr = soft_edge_aa(fr, radius=1.4)
        # Tiny despill safety on edge (purple residual after AA)
        r, g, b, a = fr[:, :, 0], fr[:, :, 1], fr[:, :, 2], fr[:, :, 3]
        score = r.astype(np.int16) + b.astype(np.int16) - 2 * g.astype(np.int16)
        spill = (a > 8) & (score > 30) & (g < 110)
        if spill.any():
            lum = ((g.astype(np.int16) + np.minimum(r, b).astype(np.int16)) // 2).astype(np.uint8)
            fr = fr.copy()
            fr[spill, 0] = lum[spill]
            fr[spill, 1] = lum[spill]
            fr[spill, 2] = lum[spill]
        Image.fromarray(fr, "RGBA").save(DIR / f"{i:02d}.png")

    meta_path = DIR / "meta.json"
    meta = json.loads(meta_path.read_text(encoding="utf-8"))
    meta["frames"] = TARGET
    meta["files"] = [f"{i:02d}.png" for i in range(TARGET)]
    meta["notes"] = (
        "Stretch out + reverse-return (no AI sit-up morph). "
        f"Clean prefix 0..{cut}, reversed home; soft edge AA."
    )
    meta["source"] = "fix_stretch_return_v1"
    meta_path.write_text(json.dumps(meta, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    print(f"rebuilt {TARGET} frames from clean 0..{cut} + reverse ({len(path)} path keys)")


if __name__ == "__main__":
    main()
