"""Synthetic authentication provider. No forwarding to the real internet.

Test image only. All accounts/passwords are synthetic; request logs are disabled.
"""
import html
import http.server
import http.cookies
import json
import select
import socket
import socketserver
import ssl
import threading
from urllib.parse import parse_qs, urlsplit, urlencode

COUNTS = dict(identifier=0, password=0, consent=0, returned=0, wrong_password=0)
ALLOWED = {'accounts.google.com:443', 'myaccount.google.com:443', 'moosuri.example.test:443'}


class Page(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def reply(self, text, status=200, cookies=()):
        raw = text.encode()
        self.send_response(status)
        self.send_header('Content-Type', 'text/html; charset=utf-8')
        self.send_header('Content-Length', str(len(raw)))
        for cookie in cookies:
            self.send_header('Set-Cookie', cookie)
        self.end_headers()
        self.wfile.write(raw)

    def redirect(self, url, cookies=()):
        self.send_response(303)
        self.send_header('Location', url)
        for cookie in cookies:
            self.send_header('Set-Cookie', cookie)
        self.send_header('Content-Length', '0')
        self.end_headers()

    def do_GET(self):
        url = urlsplit(self.path)
        query = parse_qs(url.query)
        cookies = http.cookies.SimpleCookie(self.headers.get('Cookie', ''))
        if url.path == '/evidence':
            return self.reply(json.dumps(COUNTS))
        if url.path == '/ServiceLogin':
            # Match the current live Google email/phone field, which is text.
            return self.reply('<form method="POST" action="/identifier"><input type="text" id="identifierId" name="email"><button id="identifierNext">Next</button></form>')
        if self.headers.get('Host') == 'myaccount.google.com' and url.path == '/':
            email = query.get('email', [''])[0]
            if not email.endswith('@example.test'):
                return self.reply('invalid fixture account', 400)
            return self.reply('<button aria-label="Google Account: Synthetic (' + html.escape(email) + ')">Account</button>')
        if url.path == '/o/oauth2/v2/auth':
            email = query.get('login_hint', [''])[0]
            if cookies.get('signed') is None or cookies['signed'].value != email:
                return self.reply('Access blocked: missing same-profile fixture session', 403)
            callback = query.get('redirect_uri', [''])[0]
            if callback != 'https://moosuri.example.test/api/v1/mail/oauth/callback':
                return self.reply('Access blocked: fixture callback', 403)
            return self.reply('<div data-email="' + html.escape(email) + '">' + html.escape(email) +
                '</div><form method="POST" action="/consent"><button>Allow</button></form>')
        if self.headers.get('Host') == 'moosuri.example.test' and url.path == '/api/v1/mail/oauth/callback':
            COUNTS['returned'] += 1
            return self.reply('Synthetic callback received')
        self.reply('unknown fixture page', 404)

    def do_POST(self):
        length = int(self.headers.get('Content-Length', '0'))
        if length > 8192:
            return self.reply('invalid', 400)
        fields = parse_qs(self.rfile.read(length).decode())
        cookies = http.cookies.SimpleCookie(self.headers.get('Cookie', ''))
        if self.path == '/identifier':
            COUNTS['identifier'] += 1
            email = fields.get('email', [''])[0]
            if not email.endswith('@example.test'):
                return self.reply('fixture permits only synthetic accounts', 400)
            if email.startswith('rejected'):
                return self.reply('This browser or app may not be secure')
            if email.startswith('waiting'):
                return self.reply('Check your phone')
            visible = 'sibling@example.test' if email.startswith('mismatch') else email
            return self.reply('<div id="profileIdentifier">' + html.escape(visible) +
                '</div><form method="POST" action="/password"><input type="password" name="password"><button id="passwordNext">Next</button></form>',
                cookies=['selected=' + email + '; Path=/; Secure; HttpOnly; SameSite=Lax'])
        if self.path == '/password':
            COUNTS['password'] += 1
            if fields.get('password') != ['synthetic-only-password'] or cookies.get('selected') is None:
                COUNTS['wrong_password'] += 1
                return self.reply('wrong fixture password', 403)
            email = cookies['selected'].value
            home = 'https://myaccount.google.com/?' + urlencode({'email': email})
            signed = ['signed=' + email + '; Path=/; Secure; HttpOnly; SameSite=Lax']
            if email.startswith('verify'):
                return self.reply('Check your phone<script>setTimeout(()=>location.href=' + json.dumps(home) + ',1800)</script>', cookies=signed)
            return self.redirect(home, cookies=signed)
        if self.path == '/consent' and cookies.get('signed'):
            COUNTS['consent'] += 1
            if cookies['signed'].value.startswith('denied'):
                return self.redirect('https://moosuri.example.test/api/v1/mail/oauth/callback?error=access_denied&state=synthetic-state')
            if cookies['signed'].value.startswith('wrongstate'):
                return self.redirect('https://moosuri.example.test/api/v1/mail/oauth/callback?code=synthetic&state=other-state')
            return self.redirect('https://moosuri.example.test/api/v1/mail/oauth/callback?code=synthetic&state=synthetic-state')
        self.reply('invalid fixture request', 400)


class Tunnel(socketserver.StreamRequestHandler):
    def handle(self):
        first = self.rfile.readline(4097)
        if len(first) > 4096:
            return
        parts = first.decode('ascii', errors='replace').split()
        for _ in range(100):
            if self.rfile.readline(4097) in (b'\r\n', b'\n', b''):
                break
        if len(parts) != 3 or parts[0] != 'CONNECT' or parts[1] not in ALLOWED:
            self.wfile.write(b'HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\n\r\n')
            return
        # This is the only upstream and is owned by this same fixture process.
        with socket.create_connection(('127.0.0.1', 8443), timeout=5) as remote:
            self.wfile.write(b'HTTP/1.1 200 Connection established\r\n\r\n')
            self.wfile.flush()
            sockets = (self.connection, remote)
            while True:
                ready, _, _ = select.select(sockets, [], [], 30)
                if not ready:
                    return
                for source in ready:
                    chunk = source.recv(65536)
                    if not chunk:
                        return
                    (remote if source is self.connection else self.connection).sendall(chunk)


if __name__ == '__main__':
    server = http.server.ThreadingHTTPServer(('127.0.0.1', 8443), Page)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain('/capsule/test-tls/server.crt', '/capsule/test-tls/server.key')
    server.socket = context.wrap_socket(server.socket, server_side=True)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    # Metadata-only evidence endpoint, reachable only inside the proxy container.
    threading.Thread(target=http.server.ThreadingHTTPServer(('127.0.0.1', 8081), Page).serve_forever, daemon=True).start()
    socketserver.ThreadingTCPServer.allow_reuse_address = True
    with socketserver.ThreadingTCPServer(('0.0.0.0', 3128), Tunnel) as proxy:
        proxy.serve_forever()
