#!/usr/bin/env python3
"""Linux gateway/Vault/Kiri integration with a synthetic TLS Google provider.
Usage: sudo python3 member-google.py /path/moosuri /path/vault-service sha256:TEST_IMAGE
Does not test real Google, OAuth app creation, browser UI, or a clean installer.
"""
import contextlib
import http.client
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import sys
import tempfile
import time
import threading
import importlib.util
import hashlib
from uuid import uuid4

ASSISTANT = "https://moosuri.example.test"
VAULT = "https://vault.example.test"


class UnixHTTP(http.client.HTTPConnection):
    def connect(self):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(self.timeout)
        self.sock.connect(self.host)


def directory(parent, name):
    path = parent / name
    path.mkdir(mode=0o700)
    return path


def port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def members(binary, vault_binary, harness, envelope):
    assert sys.platform == "linux" and os.geteuid() == 0
    with tempfile.TemporaryDirectory(prefix="moosuri-mg-") as tmp, contextlib.ExitStack() as stack:
        root = Path(tmp).resolve()
        registry, ipc, gateway = (directory(root, n) for n in ("registry", "ipc", "gateway"))
        env = {"PATH": os.defpath, "TMPDIR": str(root)}

        def run(args, password=None):
            result = subprocess.run([str(a) for a in args], input=password, text=True, capture_output=True, env=env, timeout=30)
            assert result.returncode == 0, f"fixture command failed: {args[1]}"
            return json.loads(result.stdout) if result.stdout.startswith("{") else None

        def launch(args, ready):
            process = subprocess.Popen([str(a) for a in args], env=env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            stack.callback(stop, process)
            deadline = time.monotonic() + 15
            while time.monotonic() < deadline and process.poll() is None:
                try:
                    if ready():
                        return process
                except (OSError, http.client.HTTPException):
                    pass
                time.sleep(0.05)
            raise AssertionError(f"fixture service failed: {args[0]}")

        def request_http(conn, method, path, body=None, headers=None):
            with contextlib.closing(conn) as c:
                c.request(method, path, body, headers or {})
                response = c.getresponse()
                raw = response.read()
                cookies = [v.split(";", 1)[0] for k, v in response.getheaders() if k.lower() == "set-cookie"]
                return response.status, json.loads(raw) if raw.startswith(b"{") else None, "; ".join(cookies)

        account_by_cookie = {}

        def app(method, path, body=None, cookie=None):
            headers = {"Host": "moosuri.example.test", "X-Real-IP": "192.0.2.1", "Sec-Fetch-Site": "same-origin"}
            if cookie:
                headers["Cookie"] = cookie
                headers["X-Moosuri-Account"] = account_by_cookie[cookie]
            if method == "POST":
                headers.update({"Origin": ASSISTANT, "Content-Type": "application/json"})
            result = request_http(UnixHTTP(str(gateway / "gateway.sock"), timeout=10), method, path,
                                  json.dumps(body) if body is not None else None, headers)
            if result[2] and result[1] and "user" in result[1]:
                account_by_cookie[result[2]] = result[1]["user"]["id"]
            return result

        def provision(label):
            email = label + "@example.test"
            allocation = run([binary, "account-runtime", "--registry", registry, "reserve", "--request-id", uuid4(), "--email", email])
            base = [binary, "personal-volume", "--root", directory(root, label), "--account", allocation["account_id"],
                    "--tenant", allocation["tenant_id"], "--volume", allocation["volume_id"], "--generation", allocation["generation"]]
            password = secrets.token_urlsafe(24)
            run([*base, "initialize", "--name", label, "--email", email, "--password-stdin"], password + "\n")
            sock = directory(ipc, allocation["account_id"]) / "personal.sock"
            launch([*base, "serve", "--socket", sock, "--vault-origin", VAULT], sock.is_socket)
            run([binary, "account-runtime", "--registry", registry, "activate", "--email", email, "--ipc-root", ipc])
            return allocation, email, password

        key = directory(root, "keys") / "adapter.key"
        key.write_text(secrets.token_hex(32))
        key.chmod(0o400)
        adapter_port, vault_port = port(), port()
        # Root-private config and socket; both the actual gateway and root
        # Google service access this synthetic installation registry.
        config = root / "capsule.json"
        config.write_text(json.dumps(harness.auth_config)); config.chmod(0o600)
        runtime_socket = root / "google.sock"
        listener = socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)
        listener.bind(str(runtime_socket)); os.chmod(runtime_socket,0o600); listener.listen(8)
        listener.settimeout(.2)
        brokers=[]; stopped=threading.Event()
        def serve():
            while not stopped.is_set():
                try: peer,_=listener.accept()
                except socket.timeout: continue
                except OSError: return
                with peer:
                    brokers.append(subprocess.Popen([binary,"runtime-google-browser","--registry",str(registry),
                        "--capsule-config",str(config),"--gateway-uid","0","--public-origin",ASSISTANT,
                        "--startup-lock",str(root/"google-start.lock"),
                        "--client-file",str(registry/"google-web-client.json")],stdin=peer,stdout=subprocess.DEVNULL,stderr=None))
        thread=threading.Thread(target=serve,daemon=True);thread.start()
        def finish_brokers():
            stopped.set();listener.close();thread.join(1)
            for process in brokers: stop(process)
        stack.callback(finish_brokers)
        first = provision("first")
        command = [binary, "personal-gateway", "--registry", registry, "--ipc-root", ipc, "--socket-dir", gateway,
                   "--public-origin", ASSISTANT, "--vault-site-origin", VAULT, "--vault-all-members",
                   "--google-runtime-socket",runtime_socket,"--google-runtime-installation",harness.owner,
                   "--vault-adapter-listen", f"127.0.0.1:{adapter_port}", "--vault-adapter-key-file", key]
        gateway_process = launch(command, (gateway / "gateway.sock").is_socket)
        second = provision("second")  # No gateway reconfiguration/restart.

        def vault(method, path, body=None, cookie=None, account=None):
            headers = {"Host": "vault.example.test"}
            if cookie:
                headers["Cookie"] = cookie
            if account:
                headers["X-Moosuri-Vault-Account"] = account
            if method == "POST":
                headers.update({"Origin": ASSISTANT, "Content-Type": "application/x-www-form-urlencoded"})
            return request_http(http.client.HTTPConnection("127.0.0.1", vault_port, timeout=10), method, path, body, headers)

        launch([vault_binary, "--data-dir", root / "vault-data", "--bind", f"127.0.0.1:{vault_port}",
                "--origin", VAULT, "--assistant-origin", ASSISTANT, "--adapter-url", f"http://127.0.0.1:{adapter_port}/internal/v1/vault",
                "--adapter-key-file", key], lambda: vault("GET", "/health/live")[0] == 204)
        sessions = []
        passwords = []
        for allocation, email, password in (first, second):
            status, _, cookie = app("POST", "/api/v1/auth/login", {"email": email, "password": password})
            assert status == 200
            replacement = secrets.token_urlsafe(24)
            passwords.append(replacement)
            status, _, changed_cookie = app("POST", "/api/v1/auth/password", {"password": replacement}, cookie)
            assert status == 200, ("password change", status)
            cookie = changed_cookie or cookie
            assert app("GET", "/api/v1/personal/vault", cookie=cookie)[1]["state"] == "ready"
            status, handoff, _ = app("POST", "/api/v1/personal/vault/handoff", {}, cookie)
            assert status == 200
            status, _, vault_cookie = vault("POST", "/session", "handoff=" + handoff["token"])
            assert status == 303 and vault_cookie
            assert vault("GET", "/session/current", cookie=vault_cookie)[1]["owner"]["account"] == allocation["account_id"]
            assert vault("POST", "/session", "handoff=" + handoff["token"])[0] == 401
            sessions.append((cookie, vault_cookie, allocation["account_id"]))

        endpoint="/api/v1/personal/google/kiri"
        def command(index,op,job,**fields):
            return app("POST",endpoint,dict(op=op,job=job,**fields),sessions[index][0])
        def wait(index,job,states):
            end=time.monotonic()+55
            while time.monotonic()<end:
                status,receipt,_=command(index,"status",job)
                assert status==200, ("status",status)
                if receipt["state"] in states:return receipt
                assert receipt["state"] not in {"unavailable","blocked","needs_human","cancelled"},receipt["state"]
                time.sleep(.2)
            raise AssertionError("phase timeout")
        def start(index,job=None):
            if job is None:
                job=str(uuid4());status,receipt,_=command(index,"start",job,service="gmail")
                assert status==200,("start",status)
            receipt=wait(index,job,{"vault"})
            route="/session/kiri-recipient?challenge="+receipt["challenge"]
            status,recipient,_=vault("GET",route,cookie=sessions[index][1])
            assert status==200,("recipient",status)
            return job,recipient,route
        pending=[str(uuid4()),str(uuid4())]
        for index,job in enumerate(pending):
            assert command(index,"start",job,service="gmail")[0]==200
        a,ra,route_a=start(0,pending[0]);b,rb,route_b=start(1,pending[1])
        assert ra["spki"]!=rb["spki"]
        for i,r in enumerate((ra,rb)):
            assert r["binding"]["account"]==sessions[i][2]
            assert r["binding"]["volume"]==(first,second)[i][0]["volume_id"]
        print("MG-01 concurrent new members receive distinct live guest-only recipients",flush=True)
        assert command(0,"start",str(uuid4()),service="gmail")[0]==409
        assert command(1,"start",str(uuid4()),service="gmail")[0]==409
        print("MG-09 one prepared capsule per member preserves the sibling slot",flush=True)
        assert vault("GET",route_a,cookie=sessions[1][1])[0]==503
        assert vault("GET",route_b,cookie=sessions[0][1])[0]==503
        print("MG-02 neither vault grant can retrieve the sibling recipient",flush=True)
        assert command(0,"cancel",a)[0]==200
        assert vault("GET",route_a,cookie=sessions[0][1])[0]==503
        assert vault("GET",route_b,cookie=sessions[1][1])[0]==200
        print("MG-03 cancellation invalidates its key without affecting the sibling",flush=True)
        # A cancelled prepared key must not lock this member out for 12 minutes.
        retry,rr,retry_route=start(0)
        assert rr["spki"]!=ra["spki"]
        stop(brokers[-1])
        assert wait(0,retry,{"unavailable"})["state"]=="unavailable"
        assert vault("GET",retry_route,cookie=sessions[0][1])[0]==503
        assert vault("GET",route_b,cookie=sessions[1][1])[0]==200
        print("MG-07 cancel/retry yields a fresh key; broker exit invalidates it without affecting sibling",flush=True)
        packet=envelope(rb,"selected@example.test")
        status,_,_=command(1,"deliver",b,google_email="selected@example.test",envelope=packet);assert status==200
        assert command(1,"deliver",b,google_email="selected@example.test",envelope=packet)[0]==200
        wrong=dict(packet,body=packet["body"][:-4]+"AAAA")
        assert command(1,"deliver",b,google_email="selected@example.test",envelope=wrong)[0]==409
        assert wait(1,b,{"setup_required"})["state"]=="setup_required"
        print("MG-04 actual Kiri login succeeds once; missing app remains explicit",flush=True)
        print("MG-05 exact delivery replay is idempotent; changed ciphertext is refused",flush=True)
        idle,ri,idle_route=start(0)
        assert app("POST","/api/v1/auth/logout",{},sessions[0][0])[0]==204
        assert vault("GET",idle_route,cookie=sessions[0][1])[0]==401
        deadline=time.monotonic()+12
        while time.monotonic()<deadline:
            if len(harness.resource_ids("container"))==1:break
            time.sleep(.2)
        assert len(harness.resource_ids("container"))==1
        print("MG-08 logout invalidates recipient lookup and reclaims its idle capsule",flush=True)
        # Logging in again does not reset the per-member start budget. A used
        # login has a separate conservative teardown fence; use A's idle path.
        status,_,cookie=app("POST","/api/v1/auth/login",{"email":first[1],"password":passwords[0]})
        assert status==200
        sessions[0]=(cookie,sessions[0][1],sessions[0][2])
        burst=str(uuid4())
        assert command(0,"start",burst,service="gmail")[0]==200
        assert command(0,"cancel",burst)[0]==200
        assert command(0,"start",str(uuid4()),service="gmail")[0]==429
        print("MG-10 cancelled preparations remain rate bounded across member logout/login",flush=True)
        # A fresh gateway clears obsolete receipts; root brokers must observe
        # closed sockets and remove every owned authentication container.
        stop(gateway_process)
        deadline=time.monotonic()+20
        while time.monotonic()<deadline:
            if len(harness.resource_ids("container"))==1:break
            time.sleep(.2)
        assert len(harness.resource_ids("container"))==1
        counts=json.loads(harness.proxy_exec("import urllib.request;print(urllib.request.urlopen('http://127.0.0.1:8081/evidence').read().decode())"))
        assert counts==dict(identifier=1,password=1,consent=0,returned=0,wrong_password=0),counts
        print("MG-06 gateway shutdown cleans all attempts; exact provider effects confirmed",flush=True)


