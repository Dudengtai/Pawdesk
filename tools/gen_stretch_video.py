"""Generate a realistic cat-stretch i2v clip from magenta master.

Storyboard (user target — SIDE view, face LEFT, body horizontal on desk):
  front sit → turn / lower into SIDE PROFILE facing LEFT
  → chest down, front paws slide LEFT along ground
  → body long HORIZONTAL across frame, rear on the right, tail up
  → hold → return to upright front sitting pose.

Run:  python tools/gen_stretch_video.py
"""

from __future__ import annotations

import base64
import json
import time
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
MASTER = ROOT / "assets" / "pets" / "cow-cat" / "_master" / "base_sit_magenta.jpg"
VIDEO_DIR = ROOT / "assets" / "pets" / "cow-cat" / "_video"

PROMPT = (
    "CRITICAL: keep the EXACT same character, face paint style, pink nose, teal collar "
    "with silver bell, and fur shading as the reference still — same identity, not a redraw. "
    "Full body always visible, locked fixed camera, solid pure magenta background. "
    "START: upright front-facing sitting pose matching the reference image exactly "
    "(clean pink nose, small neat mouth, white muzzle — no black smudge on the face). "
    "THEN a realistic lazy morning stretch in SIDE PROFILE facing LEFT: "
    "1) body turns to side view, head points LEFT, "
    "2) chest lowers, front white paws slide LEFT along the floor (horizontal stretch "
    "across the desk — NOT toward camera, NOT upright begging), "
    "3) long horizontal spine left-to-right, rear on the RIGHT, "
    "4) black tail lifts UP behind the rear, "
    "5) hold briefly with soft/closed eyes, clean facial features still match reference, "
    "6) return smoothly to the ORIGINAL front-facing upright sit (same face as start). "
    "Paint style must stay consistent with the reference throughout (soft shaded cartoon, "
    "not flat line-art). No morph, no zoom, no camera pan."
)


def bearer() -> str:
    import os

    if os.environ.get("XAI_API_KEY"):
        return os.environ["XAI_API_KEY"].strip()
    auth = json.loads((Path.home() / ".grok" / "auth.json").read_text(encoding="utf-8"))
    for v in auth.values():
        if isinstance(v, dict) and isinstance(v.get("key"), str):
            return v["key"]
    raise SystemExit("no auth")


def api(method: str, url: str, token: str, body: dict | None = None) -> dict:
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(
        url,
        data=data,
        method=method,
        headers={
            "Authorization": f"Bearer {token}",
            "Content-Type": "application/json",
        },
    )
    with urllib.request.urlopen(req, timeout=180) as resp:
        raw = resp.read().decode()
        return json.loads(raw) if raw else {}


def main() -> None:
    if not MASTER.is_file():
        raise SystemExit(f"missing {MASTER} — create magenta master first")
    VIDEO_DIR.mkdir(parents=True, exist_ok=True)
    token = bearer()
    b64 = base64.standard_b64encode(MASTER.read_bytes()).decode()
    body = {
        "model": "grok-imagine-video",
        "prompt": PROMPT,
        "duration": 6,
        "image": {"url": f"data:image/jpeg;base64,{b64}"},
    }
    print("start stretch gen…", flush=True)
    start = api("POST", "https://api.x.ai/v1/videos/generations", token, body)
    print(json.dumps(start)[:600], flush=True)
    rid = start.get("request_id") or start.get("id")
    if not rid:
        raise SystemExit("no request_id")
    t0 = time.time()
    while time.time() - t0 < 420:
        time.sleep(4)
        st = api("GET", f"https://api.x.ai/v1/videos/{rid}", token)
        status = st.get("status")
        print(f"status={status} progress={st.get('progress')}", flush=True)
        if status == "done":
            url = st["video"]["url"]
            out = VIDEO_DIR / "stretch.mp4"
            urllib.request.urlretrieve(url, out)
            (VIDEO_DIR / "stretch_job.json").write_text(
                json.dumps(st, indent=2), encoding="utf-8"
            )
            print(f"saved {out} ({out.stat().st_size} bytes)", flush=True)
            return
        if status in ("failed", "error"):
            raise SystemExit(f"failed: {st}")
    raise SystemExit("timeout")


if __name__ == "__main__":
    main()
