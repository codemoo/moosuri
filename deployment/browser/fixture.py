"""Synthetic fixtures in an owned proxy namespace. No host files/credentials."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import ipaddress
import json
import os
import socket
import struct
import threading

PROXY = str(ipaddress.IPv4Address(os.environ["MOOSURI_BROWSER_PROXY_IP"]))
PUBLIC = "93.184.216.34"  # assigned only to this container's loopback by proxy-test.sh
counts = {"canary": 0, "public": 0, "dns_public": 0, "dns_private": 0}
dns_private = False


def dns():
    server = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    server.bind(("0.0.0.0", 53))
    while True:
        packet, address = server.recvfrom(4096)
        try:
            end = 12
            while packet[end]:
                end += packet[end] + 1
            end += 1
            qtype, qclass = struct.unpack("!HH", packet[end:end + 4])
            end += 4
            answer = b""
            if qtype == 1 and qclass == 1:
                key = "dns_private" if dns_private else "dns_public"
                counts[key] += 1
                ip = PROXY if dns_private else PUBLIC
                answer = b"\xc0\x0c" + struct.pack("!HHIH", 1, 1, 0, 4) + socket.inet_aton(ip)
            response = packet[:2] + struct.pack("!HHHHH", 0x8180, 1, 1 if answer else 0, 0, 0) + packet[12:end] + answer
            server.sendto(response, address)
        except (IndexError, ValueError, struct.error):
            pass


class Public(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_GET(self):
        global dns_private
        if self.path == "/rebind":
            dns_private = True
            data = b"rebound"
        elif self.path == "/counts":
            data = json.dumps(counts).encode()
        elif self.path == "/reset-counts":
            for key in counts:
                counts[key] = 0
            dns_private = False
            data = b"reset"
        elif self.path == "/redirect":
            counts["public"] += 1
            self.send_response(302)
            self.send_header("Location", f"http://{PROXY}/private-redirect")
            self.end_headers()
            return
        elif self.path == "/refresh":
            counts["public"] += 1
            data = b'<meta http-equiv="refresh" content="1;url=/changed"><p>before refresh</p>'
        elif self.path == "/changed":
            counts["public"] += 1
            data = b'<p>changed navigation</p>'
        else:
            counts["public"] += 1
            data = f'''<!doctype html><title>Contained Chromium fixture</title>
<p><span aria-hidden="true">not </span>refundable</p><p id="rendered">script did not run</p>
<script>document.getElementById('rendered').textContent='SCRIPT_RENDERED_OK';
fetch('http://{PROXY}:8081/fetch').catch(()=>{{}});
fetch('http://127.0.0.1:8081/loopback').catch(()=>{{}});</script>
<img src="http://{PROXY}:8081/img"><iframe src="http://{PROXY}:8081/frame"></iframe>'''.encode()
        self.send_response(200)
        self.send_header("Content-Type", "text/html; charset=utf-8")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


class Canary(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_GET(self):
        counts["canary"] += 1
        self.send_response(200)
        data = b"PRIVATE_CANARY_SHOULD_NEVER_BE_READ"
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


threading.Thread(target=ThreadingHTTPServer(("0.0.0.0", 8081), Canary).serve_forever, daemon=True).start()
threading.Thread(target=ThreadingHTTPServer((PROXY, 80), Canary).serve_forever, daemon=True).start()
threading.Thread(target=dns, daemon=True).start()
ThreadingHTTPServer((PUBLIC, 80), Public).serve_forever()