def main(binary,vault_binary,image):
    assert Path('/etc/moosuri-kiri-lab-marker').read_text().strip()=='SYNTHETIC_DISPOSABLE_VM'
    browser=Path(__file__).resolve().parents[2]/'browser'
    sys.path.insert(0,str(browser))
    spec=importlib.util.spec_from_file_location('google_fixture_harness',browser/'google-auth-smoke.py')
    module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
    h=module.Harness('kiri',image)
    h.compose=h.docker+['compose','--env-file','/dev/null','-p',h.project,'-f',str(browser/'compose.yaml')]
    with tempfile.NamedTemporaryFile(mode='w',suffix='.json') as fixture:
        json.dump({'services':{'proxy':{'user':'0:0','entrypoint':['/usr/bin/tini','--','/usr/bin/python3','/capsule/google-fixture.py']}}},fixture);fixture.flush()
        h.compose+=['-f',fixture.name]
        try:
            h.configure();h.run('up','--no-build','-d','proxy');time.sleep(.5)
            proxy=json.loads(subprocess.check_output(h.docker+['inspect',h.run('ps','-q','proxy').strip()],env=h.env,text=True))[0]
            network=next(n for n in proxy['NetworkSettings']['Networks'].values() if n['IPAddress']==h.env['MOOSURI_BROWSER_PROXY_IP'])
            seccomp=Path(h.config_dir.name)/'seccomp.json';seccomp.write_bytes((browser/'seccomp.json').read_bytes());seccomp.chmod(0o600)
            h.auth_config=dict(engine='kiri',image_id=image,network_id=network['NetworkID'],proxy_container_id=proxy['Id'],proxy_ip=h.env['MOOSURI_BROWSER_PROXY_IP'],owner=h.owner,project=h.project,seccomp_file=str(seccomp),verified=True,smoke_evidence_sha256=hashlib.sha256(image.encode()).hexdigest(),javascript_enabled=True)
            members(binary,vault_binary,h,module.envelope)
        finally:
            if h.cleanup(): raise AssertionError('owned fixture cleanup failed')

if __name__=='__main__':main(*sys.argv[1:])
