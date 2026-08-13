"""One-shot: grok-imagine-video i2v for look-turn bake."""

from __future__ import annotations

import base64
import json
import os
import time
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
IMG = ROOT / "assets" / "pets" / "cow-cat" / "look_yaw" / "_keys7" / "3.png"
OUT_DIR = ROOT / "assets" / "pets" / "cow-cat" / "_video"


def bearer() -> str:
    env = os.environ.get("XAI_API_KEY") or os.environ.get("GROK_API_KEY")
    if env:
        return env.strip()
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
            "User-Agent": "PawDesk-video-probe/1.0",
        },
    )
    with urllib.request.urlopen(req, timeout=180) as resp:
        raw = resp.read().decode("utf-8", "replace")
        return json.loads(raw) if raw else {}


def main() -> None:
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    token = bearer()
    b64 = base64.standard_b64encode(IMG.read_bytes()).decode("ascii")
    body = {
        "model": "grok-imagine-video",
        "prompt": (
            "Same sitting tuxedo cow-cat, pointed ears, locked camera. "
            "Only the head turns slowly left, then right, then back to face the camera. "
            "Body, paws and tail stay still. Isolated character, flat background, keep identity."
        ),
        "duration": 6,
        "image": {"url": f"data:image/png;base64,{b64}"},
    }
    print("starting i2v from", IMG, flush=True)
    start = api("POST", "https://api.x.ai/v1/videos/generations", token, body)
    print(json.dumps(start)[:400], flush=True)
    rid = start.get("request_id") or start.get("id")
    if not rid:
        raise SystemExit("no request_id")
    print("request_id", rid, flush=True)
    t0 = time.time()
    while time.time() - t0 < 300:
        time.sleep(4)
        st = api("GET", f"https://api.x.ai/v1/videos/{rid}", token)
        status = st.get("status") or st.get("state")
        print(f"status={status} progress={st.get('progress')}", flush=True)
        if status == "done":
            url = st["video"]["url"]
            out = OUT_DIR / "look_turn.mp4"
            urllib.request.urlretrieve(url, out)
            (OUT_DIR / "look_turn_job.json").write_text(
                json.dumps(st, indent=2), encoding="utf-8"
            )
            print("SAVED", out, out.stat().st_size, flush=True)
            return
        if status in ("failed", "error", "cancelled"):
            raise SystemExit(f"failed: {st}")
    raise SystemExit("timeout")


if __name__ == "__main__":
    main()
