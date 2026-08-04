"""HTTP server for ZDR video: PUT /upload.mp4 receives result; GET serves files."""

from __future__ import annotations

import sys
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from urllib.parse import unquote, urlparse

ROOT = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("assets/pets/cow-cat/_video")
PORT = int(sys.argv[2]) if len(sys.argv) > 2 else 8765
ROOT.mkdir(parents=True, exist_ok=True)
UPLOAD = ROOT / "upload.mp4"


class Handler(BaseHTTPRequestHandler):
    def do_PUT(self):  # noqa: N802
        n = int(self.headers.get("Content-Length", "0"))
        data = self.rfile.read(n)
        # Always save primary upload target
        target = UPLOAD
        path = unquote(urlparse(self.path).path).lstrip("/")
        if path and path != "upload.mp4":
            target = ROOT / Path(path).name
        target.write_bytes(data)
        print(f"PUT {self.path} -> {target} ({len(data)} bytes)", flush=True)
        self.send_response(200)
        self.send_header("Content-Type", "text/plain")
        self.end_headers()
        self.wfile.write(b"ok")

    def do_GET(self):  # noqa: N802
        path = unquote(urlparse(self.path).path).lstrip("/")
        if not path or path == "/":
            path = "upload.mp4"
        # prevent path traversal
        name = Path(path).name
        f = ROOT / name
        if not f.is_file():
            self.send_response(404)
            self.end_headers()
            self.wfile.write(b"not found")
            return
        body = f.read_bytes()
        ctype = "video/mp4" if f.suffix.lower() == ".mp4" else "image/jpeg"
        if f.suffix.lower() == ".png":
            ctype = "image/png"
        self.send_response(200)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
        print(f"GET {name} ({len(body)} bytes)", flush=True)

    def log_message(self, fmt, *args):
        print(fmt % args, flush=True)


if __name__ == "__main__":
    print(f"listening :{PORT} root={ROOT.resolve()}", flush=True)
    HTTPServer(("0.0.0.0", PORT), Handler).serve_forever()
