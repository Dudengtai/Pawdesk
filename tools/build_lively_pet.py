"""Lively pet animation rebuild — identity-locked, layered, storyboard-driven.

Design goals (desktop pet @ default scale ~0.6):
  • Readable motion at ~77 logical px — exaggerate past old coherent_warp
  • Multi-pivot: feet / head / hip (not whole-body clock swing)
  • Storyboard envelopes: anticipate → peak → hold → settle (not pure sine)
  • True iris-mask eyelids (no dark ellipse overlay on eyes)
  • Single master: assets/pets/cow-cat/_master/base_sit.png

Run:  python tools/build_lively_pet.py
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
SOURCE = "lively_v1"

# Frame budgets @ 30fps
IDLE_FPS = 30.0
IDLE_FRAMES = 120  # 4.0s loop
ACTION_FPS = 30.0
STRETCH_FRAMES = 54  # ~1.8s
CUTE_FRAMES = 48  # ~1.6s
WAG_FRAMES = 48  # ~1.6s
SLEEP_FRAMES = 66  # ~2.2s
WATCH_FRAMES = 60  # 2.0s loop
INTERACT_FRAMES = 30  # 1.0s loop-ish
POUNCE_FRAMES = 48


# ── Load / write ────────────────────────────────────────────────────────────


def load_base() -> Image.Image:
    if not MASTER.is_file():
        raise SystemExit(f"missing {MASTER}")
    im = Image.open(MASTER).convert("RGBA")
    if im.size != (SIZE, SIZE):
        im = im.resize((SIZE, SIZE), Image.Resampling.LANCZOS)
    return im


def write_set(
    name: str,
    frames: list[Image.Image],
    fps: float,
    loop: bool,
    *,
    notes: str = "",
) -> None:
    dest = OUT / name
    dest.mkdir(parents=True, exist_ok=True)
    for old in dest.glob("*.png"):
        old.unlink()
    files: list[str] = []
    wide = len(frames) > 100
    for i, fr in enumerate(frames):
        fn = f"{i:03d}.png" if wide else f"{i:02d}.png"
        if fr.size != (SIZE, SIZE):
            fr = fr.resize((SIZE, SIZE), Image.Resampling.LANCZOS)
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
        "source": SOURCE,
    }
    if notes:
        meta["notes"] = notes
    (dest / "meta.json").write_text(
        json.dumps(meta, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    print(f"  {name}: {len(files)}f @{fps:g}fps loop={loop}")


# ── Easing ──────────────────────────────────────────────────────────────────


def clamp01(t: float) -> float:
    return max(0.0, min(1.0, t))


def ease_in_out(t: float) -> float:
    t = clamp01(t)
    return t * t * (3.0 - 2.0 * t)


def ease_out_cubic(t: float) -> float:
    t = clamp01(t)
    return 1.0 - (1.0 - t) ** 3


def ease_in_cubic(t: float) -> float:
    t = clamp01(t)
    return t * t * t


def ease_out_back(t: float, c: float = 1.4) -> float:
    """Overshoot ease-out (t 0→1, result may briefly >1 then settle to 1)."""
    t = clamp01(t)
    t1 = t - 1.0
    return 1.0 + (c + 1.0) * t1 * t1 * t1 + c * t1 * t1


def story_envelope(
    t: float,
    *,
    anticip: float = 0.12,
    rise: float = 0.28,
    hold: float = 0.18,
    settle: float = 0.42,
    anticip_depth: float = 0.18,
) -> float:
    """Return amplitude in [-anticip_depth, 1] then back to 0.

    Phases (fractions of timeline, renormalized if sum≠1):
      anticip → dip negative (crouch / wind-up)
      rise    → climb 0→1 (with soft ease)
      hold    → stay near 1
      settle  → return 1→0
    """
    t = clamp01(t)
    total = anticip + rise + hold + settle
    anticip /= total
    rise /= total
    hold /= total
    settle /= total
    if t < anticip:
        u = t / max(1e-6, anticip)
        return -anticip_depth * math.sin(u * math.pi)
    t1 = t - anticip
    if t1 < rise:
        u = t1 / max(1e-6, rise)
        return ease_out_back(u, 1.25)
    t2 = t1 - rise
    if t2 < hold:
        # tiny settle wiggle at peak
        return 1.0 - 0.03 * math.sin((t2 / max(1e-6, hold)) * math.pi)
    t3 = t2 - hold
    u = t3 / max(1e-6, settle)
    return ease_in_out(1.0 - u)


def blink_envelope(u: float) -> float:
    u = clamp01(u)
    if u < 0.32:
        t = u / 0.32
        return t * t
    if u < 0.48:
        return 1.0
    t = (u - 0.48) / 0.52
    return (1.0 - t) ** 2


def lid_at(t: float, events: list[tuple[float, float]]) -> float:
    amt = 0.0
    for center, half in events:
        start, end = center - half, center + half
        if start <= t <= end:
            u = (t - start) / max(1e-6, end - start)
            amt = max(amt, blink_envelope(u))
    return amt


# ── Iris-mask natural blink (from build_idle_base) ──────────────────────────


def iris_mask(arr: np.ndarray) -> np.ndarray:
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
    m[:55, :] = False
    m[110:, :] = False
    m[:, :78] = False
    m[:, 150:] = False
    return m


def dilate(mask: np.ndarray, radius: int = 2) -> np.ndarray:
    if radius <= 0:
        return mask
    img = Image.fromarray((mask.astype(np.uint8) * 255), mode="L")
    size = radius * 2 + 1
    out = img.filter(ImageFilter.MaxFilter(size=size))
    return np.asarray(out) > 127


def detect_eyes(arr: np.ndarray) -> list[dict]:
    iris = iris_mask(arr)
    ys, xs = np.where(iris)
    if len(xs) < 16:
        # soft fallback centroids for non-standard masters
        return [
            {
                "cx": 102.0,
                "cy": 86.0,
                "rx": 10.0,
                "ry": 9.0,
                "mask": np.zeros(iris.shape, dtype=bool),
                "lid": (16, 16, 18),
                "crease": (8, 8, 10),
            },
            {
                "cx": 148.0,
                "cy": 86.0,
                "rx": 10.0,
                "ry": 9.0,
                "mask": np.zeros(iris.shape, dtype=bool),
                "lid": (16, 16, 18),
                "crease": (8, 8, 10),
            },
        ]
    mid = 0.5 * (xs.min() + xs.max())
    eyes: list[dict] = []
    r = arr[:, :, 0]
    g = arr[:, :, 1]
    b = arr[:, :, 2]
    a = arr[:, :, 3]
    for cond in (xs < mid, xs >= mid):
        if not np.any(cond):
            continue
        xx, yy = xs[cond], ys[cond]
        cx = float(xx.mean())
        cy = float(yy.mean())
        rx = float(max(7.0, (xx.max() - xx.min()) * 0.55 + 2.5))
        ry = float(max(6.5, (yy.max() - yy.min()) * 0.55 + 2.5))
        local = np.zeros(iris.shape, dtype=bool)
        local[yy, xx] = True
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
        yy_g, xx_g = np.ogrid[y0:y1, x0:x1]
        in_ell = ((xx_g - cx) / (rx * 1.05)) ** 2 + ((yy_g - cy) / (ry * 1.05)) ** 2 <= 1.0
        local[y0:y1, x0:x1] |= region_dark & in_ell
        local = dilate(local, 2)
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
    return eyes


def _sample_lid_tex(
    arr: np.ndarray, x: int, y: int, cx: float, cy: float, ry: float
) -> tuple[int, int, int]:
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
    if max(rr, gg, bb) < 18:
        rr, gg, bb = 22, 22, 26
    return rr, gg, bb


def natural_blink(base: Image.Image, eyes: list[dict], amount: float) -> Image.Image:
    amount = clamp01(amount)
    if amount < 0.02:
        return base.copy()
    # Empty masks → skip (fallback eyes)
    if all(int(e["mask"].sum()) == 0 for e in eyes):
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
        lid_front = amount * 1.08
        soft = 0.09
        for x, y in zip(xs.tolist(), ys.tolist()):
            ny = (y + 0.5 - cy) / max(ry, 1.0)
            v = (ny + 1.0) * 0.5
            nx = (x + 0.5 - cx) / max(rx, 1.0)
            cover = (lid_front - v) / soft
            if cover <= 0:
                continue
            cover = min(1.0, cover)
            tr, tg, tb = _sample_lid_tex(arr, x, y, cx, cy, ry)
            edge_dark = 1.0 - 0.12 * (1.0 - cover)
            fr = int(tr * edge_dark)
            fg = int(tg * edge_dark)
            fb = int(tb * edge_dark)
            if amount > 0.55 and abs(v - 0.50) < 0.06:
                t = (1.0 - abs(v - 0.50) / 0.06) * min(1.0, (amount - 0.55) / 0.35)
                fr = int(fr * (1 - 0.55 * t) + crease[0] * 0.55 * t)
                fg = int(fg * (1 - 0.55 * t) + crease[1] * 0.55 * t)
                fb = int(fb * (1 - 0.55 * t) + crease[2] * 0.55 * t)
            if amount > 0.85:
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


# ── Layered warp ────────────────────────────────────────────────────────────


def warp(
    base: Image.Image,
    *,
    scale_x: float = 1.0,
    scale_y: float = 1.0,
    rot_deg: float = 0.0,
    dx: int = 0,
    dy: int = 0,
    pivot_y_ratio: float = 0.86,
) -> Image.Image:
    """Whole-sprite warp about a vertical pivot (default feet)."""
    w, h = base.size
    piv = (w // 2, int(h * pivot_y_ratio))
    im = base
    if abs(scale_x - 1.0) > 1e-4 or abs(scale_y - 1.0) > 1e-4:
        nw = max(1, int(round(w * scale_x)))
        nh = max(1, int(round(h * scale_y)))
        scaled = im.resize((nw, nh), Image.Resampling.LANCZOS)
        canvas = Image.new("RGBA", (w, h), (0, 0, 0, 0))
        ox = piv[0] - int(round(piv[0] * scale_x))
        oy = piv[1] - int(round(piv[1] * scale_y))
        canvas.paste(scaled, (ox, oy), scaled)
        im = canvas
    if abs(rot_deg) > 1e-3:
        im = im.rotate(
            rot_deg,
            resample=Image.Resampling.BICUBIC,
            center=piv,
            expand=False,
            fillcolor=(0, 0, 0, 0),
        )
    if dx or dy:
        out = Image.new("RGBA", (w, h), (0, 0, 0, 0))
        out.paste(im, (dx, dy), im)
        im = out
    return im


def warp_head(
    base: Image.Image,
    *,
    rot_deg: float = 0.0,
    dx: int = 0,
    dy: int = 0,
    head_y_ratio: float = 0.38,
    blend_top: float = 0.18,
    blend_bot: float = 0.55,
) -> Image.Image:
    """Tilt / shift primarily the upper body (head region) with soft vertical blend.

    Keeps feet planted; body below blend_bot stays mostly base.
    """
    if abs(rot_deg) < 1e-3 and dx == 0 and dy == 0:
        return base.copy()
    w, h = base.size
    piv = (w // 2, int(h * head_y_ratio))
    moved = base.rotate(
        rot_deg,
        resample=Image.Resampling.BICUBIC,
        center=piv,
        expand=False,
        fillcolor=(0, 0, 0, 0),
    )
    if dx or dy:
        tmp = Image.new("RGBA", (w, h), (0, 0, 0, 0))
        tmp.paste(moved, (dx, dy), moved)
        moved = tmp

    # Soft vertical mix: upper = moved, lower = base
    base_a = np.array(base, dtype=np.float32)
    mov_a = np.array(moved, dtype=np.float32)
    y = np.linspace(0.0, 1.0, h, dtype=np.float32)[:, None]
    # 1 at top → 0 below blend_bot
    t = np.clip((blend_bot - y) / max(1e-4, blend_bot - blend_top), 0.0, 1.0)
    t = t * t * (3.0 - 2.0 * t)  # smoothstep
    t = t[:, :, None]
    out = base_a * (1.0 - t) + mov_a * t
    # Prefer higher alpha where either has content
    out_a = np.maximum(base_a[:, :, 3:4], mov_a[:, :, 3:4])
    # Where moved contributed, keep blended RGB; restore alpha sensibly
    rgb = out[:, :, :3]
    alpha = np.clip(
        base_a[:, :, 3:4] * (1.0 - t) + mov_a[:, :, 3:4] * t, 0, 255
    )
    result = np.concatenate([rgb, alpha], axis=2).astype(np.uint8)
    return Image.fromarray(result, "RGBA")


def warp_hip(
    base: Image.Image,
    *,
    rot_deg: float = 0.0,
    dx: int = 0,
    hip_y_ratio: float = 0.72,
    head_lock: float = 0.42,
) -> Image.Image:
    """Sway lower body about hip; upper/head mostly locked (tail-wag feel)."""
    if abs(rot_deg) < 1e-3 and dx == 0:
        return base.copy()
    w, h = base.size
    piv = (w // 2, int(h * hip_y_ratio))
    swung = base.rotate(
        rot_deg,
        resample=Image.Resampling.BICUBIC,
        center=piv,
        expand=False,
        fillcolor=(0, 0, 0, 0),
    )
    if dx:
        tmp = Image.new("RGBA", (w, h), (0, 0, 0, 0))
        tmp.paste(swung, (dx, 0), swung)
        swung = tmp

    base_a = np.array(base, dtype=np.float32)
    sw_a = np.array(swung, dtype=np.float32)
    y = np.linspace(0.0, 1.0, h, dtype=np.float32)[:, None]
    # 0 above head_lock (keep base head), 1 near bottom
    t = np.clip((y - head_lock) / max(1e-4, 1.0 - head_lock), 0.0, 1.0)
    t = t * t * (3.0 - 2.0 * t)
    t = t[:, :, None]
    rgb = base_a[:, :, :3] * (1.0 - t) + sw_a[:, :, :3] * t
    alpha = base_a[:, :, 3:4] * (1.0 - t) + sw_a[:, :, 3:4] * t
    result = np.concatenate([rgb, alpha], axis=2).astype(np.uint8)
    return Image.fromarray(result, "RGBA")


# ── Clip builders ───────────────────────────────────────────────────────────


def build_idle_blink(base: Image.Image, eyes: list[dict]) -> list[Image.Image]:
    """Breath + natural blinks + micro head sway — lively but not a second act.

    Order: lids on rest pose → tiny breath warp (masks stay valid; breath is small).
    """
    blink_events = [
        (0.30, 0.028),
        (0.38, 0.022),  # double blink
        (0.78, 0.030),
    ]
    frames: list[Image.Image] = []
    for i in range(IDLE_FRAMES):
        t = i / IDLE_FRAMES
        amt = lid_at(t, blink_events)
        src = natural_blink(base, eyes, amt) if amt > 0.02 else base
        # 2 breath cycles / loop, subtle (readable at small size but calm)
        breath = 0.018 * math.sin(t * math.tau * 2.0)
        fr = warp(
            src,
            scale_y=1.0 + breath,
            scale_x=1.0 - breath * 0.5,
            pivot_y_ratio=0.86,
        )
        # Micro head sway (very soft presence)
        head_s = 0.8 * math.sin(t * math.tau + 0.4)
        fr = warp_head(fr, rot_deg=head_s, dy=int(round(-0.6 * abs(math.sin(t * math.tau)))))
        frames.append(fr)
    return frames


def build_stretch(base: Image.Image, eyes: list[dict]) -> list[Image.Image]:
    """Anticipate crouch → tall stretch → hold → settle to sit.

    Lids are painted on the rest pose *before* warp so iris masks stay valid.
    """
    frames: list[Image.Image] = []
    n = STRETCH_FRAMES
    for i in range(n):
        t = i / max(1, n - 1)
        a = story_envelope(
            t,
            anticip=0.14,
            rise=0.30,
            hold=0.16,
            settle=0.40,
            anticip_depth=0.55,
        )
        # soft lids near peak stretch (effort squint) — pre-warp
        src = base
        if a > 0.7:
            src = natural_blink(base, eyes, 0.22 * ((a - 0.7) / 0.3))
        # a: negative = crouch wind-up, positive = stretch up
        if a < 0:
            u = -a  # 0→1 crouch amount
            fr = warp(
                src,
                scale_y=1.0 - 0.12 * u,
                scale_x=1.0 + 0.08 * u,
                dy=int(round(10 * u)),
                rot_deg=2.0 * u,
                pivot_y_ratio=0.86,
            )
        else:
            u = min(1.15, a)  # allow slight overshoot from ease_out_back
            fr = warp(
                src,
                scale_y=1.0 + 0.26 * u,
                scale_x=1.0 - 0.10 * u,
                dy=int(round(-14 * u)),
                rot_deg=-4.0 * u,
                pivot_y_ratio=0.86,
            )
            # slight head lift with stretch
            fr = warp_head(fr, rot_deg=-3.0 * u, dy=int(round(-3 * u)))
        frames.append(fr)
    return frames


def build_cute(base: Image.Image, eyes: list[dict]) -> list[Image.Image]:
    """Head tilt + half lids + tiny lift — distinct from stretch.

    Half-lids applied on rest pose, then head/body warp (masks stay aligned).
    """
    frames: list[Image.Image] = []
    n = CUTE_FRAMES
    for i in range(n):
        t = i / max(1, n - 1)
        a = story_envelope(
            t,
            anticip=0.08,
            rise=0.28,
            hold=0.22,
            settle=0.42,
            anticip_depth=0.15,
        )
        if a < 0:
            u = -a
            fr = warp(base, scale_y=1.0 - 0.02 * u, dy=int(round(2 * u)))
        else:
            u = min(1.1, a)
            lid = 0.48 * u
            src = natural_blink(base, eyes, lid) if lid > 0.02 else base
            # whole body tiny lift + pad-up feel
            fr = warp(
                src,
                scale_y=1.0 + 0.05 * u,
                scale_x=1.0 + 0.02 * u,
                dy=int(round(-6 * u)),
                pivot_y_ratio=0.86,
            )
            fr = warp_head(
                fr,
                rot_deg=-16.0 * u,
                dx=int(round(-2 * u)),
                dy=int(round(-4 * u)),
            )
        frames.append(fr)
    return frames


def build_tail_wag(base: Image.Image, eyes: list[dict]) -> list[Image.Image]:
    """Hip-driven multi-beat sway; head stays relatively still."""
    frames: list[Image.Image] = []
    n = WAG_FRAMES
    for i in range(n):
        t = i / max(1, n - 1)
        env = story_envelope(
            t,
            anticip=0.06,
            rise=0.18,
            hold=0.48,
            settle=0.28,
            anticip_depth=0.08,
        )
        env = max(0.0, env)
        # 2.5 wag cycles while envelope is up
        phase = t * math.pi * 5.0
        wag = math.sin(phase) * env
        fr = warp_hip(
            base,
            rot_deg=14.0 * wag,
            dx=int(round(5 * wag)),
            hip_y_ratio=0.70,
            head_lock=0.40,
        )
        # tiny happy bob
        fr = warp(
            fr,
            dy=int(round(-2 * abs(wag))),
            scale_y=1.0 + 0.015 * abs(wag),
            pivot_y_ratio=0.86,
        )
        frames.append(fr)
    return frames


def build_sleep(base: Image.Image, eyes: list[dict]) -> list[Image.Image]:
    """Sink + full lids + long hold + soft nod.

    Close eyes on rest pose first, then sink/warp so lid pixels travel with face.
    """
    frames: list[Image.Image] = []
    n = SLEEP_FRAMES
    for i in range(n):
        t = i / max(1, n - 1)
        a = story_envelope(
            t,
            anticip=0.06,
            rise=0.22,
            hold=0.38,
            settle=0.34,
            anticip_depth=0.1,
        )
        if a < 0:
            u_body = -a * 0.3
        else:
            u_body = min(1.0, a)
        lid = min(1.0, max(0.0, u_body * 1.4))
        src = natural_blink(base, eyes, lid) if lid > 0.02 else base
        fr = warp(
            src,
            scale_y=1.0 - 0.09 * u_body,
            scale_x=1.0 + 0.05 * u_body,
            dy=int(round(14 * u_body)),
            rot_deg=2.5 * u_body,
            pivot_y_ratio=0.86,
        )
        # nod at peak
        nod = 0.0
        if a > 0.5:
            nod = 3.0 * math.sin((a - 0.5) * math.pi * 2.0) * min(1.0, (a - 0.5) * 2)
        fr = warp_head(fr, rot_deg=nod + 1.5 * u_body, dy=int(round(2 * u_body)))
        frames.append(fr)
    return frames


def build_watch(base: Image.Image, eyes: list[dict]) -> list[Image.Image]:
    """Seamless lean + head interest; frame0 ≈ sit identity."""
    frames: list[Image.Image] = []
    n = WATCH_FRAMES
    for i in range(n):
        t = i / n  # [0,1) seamless
        s = math.sin(t * math.tau)
        c = math.cos(t * math.tau)
        # soft blink mid-cycle once — pre-warp on rest identity
        src = base
        if 0.48 < t < 0.58:
            u = (t - 0.48) / 0.10
            src = natural_blink(base, eyes, blink_envelope(u) * 0.95)
        # body lean mild
        fr = warp(
            src,
            dx=int(round(5 * s)),
            dy=int(round(-2 * abs(s))),
            scale_y=1.0 + 0.018 * abs(s),
            rot_deg=2.0 * s,
            pivot_y_ratio=0.86,
        )
        # head tracks stronger (curious)
        fr = warp_head(
            fr,
            rot_deg=6.5 * s,
            dx=int(round(3 * s)),
            dy=int(round(-2 * abs(s) - 0.5 * c)),
        )
        frames.append(fr)
    return frames


def build_approaching(base: Image.Image) -> list[Image.Image]:
    frames: list[Image.Image] = []
    n = POUNCE_FRAMES
    for i in range(n):
        t = i / max(1, n - 1)
        if t < 0.14:
            u = ease_in_out(t / 0.14)
            fr = warp(
                base,
                scale_y=1.0 - 0.16 * u,
                scale_x=1.0 + 0.10 * u,
                dy=int(round(12 * u)),
            )
        elif t < 0.38:
            u = ease_out_cubic((t - 0.14) / 0.24)
            fr = warp(
                base,
                scale_y=1.0 + 0.14 * u,
                scale_x=1.0 - 0.08 * u,
                rot_deg=-18.0 * u,
                dy=int(round(-28 * u)),
                dx=int(round(16 * u)),
            )
        elif t < 0.68:
            u = (t - 0.38) / 0.30
            fr = warp(
                base,
                scale_y=1.10 - 0.04 * u,
                scale_x=0.90 + 0.03 * u,
                rot_deg=-18.0 + 8.0 * u,
                dy=int(round(-28 + 10 * u)),
                dx=int(round(18 + 4 * u)),
            )
        elif t < 0.88:
            u = ease_in_out((t - 0.68) / 0.20)
            fr = warp(
                base,
                scale_y=1.0 - 0.12 * u,
                scale_x=1.0 + 0.08 * u,
                rot_deg=-10.0 * (1.0 - u),
                dy=int(round(-12 + 18 * u)),
                dx=int(round(14 * (1.0 - u))),
            )
        else:
            u = ease_out_cubic((t - 0.88) / 0.12)
            fr = warp(
                base,
                scale_y=0.88 + 0.12 * u,
                scale_x=1.08 - 0.08 * u,
                dy=int(round(8 * (1.0 - u))),
            )
        frames.append(fr)
    return frames


def build_playing(base: Image.Image) -> list[Image.Image]:
    frames: list[Image.Image] = []
    n = INTERACT_FRAMES
    for i in range(n):
        t = i / n
        s = math.sin(t * math.tau * 2.0)
        fr = warp(
            base,
            rot_deg=-7.0 * s,
            dy=int(round(-8 * abs(s))),
            scale_y=1.0 + 0.06 * abs(s),
            scale_x=1.0 - 0.03 * abs(s),
        )
        fr = warp_head(fr, rot_deg=-4.0 * s, dy=int(round(-2 * abs(s))))
        frames.append(fr)
    return frames


def build_dragging(base: Image.Image) -> list[Image.Image]:
    frames: list[Image.Image] = []
    n = INTERACT_FRAMES
    for i in range(n):
        t = i / n
        s = math.sin(t * math.tau)
        fr = warp(
            base,
            rot_deg=20.0 * s,
            dy=int(round(-8 - 3 * abs(s))),
            scale_y=1.0 + 0.05 * abs(s),
            scale_x=1.0 - 0.04 * abs(s),
            pivot_y_ratio=0.20,  # scruff
        )
        frames.append(fr)
    return frames


def build_edge_peek(base: Image.Image, eyes: list[dict]) -> list[Image.Image]:
    frames: list[Image.Image] = []
    n = INTERACT_FRAMES
    for i in range(n):
        t = i / n
        s = math.sin(t * math.tau)
        src = base
        if 0.40 < t < 0.55:
            u = (t - 0.40) / 0.15
            src = natural_blink(base, eyes, blink_envelope(u) * 0.85)
        # body lowered off edge + head bob curiosity
        fr = warp(
            src,
            dy=int(round(46 + 8 * s)),
            scale_y=0.94,
            rot_deg=1.5 * s,
            pivot_y_ratio=0.86,
        )
        fr = warp_head(fr, rot_deg=5.0 * s, dy=int(round(-4 * abs(s))))
        frames.append(fr)
    return frames


def build_reminder_wave(base: Image.Image) -> list[Image.Image]:
    frames: list[Image.Image] = []
    n = INTERACT_FRAMES
    for i in range(n):
        t = i / n
        s = math.sin(t * math.tau * 2.0)
        fr = warp(
            base,
            rot_deg=-8.0 * s,
            dy=int(round(-4 * abs(s))),
            dx=int(round(3 * s)),
            scale_y=1.0 + 0.02 * abs(s),
        )
        # bigger head "wave" lean
        fr = warp_head(fr, rot_deg=-14.0 * s, dx=int(round(4 * s)), dy=int(round(-3 * abs(s))))
        frames.append(fr)
    return frames


def build_reminder_feed(base: Image.Image, eyes: list[dict]) -> list[Image.Image]:
    frames: list[Image.Image] = []
    n = INTERACT_FRAMES
    for i in range(n):
        t = i / max(1, n - 1)
        a = story_envelope(
            t,
            anticip=0.08,
            rise=0.30,
            hold=0.24,
            settle=0.38,
            anticip_depth=0.2,
        )
        u = max(0.0, min(1.1, a))
        lid = 0.28 * ((u - 0.4) / 0.6) if u > 0.4 else 0.0
        src = natural_blink(base, eyes, lid) if lid > 0.02 else base
        fr = warp(
            src,
            scale_y=1.0 + 0.08 * u,
            scale_x=1.0 - 0.03 * u,
            dy=int(round(-8 * u)),
            rot_deg=-5.0 * u,
        )
        fr = warp_head(fr, rot_deg=-6.0 * u, dy=int(round(-3 * u)))
        frames.append(fr)
    return frames


# ── Main ────────────────────────────────────────────────────────────────────


def main() -> None:
    base = load_base()
    arr = np.array(base)
    eyes = detect_eyes(arr)
    print(f"lively_v1 — base {base.size}, identity lock ON")
    for i, e in enumerate(eyes):
        print(
            f"  eye{i}: cx={e['cx']:.1f} cy={e['cy']:.1f} "
            f"rx={e['rx']:.1f} ry={e['ry']:.1f} mask={int(e['mask'].sum())}px"
        )

    write_set(
        "idle_blink",
        build_idle_blink(base, eyes),
        IDLE_FPS,
        True,
        notes="Breath + iris-mask blinks + micro head sway (lively_v1)",
    )
    # Prefer video-baked stretch (real cat yoga) if present — do not overwrite.
    stretch_meta = OUT / "idle_stretch" / "meta.json"
    skip_stretch = False
    if stretch_meta.is_file():
        try:
            src = json.loads(stretch_meta.read_text(encoding="utf-8")).get("source", "")
            if str(src).startswith("video_stretch"):
                print("  idle_stretch: keep existing video_stretch (skip warp)")
                skip_stretch = True
        except Exception:
            pass
    if not skip_stretch:
        write_set(
            "idle_stretch",
            build_stretch(base, eyes),
            ACTION_FPS,
            False,
            notes="Anticipate crouch → tall stretch → settle (fallback warp)",
        )
    # Prefer video-baked yawn cute if present — do not overwrite.
    cute_meta = OUT / "idle_cute" / "meta.json"
    skip_cute = False
    if cute_meta.is_file():
        try:
            src = json.loads(cute_meta.read_text(encoding="utf-8")).get("source", "")
            if str(src).startswith("video_reminder_yawn") or str(src).startswith(
                "yawn_keys"
            ):
                print("  idle_cute: keep existing yawn cute (skip warp)")
                skip_cute = True
        except Exception:
            pass
    if not skip_cute:
        write_set(
            "idle_cute",
            build_cute(base, eyes),
            ACTION_FPS,
            False,
            notes="Head tilt + half lids + pad-up",
        )
    write_set(
        "idle_tail_wag",
        build_tail_wag(base, eyes),
        ACTION_FPS,
        False,
        notes="Hip-driven multi-beat sway",
    )
    write_set(
        "idle_sleep",
        build_sleep(base, eyes),
        ACTION_FPS * 0.9,  # slightly sleepy cadence
        False,
        notes="Sink + full lids + long hold",
    )
    write_set(
        "idle_watch",
        build_watch(base, eyes),
        IDLE_FPS,
        True,
        notes="Seamless lean + head track + soft blink",
    )
    write_set("approaching", build_approaching(base), ACTION_FPS, False)
    write_set("playing_interaction", build_playing(base), ACTION_FPS, True)
    write_set("dragging", build_dragging(base), ACTION_FPS, True)
    write_set("edge_peek", build_edge_peek(base, eyes), 18.0, True)
    # Prefer video-baked wide-open meow if present — do not overwrite.
    wave_meta = OUT / "reminder_wave" / "meta.json"
    skip_wave = False
    if wave_meta.is_file():
        try:
            src = json.loads(wave_meta.read_text(encoding="utf-8")).get("source", "")
            src_s = str(src)
            if (
                src_s.startswith("video_reminder_meow")
                or src_s.startswith("video_reminder_yawn")
                or src_s.startswith("yawn_keys")
            ):
                print("  reminder_wave: keep existing yawn clip (skip warp)")
                skip_wave = True
        except Exception:
            pass
    if not skip_wave:
        write_set("reminder_wave", build_reminder_wave(base), ACTION_FPS, True)
    write_set("reminder_feed", build_reminder_feed(base, eyes), ACTION_FPS, True)

    print("done — lively_v1 all clips regenerated")


if __name__ == "__main__":
    main()
