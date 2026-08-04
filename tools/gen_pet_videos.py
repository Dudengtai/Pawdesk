"""Generate pet motion videos via xAI image-to-video (coding JWT / XAI_API_KEY).

Does NOT use output.upload_url (coding auth returns temporary video URL).
Downloads mp4 into assets/pets/cow-cat/_video/
"""

from __future__ import annotations

import base64
import json
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VIDEO_DIR = ROOT / "assets" / "pets" / "cow-cat" / "_video"
MASTER = ROOT / "assets" / "pets" / "cow-cat" / "_master" / "base_sit_magenta.jpg"

JOBS = [
    (
        "idle",
        "Same black-and-white tuxedo cat with yellow-green eyes and teal collar, "
        "centered full body sitting, gentle idle breathing soft chest rise and fall, "
        "occasional slow blink, tiny ear twitch, locked camera, solid magenta background, "
        "keep exact same character, no morph, no zoom out",
        6,
    ),
    (
        "stretch",
        "Same exact cat character sitting, performs a lazy front-paw stretch then returns "
        "to sit, yellow-green eyes, teal collar, locked camera, solid magenta background, "
        "keep identity",
        6,
    ),
    (
        "cute",
        "Same exact cat character sitting, cute head tilt and sweet look then returns, "
        "yellow-green eyes, teal collar, locked camera, solid magenta background, keep identity",
        6,
    ),
    (
        "pounce",
        "Same exact cat character crouches then pounces forward mid-air with paws out then lands "
        "sitting, yellow-green eyes, teal collar, locked camera, solid magenta background, "
        "keep identity, full body always visible",
        6,
    ),
]


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


def gen_one(token: str, name: str, prompt: str, duration: int) -> Path:
    VIDEO_DIR.mkdir(parents=True, exist_ok=True)
    raw = MASTER.read_bytes()
    b64 = base64.standard_b64encode(raw).decode()
    body = {
        "model": "grok-imagine-video",
        "prompt": prompt,
        "duration": duration,
        "image": {"url": f"data:image/jpeg;base64,{b64}"},
    }
    print(f"[{name}] start…", flush=True)
    start = api("POST", "https://api.x.ai/v1/videos/generations", token, body)
    rid = start["request_id"]
    print(f"[{name}] request_id={rid}", flush=True)
    t0 = time.time()
    while time.time() - t0 < 360:
        time.sleep(3)
        st = api("GET", f"https://api.x.ai/v1/videos/{rid}", token)
        status = st.get("status")
        print(f"[{name}] {status} {st.get('progress')}", flush=True)
        if status == "done":
            url = st["video"]["url"]
            out = VIDEO_DIR / f"{name}.mp4"
            urllib.request.urlretrieve(url, out)
            (VIDEO_DIR / f"{name}_job.json").write_text(
                json.dumps(st, indent=2), encoding="utf-8"
            )
            print(f"[{name}] saved {out} ({out.stat().st_size} bytes)", flush=True)
            return out
        if status in ("failed", "error"):
            raise SystemExit(f"{name} failed: {st}")
    raise SystemExit(f"{name} timeout")


def main() -> None:
    if not MASTER.is_file():
        raise SystemExit(f"missing {MASTER}")
    token = bearer()
    for name, prompt, dur in JOBS:
        out = VIDEO_DIR / f"{name}.mp4"
        if out.is_file() and out.stat().st_size > 10_000:
            print(f"[{name}] skip existing {out}", flush=True)
            continue
        gen_one(token, name, prompt, dur)
    print("all videos ready")


if __name__ == "__main__":
    main()
