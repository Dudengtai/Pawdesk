"""Premium base idle only: natural eyelid blink + near-still presence.

Design rules (desktop pet, high bar):
  1. Rest pose is the hero — almost still. No floaty body morph as a second act.
  2. Blink = upper lid *replaces* iris/pupil pixels (opaque fur), never a dark
     ellipse alpha-composited over open eyes (reads as a stain / extra layer).
  3. Only rewrite a per-eye mask built from real iris pixels (+ small dilate),
     so face fur is never painted over.
  4. Timing: long open hold, short close (~200ms), soft double-blink.
  5. Seamless loop: phase 0 ≈ rest identity.

Run:  python tools/build_idle_base.py
"""

from __future__ import annotations

import json
import math
from pathlib import Path

import numpy as np
from PIL import Image, ImageFilter

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "assets" / "pets" / "cow-cat"
MASTER = OUT / "_master" / "base_sit.png"
SIZE = 256
ANCHOR = {"x": 128, "y": 220}
FPS = 30.0
LOOP_SECS = 4.0
N_FRAMES = int(round(LOOP_SECS * FPS))  # 120


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
        fn = f"{i:03d}.png" if len(frames) > 100 else f"{i:02d}.png"
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
        "source": "idle_base_v2",
        "notes": "Natural eyelid replacement blink on iris mask only; near-still rest.",
    }
    (dest / "meta.json").write_text(
        json.dumps(meta, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    print(f"  {name}: {len(files)}f @{fps}fps loop={loop}")


def iris_mask(arr: np.ndarray) -> np.ndarray:
    """Boolean mask of yellow-green iris (not white fur, not black face)."""
    r = arr[:, :, 0].astype(np.int16)
    g = arr[:, :, 1].astype(np.int16)
    b = arr[:, :, 2].astype(np.int16)
    a = arr[:, :, 3]
    m = (
        (a > 200)
        & (g > 130)
        & (r > 85)
        & (b < 130)
        & (g >= r - 30)
        & (g > b + 20)
        & ((r + g + b) < 520)
    )
    # Upper face only
    m[:55, :] = False
    m[110:, :] = False
    m[:, :78] = False
    m[:, 150:] = False
    return m


def dilate(mask: np.ndarray, radius: int = 2) -> np.ndarray:
    """Binary dilate via max-filter (includes pupil hole inside iris ring)."""
    if radius <= 0:
        return mask
    img = Image.fromarray((mask.astype(np.uint8) * 255), mode="L")
    # MaxFilter size must be odd
    size = radius * 2 + 1
    out = img.filter(ImageFilter.MaxFilter(size=size))
    return np.asarray(out) > 127


def detect_eyes(arr: np.ndarray) -> list[dict]:
    """Two eyes from iris clusters — keep each eye's own height (master is slightly asymmetric)."""
    iris = iris_mask(arr)
    ys, xs = np.where(iris)
    if len(xs) < 16:
        raise SystemExit("iris detection failed — check master base_sit.png")

    # Split left / right by x gap
    mid = 0.5 * (xs.min() + xs.max())
    eyes = []
    for cond in (xs < mid, xs >= mid):
        if not np.any(cond):
            continue
        xx, yy = xs[cond], ys[cond]
        cx = float(xx.mean())
        cy = float(yy.mean())
        # Tight bbox → ellipse radii
        rx = float(max(7.0, (xx.max() - xx.min()) * 0.55 + 2.5))
        ry = float(max(6.5, (yy.max() - yy.min()) * 0.55 + 2.5))

        # Per-eye iris mask + dilate to swallow pupil & rim
        local = np.zeros(iris.shape, dtype=bool)
        local[yy, xx] = True
        # Also grab dark pupil *inside* iris bbox only
        r = arr[:, :, 0]
        g = arr[:, :, 1]
        b = arr[:, :, 2]
        a = arr[:, :, 3]
        x0, x1 = int(xx.min()) - 2, int(xx.max()) + 3
        y0, y1 = int(yy.min()) - 2, int(yy.max()) + 3
        x0, y0 = max(0, x0), max(0, y0)
        x1, y1 = min(SIZE, x1), min(SIZE, y1)
        region_dark = (
            (a[y0:y1, x0:x1] > 200)
            & (r[y0:y1, x0:x1] < 50)
            & (g[y0:y1, x0:x1] < 50)
            & (b[y0:y1, x0:x1] < 50)
        )
        # Only dark pixels that are inside the iris ellipse (not face fur outside)
        yy_g, xx_g = np.ogrid[y0:y1, x0:x1]
        in_ell = ((xx_g - cx) / (rx * 1.05)) ** 2 + ((yy_g - cy) / (ry * 1.05)) ** 2 <= 1.0
        local[y0:y1, x0:x1] |= region_dark & in_ell
        local = dilate(local, 2)

        # Lid color: dark fur *above* this eye
        samples = []
        for dy in range(-int(ry) - 12, -int(ry) - 2):
            for dx in range(-int(rx * 0.5), int(rx * 0.5) + 1):
                x, y = int(cx + dx), int(cy + dy)
                if 0 <= x < SIZE and 0 <= y < SIZE and a[y, x] > 220:
                    rr, gg, bb = int(r[y, x]), int(g[y, x]), int(b[y, x])
                    if max(rr, gg, bb) < 75:
                        samples.append((rr, gg, bb))
        if samples:
            lid = tuple(int(sum(c[i] for c in samples) / len(samples)) for i in range(3))
        else:
            lid = (12, 12, 14)
        crease = (max(0, lid[0] // 2), max(0, lid[1] // 2), max(0, lid[2] // 2))

        eyes.append(
            {
                "cx": cx,
                "cy": cy,
                "rx": rx,
                "ry": ry,
                "mask": local,
                "lid": lid,
                "crease": crease,
            }
        )

    eyes.sort(key=lambda e: e["cx"])
    if len(eyes) < 2:
        raise SystemExit(f"expected 2 eyes, got {len(eyes)}")
    return eyes


def _sample_lid_tex(
    arr: np.ndarray, x: int, y: int, cx: float, cy: float, ry: float
) -> tuple[int, int, int]:
    """Pull fur texture from above the eye (real upper lid), not a flat fill."""
    src_y = int(cy - ry - 4 - max(0.0, (y - (cy - ry)) * 0.25))
    src_x = int(cx + (x - cx) * 0.9)
    h, w = arr.shape[:2]
    src_x = max(0, min(w - 1, src_x))
    src_y = max(0, min(h - 1, src_y))
    for _ in range(8):
        if arr[src_y, src_x, 3] > 200:
            break
        src_y = max(0, src_y - 1)
    rr, gg, bb, aa = (int(arr[src_y, src_x, i]) for i in range(4))
    if aa < 180:
        return 16, 16, 18
    # Lift pure black fur slightly so closed lids don't read as empty sockets
    if max(rr, gg, bb) < 18:
        rr, gg, bb = 22, 22, 26
    return rr, gg, bb


def natural_blink(base: Image.Image, eyes: list[dict], amount: float) -> Image.Image:
    """amount 0=open → 1=closed. Opaque lid replacement on iris mask only.

    Model: upper lid front descends with ``amount`` (0 top → 1 bottom of eye).
    Pixels above the front are fully replaced by upper-lid fur texture.
    At amount=1 the whole iris is covered; a thin crease is drawn at mid-lid.
    """
    amount = max(0.0, min(1.0, amount))
    if amount < 0.02:
        return base.copy()

    arr = np.array(base, dtype=np.uint8)
    out = arr.copy()

    for eye in eyes:
        cx, cy = eye["cx"], eye["cy"]
        rx, ry = eye["rx"], eye["ry"]
        crease = eye["crease"]
        mask = eye["mask"]

        ys, xs = np.where(mask)
        if len(xs) == 0:
            continue

        # Lid front position in v∈[0,1] (0=top of eye, 1=bottom).
        # amount=0 → front above eye; amount=1 → front past bottom (full cover).
        lid_front = amount * 1.08  # slight overshoot so full close has no yellow rim
        soft = 0.09  # soft edge width in v-space

        for x, y in zip(xs.tolist(), ys.tolist()):
            ny = (y + 0.5 - cy) / max(ry, 1.0)
            v = (ny + 1.0) * 0.5  # 0 top → 1 bottom
            nx = (x + 0.5 - cx) / max(rx, 1.0)

            # How far under the lid (1 = deep, 0 = at free edge, <0 = still open)
            cover = (lid_front - v) / soft
            if cover <= 0:
                continue
            cover = min(1.0, cover)

            tr, tg, tb = _sample_lid_tex(arr, x, y, cx, cy, ry)

            # Contact shadow near the free edge of the moving lid
            edge_dark = 1.0 - 0.12 * (1.0 - cover)
            fr = int(tr * edge_dark)
            fg = int(tg * edge_dark)
            fb = int(tb * edge_dark)

            # When mostly closed, draw a soft lash crease across mid-eye
            if amount > 0.55 and abs(v - 0.50) < 0.06:
                t = (1.0 - abs(v - 0.50) / 0.06) * min(1.0, (amount - 0.55) / 0.35)
                # crease a bit darker than lid fur; keep some texture
                fr = int(fr * (1 - 0.55 * t) + crease[0] * 0.55 * t)
                fg = int(fg * (1 - 0.55 * t) + crease[1] * 0.55 * t)
                fb = int(fb * (1 - 0.55 * t) + crease[2] * 0.55 * t)

            # Horizontal squash feel near full close: bias toward crease y
            if amount > 0.85:
                # darken outer corners slightly (almond closed shape)
                corner = min(1.0, abs(nx) * 1.1)
                fr = int(fr * (1.0 - 0.15 * corner * amount))
                fg = int(fg * (1.0 - 0.15 * corner * amount))
                fb = int(fb * (1.0 - 0.15 * corner * amount))

            if cover >= 0.98:
                out[y, x, 0] = max(0, min(255, fr))
                out[y, x, 1] = max(0, min(255, fg))
                out[y, x, 2] = max(0, min(255, fb))
                out[y, x, 3] = 255
            else:
                u = cover
                out[y, x, 0] = int(arr[y, x, 0] * (1 - u) + fr * u)
                out[y, x, 1] = int(arr[y, x, 1] * (1 - u) + fg * u)
                out[y, x, 2] = int(arr[y, x, 2] * (1 - u) + fb * u)
                out[y, x, 3] = 255

    return Image.fromarray(out, "RGBA")




def blink_envelope(u: float) -> float:
    """u in [0,1] over one blink → lid amount 0→1→0. Close fast, open medium."""
    u = max(0.0, min(1.0, u))
    if u < 0.32:
        t = u / 0.32
        return t * t  # ease-in close
    if u < 0.48:
        return 1.0  # brief closed
    t = (u - 0.48) / 0.52
    return (1.0 - t) ** 2  # ease-out open


def micro_breath(base: Image.Image, phase: float) -> Image.Image:
    """±0.3% vertical life about the feet — felt as presence, not motion."""
    amp = 0.003 * math.sin(phase * math.tau)
    if abs(amp) < 0.0007:
        return base.copy()
    w, h = base.size
    sy = 1.0 + amp
    sx = 1.0 - amp * 0.45
    nw, nh = max(1, int(w * sx)), max(1, int(h * sy))
    scaled = base.resize((nw, nh), Image.Resampling.LANCZOS)
    out = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    piv_y = int(h * 0.86)
    ox = (w - nw) // 2
    oy = piv_y - int(piv_y * sy)
    out.paste(scaled, (ox, oy), scaled)
    return out


def lid_at(t: float, events: list[tuple[float, float]]) -> float:
    amt = 0.0
    for center, half in events:
        start, end = center - half, center + half
        if start <= t <= end:
            u = (t - start) / (end - start)
            amt = max(amt, blink_envelope(u))
    return amt


def main() -> None:
    base = load_base()
    arr = np.array(base)
    eyes = detect_eyes(arr)
    print("base idle redesign — iris-mask lids only (no overlay layer)")
    for i, e in enumerate(eyes):
        n = int(e["mask"].sum())
        print(
            f"  eye{i}: cx={e['cx']:.1f} cy={e['cy']:.1f} "
            f"rx={e['rx']:.1f} ry={e['ry']:.1f} mask={n}px lid={e['lid']}"
        )

    # Designer timing (fractions of 4s loop)
    # Primary ~1.5s, double-blink follow-up, late single ~3.4s
    blink_events = [
        (0.38, 0.030),  # ~7–8 frames window
        (0.47, 0.024),  # quick second
        (0.84, 0.032),
    ]

    frames: list[Image.Image] = []
    for i in range(N_FRAMES):
        t = i / N_FRAMES
        amt = lid_at(t, blink_events)
        # Pure base: rest identity + eyelid only. No body morph (that reads as
        # a second animation layer fighting the blink).
        if amt < 0.02:
            frames.append(base.copy())
        else:
            frames.append(natural_blink(base, eyes, amt))

    write_set("idle_blink", frames, fps=FPS, loop=True)

    # QA sheet + peak zoom
    debug_idx = []
    for center, half in blink_events:
        c = int(center * N_FRAMES)
        for d in range(-4, 5):
            debug_idx.append(c + d)
    debug_idx = sorted(set(i for i in debug_idx if 0 <= i < N_FRAMES))
    # always include rest
    debug_idx = [0] + debug_idx[:20]

    sheet_w = 72
    sheet = Image.new("RGBA", (sheet_w * len(debug_idx), sheet_w), (0, 0, 0, 0))
    for i, fi in enumerate(debug_idx):
        thumb = frames[fi].resize((sheet_w, sheet_w), Image.Resampling.LANCZOS)
        sheet.paste(thumb, (i * sheet_w, 0), thumb)
    dbg = OUT / "_master" / "idle_blink_sheet.png"
    sheet.save(dbg)

    peak = max(range(N_FRAMES), key=lambda i: lid_at(i / N_FRAMES, blink_events))
    # Crop both eyes with margin
    y0 = max(0, int(min(e["cy"] - e["ry"] for e in eyes) - 18))
    y1 = min(SIZE, int(max(e["cy"] + e["ry"] for e in eyes) + 22))
    x0 = max(0, int(min(e["cx"] - e["rx"] for e in eyes) - 16))
    x1 = min(SIZE, int(max(e["cx"] + e["rx"] for e in eyes) + 16))
    zoom = frames[peak].crop((x0, y0, x1, y1)).resize(
        ((x1 - x0) * 3, (y1 - y0) * 3), Image.Resampling.NEAREST
    )
    zpath = OUT / "_master" / "idle_blink_peak_zoom.png"
    zoom.save(zpath)

    # Half-blink zoom for mid-read
    half_i = None
    best = 1.0
    for i in range(N_FRAMES):
        a = lid_at(i / N_FRAMES, blink_events)
        if abs(a - 0.5) < best:
            best = abs(a - 0.5)
            half_i = i
    if half_i is not None:
        hz = frames[half_i].crop((x0, y0, x1, y1)).resize(
            ((x1 - x0) * 3, (y1 - y0) * 3), Image.Resampling.NEAREST
        )
        hz.save(OUT / "_master" / "idle_blink_half_zoom.png")
        print(f"  half blink frame {half_i}")

    print(f"  peak blink frame {peak} → {zpath}")
    print(f"  sheet → {dbg}")
    print("done — base idle only")


if __name__ == "__main__":
    main()
