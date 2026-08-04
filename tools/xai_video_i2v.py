"""Call xAI image-to-video with ZDR-compatible output.upload_url.

Usage:
  python tools/xai_video_i2v.py --image-url URL --upload-url URL --prompt "..." --out meta.json
Auth: XAI_API_KEY env, or Grok CLI auth.json JWT (fallback).
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path


def load_bearer() -> str:
    env = os.environ.get("XAI_API_KEY") or os.environ.get("GROK_API_KEY")
    if env:
        return env.strip()
    auth_path = Path.home() / ".grok" / "auth.json"
    if not auth_path.is_file():
        raise SystemExit("No XAI_API_KEY and no ~/.grok/auth.json")
    data = json.loads(auth_path.read_text(encoding="utf-8"))
    for v in data.values():
        if isinstance(v, dict) and isinstance(v.get("key"), str):
            return v["key"]
    raise SystemExit("auth.json has no key")


def http_json(method: str, url: str, bearer: str, body: dict | None = None) -> dict:
    data = None if body is None else json.dumps(body).encode("utf-8")
    req = urllib.request.Request(
        url,
        data=data,
        method=method,
        headers={
            "Authorization": f"Bearer {bearer}",
            "Content-Type": "application/json",
            "User-Agent": "PawDesk-video-pipeline/1.0",
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=120) as resp:
            raw = resp.read().decode("utf-8")
            return json.loads(raw) if raw else {}
    except urllib.error.HTTPError as e:
        err = e.read().decode("utf-8", errors="replace")
        raise SystemExit(f"HTTP {e.code} {url}: {err}") from e


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--image-url", default="")
    ap.add_argument("--image-file", default="", help="Local image; sent as data URI if set")
    ap.add_argument("--upload-url", required=True, help="Public URL that accepts HTTP PUT")
    ap.add_argument("--prompt", required=True)
    ap.add_argument("--duration", type=int, default=6)
    ap.add_argument("--model", default="grok-imagine-video")
    ap.add_argument("--poll-sec", type=float, default=4.0)
    ap.add_argument("--timeout-sec", type=float, default=300.0)
    ap.add_argument("--out", default="")
    args = ap.parse_args()

    bearer = load_bearer()
    if args.image_file:
        import base64

        raw = Path(args.image_file).read_bytes()
        b64 = base64.standard_b64encode(raw).decode("ascii")
        mime = "image/png" if args.image_file.lower().endswith(".png") else "image/jpeg"
        image_field = {"url": f"data:{mime};base64,{b64}"}
    elif args.image_url:
        image_field = {"url": args.image_url}
    else:
        raise SystemExit("need --image-url or --image-file")

    # ZDR body: image + prompt + output.upload_url
    body = {
        "model": args.model,
        "prompt": args.prompt,
        "duration": args.duration,
        "image": image_field,
        "output": {"upload_url": args.upload_url},
    }
    print("starting generation…", flush=True)
    start = http_json("POST", "https://api.x.ai/v1/videos/generations", bearer, body)
    print(json.dumps(start, indent=2)[:800], flush=True)
    rid = start.get("request_id") or start.get("id")
    if not rid:
        # some responses may complete inline
        if args.out:
            Path(args.out).write_text(json.dumps(start, indent=2), encoding="utf-8")
        raise SystemExit("no request_id in response")

    t0 = time.time()
    while time.time() - t0 < args.timeout_sec:
        time.sleep(args.poll_sec)
        st = http_json("GET", f"https://api.x.ai/v1/videos/{rid}", bearer)
        status = st.get("status") or st.get("state")
        print(f"poll status={status}", flush=True)
        if status in ("done", "succeeded", "completed", "success"):
            if args.out:
                Path(args.out).write_text(json.dumps(st, indent=2), encoding="utf-8")
            print("DONE", flush=True)
            print(json.dumps(st, indent=2)[:1200])
            return
        if status in ("failed", "error", "cancelled"):
            raise SystemExit(f"generation failed: {st}")
    raise SystemExit("timeout waiting for video")


if __name__ == "__main__":
    main()
