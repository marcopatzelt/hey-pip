#!/usr/bin/env python3
"""Dev server for tools/preview.html: serves crates/app on 127.0.0.1 and answers /slow?ms=N with a 1x1 PNG
after N ms (keeps the page's load event open so a headless screenshot waits for the scene)."""
import base64
import functools
import http.server
import sys
import time
import urllib.parse
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent  # crates/app
PNG = base64.b64decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=")


class Handler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        url = urllib.parse.urlparse(self.path)
        if url.path != "/slow":
            return super().do_GET()
        ms = int(urllib.parse.parse_qs(url.query).get("ms", ["1000"])[0])
        time.sleep(min(max(ms, 0), 10000) / 1000)
        self.send_response(200)
        self.send_header("Content-Type", "image/png")
        self.send_header("Content-Length", str(len(PNG)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(PNG)

    def log_message(self, *args):
        pass


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8765
    handler = functools.partial(Handler, directory=str(ROOT))
    http.server.ThreadingHTTPServer(("127.0.0.1", port), handler).serve_forever()
