"""Coherent pet animation: ONE master identity, ~30fps sequences.

All frames are warps / eyelid paint of the same base_sit pixels.
No cross-identity AI hard cuts. Actions ease out-and-back to sit.

Run:  python tools/build_coherent_30fps.py
"""

from __future__ import annotations

import json
import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "assets" / "pets" / "cow-cat"
MASTER = OUT / "_master" / "base_sit.png"
SIZE = 256
ANCHOR = {"x": 128, "y": 220}
# Idle ~4s breathe+blink; one-shots ~1.2s natural (runtime may stretch ≥3s)
ACTION_FRAMES = 36
IDLE_FRAMES = 120
IDLE_FPS = 30.0
ACTION_FPS = 30.0


def load_base() -> Image.Image:
    if MASTER.is_file():
        im = Image.open(MASTER).convert("RGBA")
        if im.size != (SIZE, SIZE):
            im = im.resize((SIZE, SIZE), Image.Resampling.LANCZOS)
        return im
    raise SystemExit(f"missing {MASTER} — place base_sit.png under _master/")


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
        "source": "coherent_warp",
    }
    (dest / "meta.json").write_text(
        json.dumps(meta, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    print(f"  {name}: {len(files)}f @{fps}fps loop={loop}")


def ease_in_out(t: float) -> float:
    t = max(0.0, min(1.0, t))
    return t * t * (3.0 - 2.0 * t)


def ease_out_back(t: float) -> float:
    """0→1→0 with smooth peak at mid (go out and return)."""
    if t < 0.5:
        return ease_in_out(t * 2.0)
    return ease_in_out(2.0 - t * 2.0)


def ease_out_back_hold(t: float, hold: float = 0.18) -> float:
    """Out → hold near peak → back. hold is fraction of timeline at peak."""
    hold = max(0.0, min(0.4, hold))
    rise = (1.0 - hold) * 0.5
    if t < rise:
        return ease_in_out(t / max(1e-6, rise))
    if t < rise + hold:
        return 1.0
    u = (t - rise - hold) / max(1e-6, rise)
    return ease_in_out(1.0 - u)


def warp(
    base: Image.Image,
    *,
    scale_x: float = 1.0,
    scale_y: float = 1.0,
    rot_deg: float = 0.0,
    dx: int = 0,
    dy: int = 0,
    pivot_y_ratio: float = 0.78,
) -> Image.Image:
    """Identity-preserving geometric warp around a foot pivot."""
    w, h = base.size
    piv = (w // 2, int(h * pivot_y_ratio))
    im = base
    if abs(scale_x - 1.0) > 1e-3 or abs(scale_y - 1.0) > 1e-3:
        nw = max(1, int(w * scale_x))
        nh = max(1, int(h * scale_y))
        scaled = im.resize((nw, nh), Image.Resampling.LANCZOS)
        canvas = Image.new("RGBA", (w, h), (0, 0, 0, 0))
        ox = piv[0] - int(piv[0] * scale_x)
        oy = piv[1] - int(piv[1] * scale_y)
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


def find_eyes(base: Image.Image) -> list[tuple[float, float, float]]:
    w, h = base.size
    px = base.load()
    pts = []
    for y in range(int(h * 0.10), int(h * 0.50)):
        for x in range(int(w * 0.15), int(w * 0.85)):
            r, g, b, a = px[x, y]
            if a < 180:
                continue
            if g > 140 and r > 90 and b < 130 and g >= r - 30:
                pts.append((x, y))
    if len(pts) < 10:
        return [(w * 0.40, h * 0.34, 10.0), (w * 0.60, h * 0.34, 10.0)]
    xs = sorted(pts, key=lambda p: p[0])
    mid = xs[len(xs) // 2][0]
    groups = [[p for p in pts if p[0] < mid], [p for p in pts if p[0] >= mid]]
    out = []
    for g in groups:
        if not g:
            continue
        cx = sum(p[0] for p in g) / len(g)
        cy = sum(p[1] for p in g) / len(g)
        rad = max(6.0, min(14.0, max(math.hypot(p[0] - cx, p[1] - cy) for p in g) * 1.15))
        out.append((cx, cy, rad))
    return out[:2] if len(out) >= 2 else [(w * 0.40, h * 0.34, 10.0), (w * 0.60, h * 0.34, 10.0)]


def paint_lids(base: Image.Image, amount: float) -> Image.Image:
    amount = max(0.0, min(1.0, amount))
    if amount < 0.02:
        return base.copy()
    img = base.copy()
    ov = Image.new("RGBA", img.size, (0, 0, 0, 0))
    draw = ImageDraw.Draw(ov)
    px = base.load()
    for cx, cy, rad in find_eyes(base):
        samples = []
        for dy in range(-int(rad) - 6, -int(rad)):
            for dx in range(-3, 4):
                x, y = int(cx + dx), int(cy + dy)
                if 0 <= x < base.size[0] and 0 <= y < base.size[1]:
                    r, g, b, a = px[x, y]
                    if a > 200 and max(r, g, b) < 90:
                        samples.append((r, g, b))
        if samples:
            fr = sum(s[0] for s in samples) // len(samples)
            fg = sum(s[1] for s in samples) // len(samples)
            fb = sum(s[2] for s in samples) // len(samples)
        else:
            fr, fg, fb = 16, 16, 20
        rx, ry = rad * 1.45, rad * (0.18 + 0.95 * amount)
        draw.ellipse(
            [cx - rx, cy - ry * 0.25, cx + rx, cy + ry],
            fill=(fr, fg, fb, int(250 * amount)),
        )
    ov = ov.filter(ImageFilter.GaussianBlur(0.8))
    return Image.alpha_composite(img, ov)


def sequence_out_and_back(n: int, peak_fn, hold: float = 0.0) -> list[Image.Image]:
    frames = []
    for i in range(n):
        t = i / max(1, n - 1)
        a = ease_out_back_hold(t, hold) if hold > 0 else ease_out_back(t)
        frames.append(peak_fn(a))
    return frames


def main() -> None:
    base = load_base()
    print(f"base {base.size} identity lock ON — regenerating coherent clips")

    # ── Idle loop ~4s @ 30fps: breathe + two soft blinks (seamless loop) ──
    idle_frames: list[Image.Image] = []
    for i in range(IDLE_FRAMES):
        # phase in [0,1); sin(0)==sin(2π) ⇒ seamless seam
        t = i / IDLE_FRAMES
        breath = 1.0 + 0.022 * math.sin(t * math.tau * 2.0)  # 2 breaths / loop
        fr = warp(base, scale_y=breath, scale_x=2.0 - breath)
        blink_amt = 0.0
        for center in (0.38, 0.82):
            d = abs(t - center)
            # ~5% of loop ≈ 6 frames soft blink
            if d < 0.028:
                blink_amt = max(blink_amt, 1.0 - d / 0.028)
        if blink_amt > 0:
            fr = paint_lids(fr, blink_amt)
        idle_frames.append(fr)
    write_set("idle_blink", idle_frames, fps=IDLE_FPS, loop=True)

    # ── Distinct one-shot actions ──
    # Stretch: elongate body + slight lift (taller, not the same as sleep)
    write_set(
        "idle_stretch",
        sequence_out_and_back(
            ACTION_FRAMES,
            lambda a: warp(
                base,
                scale_y=1.0 + 0.14 * a,
                scale_x=1.0 - 0.06 * a,
                dy=-int(10 * a),
                rot_deg=-2.0 * a,
            ),
            hold=0.12,
        ),
        fps=ACTION_FPS,
        loop=False,
    )

    # Cute: head tilt + soft lids + tiny lift
    write_set(
        "idle_cute",
        sequence_out_and_back(
            ACTION_FRAMES,
            lambda a: paint_lids(
                warp(
                    base,
                    rot_deg=-10.0 * a,
                    dy=-int(4 * a),
                    scale_y=1.0 + 0.03 * a,
                    scale_x=1.0 + 0.01 * a,
                ),
                0.22 * a,
            ),
            hold=0.14,
        ),
        fps=ACTION_FPS,
        loop=False,
    )

    # Tail wag: multi-cycle sway (distinct from cute single tilt)
    def tail_frame(a: float) -> Image.Image:
        # a is envelope 0→1→0; inner oscil uses a as amplitude
        wag = math.sin(a * math.pi * 4.0)  # two full wags while rising/falling
        return warp(
            base,
            rot_deg=7.5 * wag * max(0.15, a),
            dx=int(3 * wag * a),
            dy=-int(1 * a),
            scale_x=1.0 + 0.02 * abs(wag) * a,
        )

    write_set(
        "idle_tail_wag",
        sequence_out_and_back(ACTION_FRAMES, tail_frame, hold=0.08),
        fps=ACTION_FPS,
        loop=False,
    )

    # Sleep: crouch + fully closed eyes held longer
    write_set(
        "idle_sleep",
        sequence_out_and_back(
            ACTION_FRAMES,
            lambda a: paint_lids(
                warp(
                    base,
                    scale_y=1.0 - 0.07 * a,
                    scale_x=1.0 + 0.04 * a,
                    dy=int(8 * a),
                    rot_deg=1.5 * a,
                ),
                min(1.0, a * 1.5),
            ),
            hold=0.28,
        ),
        fps=ACTION_FPS * 0.75,  # slower, sleepy
        loop=False,
    )

    # Watching: gentle lean + head sway (loop seamless; frame 0 ≈ sit identity)
    watch = []
    for i in range(60):
        t = i / 60.0  # [0,1) so sin wraps cleanly 0→0
        s = math.sin(t * math.tau)
        watch.append(
            warp(
                base,
                dx=int(4 * s),
                rot_deg=3.2 * s,
                dy=-int(2 * abs(s)),
                scale_y=1.0 + 0.012 * abs(s),
            )
        )
    write_set("idle_watch", watch, fps=IDLE_FPS, loop=True)

    # ── Pounce: crouch → air → land → sit ──
    pounce_frames: list[Image.Image] = []
    for i in range(ACTION_FRAMES):
        t = i / max(1, ACTION_FRAMES - 1)
        if t < 0.15:
            u = ease_in_out(t / 0.15)
            pounce_frames.append(
                warp(base, scale_y=1.0 - 0.14 * u, scale_x=1.0 + 0.08 * u, dy=int(10 * u))
            )
        elif t < 0.40:
            u = ease_in_out((t - 0.15) / 0.25)
            pounce_frames.append(
                warp(
                    base,
                    scale_y=1.0 + 0.10 * u,
                    scale_x=1.0 - 0.06 * u,
                    rot_deg=-14.0 * u,
                    dy=-int(22 * u),
                    dx=int(12 * u),
                )
            )
        elif t < 0.70:
            u = (t - 0.40) / 0.30
            pounce_frames.append(
                warp(
                    base,
                    scale_y=1.08,
                    scale_x=0.93,
                    rot_deg=-16.0 + 5.0 * u,
                    dy=-int(26 - 8 * u),
                    dx=int(16 + 4 * u),
                )
            )
        elif t < 0.88:
            u = ease_in_out((t - 0.70) / 0.18)
            pounce_frames.append(
                warp(
                    base,
                    scale_y=1.0 - 0.10 * u,
                    scale_x=1.0 + 0.06 * u,
                    rot_deg=-10.0 * (1.0 - u),
                    dy=int(-10 + 16 * u),
                    dx=int(12 * (1.0 - u)),
                )
            )
        else:
            u = ease_in_out((t - 0.88) / 0.12)
            pounce_frames.append(
                warp(
                    base,
                    scale_y=0.90 + 0.10 * u,
                    scale_x=1.06 - 0.06 * u,
                    dy=int(6 * (1.0 - u)),
                )
            )
    write_set("approaching", pounce_frames, fps=ACTION_FPS, loop=False)

    # Play: happy bounce loop
    play = []
    for i in range(24):
        t = i / 24.0
        s = math.sin(t * math.tau * 2.0)
        play.append(
            warp(
                base,
                rot_deg=-5.0 * s,
                dy=-int(5 * abs(s)),
                scale_y=1.0 + 0.04 * abs(s),
                scale_x=1.0 - 0.02 * abs(s),
            )
        )
    write_set("playing_interaction", play, fps=ACTION_FPS, loop=True)

    # Dragging: swing like held by scruff (loop; lift is constant so 0→last is continuous)
    drag = []
    for i in range(24):
        t = i / 24.0
        s = math.sin(t * math.tau)
        drag.append(
            warp(
                base,
                rot_deg=11.0 * s,
                dy=-int(5 + 2 * abs(s)),
                scale_y=1.0 + 0.03 * abs(s),
                scale_x=1.0 - 0.02 * abs(s),
                pivot_y_ratio=0.22,  # pivot near head / scruff
            )
        )
    write_set("dragging", drag, fps=ACTION_FPS, loop=True)

    # Edge peek: body lowered, head bob
    peek = []
    for i in range(24):
        t = i / 24.0
        s = math.sin(t * math.tau)
        peek.append(
            warp(
                base,
                dy=int(40 + 6 * s),
                rot_deg=2.0 * s,
                scale_y=0.96,
            )
        )
    write_set("edge_peek", peek, fps=14.0, loop=True)

    # Reminder wave / feed
    wave = []
    for i in range(24):
        t = i / 24.0
        s = math.sin(t * math.tau * 2.0)
        wave.append(warp(base, rot_deg=-12.0 * s, dy=-int(3 * abs(s)), dx=int(2 * s)))
    write_set("reminder_wave", wave, fps=ACTION_FPS, loop=True)

    feed = sequence_out_and_back(
        24,
        lambda a: warp(
            base,
            scale_y=1.0 + 0.06 * a,
            scale_x=1.0 - 0.02 * a,
            dy=-int(5 * a),
            rot_deg=-3.0 * a,
        ),
        hold=0.2,
    )
    write_set("reminder_feed", feed, fps=ACTION_FPS, loop=True)

    print("done — single identity, distinct motions, seamless idle loop")


if __name__ == "__main__":
    main()
