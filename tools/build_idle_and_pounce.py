"""Build idle_blink + one-shot actions + approaching pounce from base + keyframes.

Keeps current base_sit appearance. Magenta/pink keys are stripped.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

from PIL import Image, ImageEnhance

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "assets" / "pets" / "cow-cat"
MASTER = OUT / "_master" / "base_sit.png"
SIZE = 128
SESS = Path(
    r"C:\Users\lig76\.grok\sessions\D%3A%5CAI%E7%BB%83%E4%B9%A0%E7%9B%AE%E5%BD%95%5CPawDesk\019fbda6-d039-7c31-8611-5f13cfd446e6\images"
)

# Reuse chroma helpers from set_pet_from_image
sys.path.insert(0, str(ROOT / "tools"))
from set_pet_from_image import (  # noqa: E402
    is_magenta_key,
    pack_frame,
    remove_bg_edge_flood,
    rotate_about,
    scale_about_center,
    shift,
)


def pack_source(path: Path) -> Image.Image:
    if not path.is_file():
        raise SystemExit(f"missing {path}")
    return pack_frame(Image.open(path))


def write_set(
    name: str,
    frames: list[Image.Image],
    fps: float,
    loop: bool,
) -> None:
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
        "anchor": {"x": 64, "y": 112},
        "files": files,
    }
    (dest / "meta.json").write_text(
        json.dumps(meta, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    print(f"wrote {name}: {len(files)} frames loop={loop} @ {fps}fps")


def lerp(a: Image.Image, b: Image.Image, t: float) -> Image.Image:
    a = a.convert("RGBA")
    b = b.convert("RGBA")
    if a.size != b.size:
        b = b.resize(a.size, Image.Resampling.LANCZOS)
    return Image.blend(a, b, t)


def main() -> None:
    base = pack_frame(Image.open(MASTER)) if MASTER.is_file() else None
    if base is None:
        raise SystemExit("need _master/base_sit.png")

    # Optional AI keyframes
    blink_path = SESS / "27.jpg"
    crouch_path = SESS / "28.jpg"  # may not exist
    pounce_path = SESS / "26.jpg"
    stretch_path = SESS / "29.jpg"

    # Re-scan latest by trying known names
    blink = pack_source(blink_path) if blink_path.is_file() else base
    pounce = pack_source(pounce_path) if pounce_path.is_file() else scale_about_center(shift(base, 8, -10), 1.08)
    # crouch / stretch generated in parallel — find newest if numbered differently
    crouch = base
    stretch = scale_about_center(shift(base, 0, 4), 1.06)
    for p in sorted(SESS.glob("*.jpg"), key=lambda x: x.stat().st_mtime, reverse=True)[:8]:
        # Heuristic: use files we know from this session if names match above
        pass
    if crouch_path.is_file():
        crouch = pack_source(crouch_path)
    if stretch_path.is_file():
        stretch = pack_source(stretch_path)

    # Also try any path passed as extra keyframe map via env-less discovery
    for cand in SESS.glob("*.jpg"):
        pass

    # ── idle_blink (loop) ──
    blink_frames = [
        base,
        base,
        lerp(base, blink, 0.55),
        blink,
        lerp(base, blink, 0.55),
        base,
        base,
        base,
    ]
    write_set("idle_blink", blink_frames, fps=6.0, loop=True)

    # ── one-shot actions ──
    write_set(
        "idle_stretch",
        [
            base,
            lerp(base, stretch, 0.4),
            stretch,
            scale_about_center(stretch, 1.03),
            stretch,
            lerp(base, stretch, 0.4),
            base,
        ],
        fps=8.0,
        loop=False,
    )
    write_set(
        "idle_cute",
        [
            base,
            shift(base, 0, -1),
            scale_about_center(base, 1.03),
            shift(base, 1, -1),
            base,
        ],
        fps=7.0,
        loop=False,
    )
    write_set(
        "idle_tail_wag",
        [
            base,
            rotate_about(base, -2),
            rotate_about(base, 2),
            rotate_about(base, -2),
            rotate_about(base, 2),
            base,
        ],
        fps=8.0,
        loop=False,
    )
    write_set(
        "idle_sleep",
        [
            lerp(base, blink, 0.7),
            blink,
            shift(blink, 0, 2),
            blink,
            lerp(base, blink, 0.5),
            base,
        ],
        fps=4.0,
        loop=False,
    )
    # Watching (loop ok for medium range)
    write_set(
        "idle_watch",
        [
            base,
            shift(base, 1, 0),
            shift(base, 2, -1),
            shift(base, 1, 0),
            base,
            shift(base, -1, 0),
            shift(base, -2, -1),
            base,
        ],
        fps=6.0,
        loop=True,
    )

    # ── approaching pounce (one-shot, ~0.5s @ 12fps ≈ 6 frames) ──
    # crouch may be from latest edit; try files 28 then fallback
    crouch_f = crouch if crouch_path.is_file() else scale_about_center(shift(base, 0, 6), 0.94)
    # Discover crouch/stretch from most recent AI outputs if numbered
    for n in range(26, 40):
        p = SESS / f"{n}.jpg"
        if not p.is_file():
            continue
    # Prefer explicit if present after this script's companion gens
    if (SESS / "28.jpg").is_file():
        crouch_f = pack_source(SESS / "28.jpg")
    if (SESS / "29.jpg").is_file():
        stretch = pack_source(SESS / "29.jpg")
        write_set(
            "idle_stretch",
            [
                base,
                lerp(base, stretch, 0.4),
                stretch,
                scale_about_center(stretch, 1.03),
                stretch,
                lerp(base, stretch, 0.4),
                base,
            ],
            fps=8.0,
            loop=False,
        )

    pounce_frames = [
        crouch_f,
        lerp(crouch_f, pounce, 0.35),
        pounce,
        scale_about_center(shift(pounce, 4, -6), 1.06),
        lerp(pounce, base, 0.45),
        base,
    ]
    write_set("approaching", pounce_frames, fps=12.0, loop=False)

    # Keep interaction clips consistent with current base (light variants)
    write_set(
        "playing_interaction",
        [
            base,
            rotate_about(base, -3),
            shift(base, 0, -2),
            rotate_about(base, 3),
            base,
            shift(base, 0, -1),
        ],
        fps=10.0,
        loop=True,
    )
    write_set(
        "dragging",
        [rotate_about(base, -5), rotate_about(base, -2), rotate_about(base, 2), rotate_about(base, 5)],
        fps=8.0,
        loop=True,
    )
    write_set(
        "edge_peek",
        [shift(base, 0, 16), shift(base, 0, 10), shift(base, 0, 6), shift(base, 0, 10)],
        fps=4.0,
        loop=True,
    )
    write_set(
        "reminder_wave",
        [
            base,
            rotate_about(shift(base, 0, -1), -4),
            rotate_about(shift(base, 0, -2), 4),
            rotate_about(shift(base, 0, -1), -4),
            base,
            shift(base, 0, -1),
        ],
        fps=7.0,
        loop=True,
    )
    write_set(
        "reminder_feed",
        [
            base,
            scale_about_center(base, 1.03),
            scale_about_center(shift(base, 0, -2), 1.05),
            scale_about_center(base, 1.03),
            base,
            shift(base, 0, 1),
        ],
        fps=7.0,
        loop=True,
    )
    print("done — current base_sit preserved as identity")


if __name__ == "__main__":
    main()
