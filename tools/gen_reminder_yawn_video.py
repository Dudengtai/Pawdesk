"""Generate a continuous yawn video from closed sit (for fluid reminder_wave)."""

from __future__ import annotations

import base64
import json
import os
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
VIDEO_DIR = ROOT / "assets" / "pets" / "cow-cat" / "_video"
# Closed mouth identity lock
MASTER = ROOT / "assets" / "pets" / "cow-cat" / "_master" / "base_sit_magenta.jpg"
OUT_NAME = "reminder_yawn"

PROMPT = (
    "Exact same cartoon black-and-white tuxedo cat sitting full body on solid pure magenta. "
    "Locked camera, no zoom, no cuts, one continuous shot. "
    "Timeline: seconds 0-1 keep mouth fully closed calm sit; "
    "seconds 1-3 slowly open the jaw in a smooth cat yawn, mouth gets wider each moment, "
    "show dark oral cavity and a clear vivid hot-pink tongue (never white); "
    "seconds 3-3.5 hold the huge open yawn with pink tongue; "
    "seconds 3.5-6 slowly close the mouth smoothly returning to the exact original closed sit. "
    "Motion must be fluid frame-to-frame, no morph ghosting, no sudden pose jumps. "
    "Keep identity, yellow-green eyes, teal collar, fur pattern fixed. Solid magenta background always."
)


def bearer() -> str:
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
    try:
        with urllib.request.urlopen(req, timeout=180) as resp:
            raw = resp.read().decode()
            return json.loads(raw) if raw else {}
    except urllib.error.HTTPError as e:
        err = e.read().decode("utf-8", errors="replace")
        raise SystemExit(f"HTTP {e.code}: {err[:800]}") from e


def main() -> None:
    if not MASTER.is_file():
        raise SystemExit(f"missing {MASTER}")
    VIDEO_DIR.mkdir(parents=True, exist_ok=True)
    token = bearer()
    b64 = base64.standard_b64encode(MASTER.read_bytes()).decode()
    body = {
        "model": "grok-imagine-video",
        "prompt": PROMPT,
        "duration": 6,
        "image": {"url": f"data:image/jpeg;base64,{b64}"},
    }
    print(f"[{OUT_NAME}] start…", flush=True)
    start = api("POST", "https://api.x.ai/v1/videos/generations", token, body)
    rid = start.get("request_id")
    if not rid:
        raise SystemExit(f"no request_id: {start}")
    print(f"[{OUT_NAME}] request_id={rid}", flush=True)

    t0 = time.time()
    while time.time() - t0 < 400:
        time.sleep(4)
        st = api("GET", f"https://api.x.ai/v1/videos/{rid}", token)
        status = st.get("status")
        print(f"[{OUT_NAME}] {status} {st.get('progress')}", flush=True)
        if status == "done":
            url = st["video"]["url"]
            out = VIDEO_DIR / f"{OUT_NAME}.mp4"
            urllib.request.urlretrieve(url, out)
            (VIDEO_DIR / f"{OUT_NAME}_job.json").write_text(
                json.dumps(st, indent=2), encoding="utf-8"
            )
            print(f"[{OUT_NAME}] saved {out} ({out.stat().st_size} bytes)", flush=True)
            return
        if status in ("failed", "error"):
            raise SystemExit(f"{OUT_NAME} failed: {st}")
    raise SystemExit(f"{OUT_NAME} timeout")


if __name__ == "__main__":
    main()
