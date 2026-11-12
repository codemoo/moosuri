"""Test-image-only listener inside each browser namespace, never production."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json

hits = 0


class Canary(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_GET(self):
        global hits
        if self.path == "/counts":
            data = json.dumps({"hits": hits}).encode()
        elif self.path == "/reset":
            hits = 0
            data = b"reset"
        else:
            hits += 1
            data = b"LOOPBACK_CANARY_SHOULD_NEVER_BE_READ"
        self.send_response(200)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


ThreadingHTTPServer(("127.0.0.1", 8081), Canary).serve_forever()
