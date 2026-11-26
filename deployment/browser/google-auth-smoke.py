"""Real Chrome/Kiri worker acceptance against an owned synthetic TLS provider.

Requires the public google-auth-test image and host Python cryptography. No live
credentials, Google accounts, developer skill files or production volumes.
"""
import argparse
import base64
import json
import os
import subprocess
import tempfile
import time
import uuid
import hashlib
from pathlib import Path
from urllib.parse import urlencode
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import padding
from cryptography.hazmat.primitives.ciphers.aead import AESGCM
from smoke import Harness, JsonPipe


def binding():
    return dict(installation=str(uuid.uuid4()), account=str(uuid.uuid4()),
                tenant=str(uuid.uuid4()), volume=str(uuid.uuid4()), generation=1, epoch=1,
                challenge=str(uuid.uuid4()), email='member@example.test', origin='https://moosuri.example.test')


def envelope(recipient, email, change=None):
    b = dict(recipient['binding'])
    if change:
        b.update(change)
    label = ('moosuri-google-v3\n' + '\n'.join(str(b[k]) for k in
        ('installation', 'account', 'tenant', 'volume', 'generation', 'epoch', 'challenge', 'email'))
        + '\n' + email + '\n' + b['origin']).encode()
    key, iv = os.urandom(32), os.urandom(12)
    public = serialization.load_der_public_key(base64.b64decode(recipient['spki']))
    wrapped = public.encrypt(key, padding.OAEP(mgf=padding.MGF1(hashes.SHA256()), algorithm=hashes.SHA256(), label=label))
    encode = lambda value: base64.b64encode(value).decode()
    return dict(key=encode(wrapped), iv=encode(iv), body=encode(AESGCM(key).encrypt(iv, b'synthetic-only-password', label)))


class Attempt:
    def __init__(self, h):
        self.h, self.binding = h, binding()
        self.binding['installation']=h.owner
        self.name = h.project + '-auth-' + uuid.uuid4().hex[:10]
        command = [h.launcher] if h.launcher else h.compose + ['run', '--rm', '-T', '--no-deps', '--name', self.name,
            '--env', 'MOOSURI_BROWSER_TENANT=' + self.binding['tenant'], 'browser', '--google-auth']
        self.p = subprocess.Popen(command,
            env=h.env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=None if h.launcher else subprocess.DEVNULL)
        h.workers.append(self)
        self.pipe = JsonPipe(self.p)
        self.pipe.write(dict(config=h.auth_config,binding=self.binding) if h.launcher else dict(protocol='moosuri-google-runtime-v1', binding=self.binding), 5)
        ready = self.pipe.read(40)
        assert ready.get('status') == 'vault', 'recipient was not prepared: ' + str(ready.get('status'))
        self.recipient = ready['recipient']
        assert self.recipient['binding'] == self.binding
        if h.launcher:
            names=h.resource_ids('container')
            rows=json.loads(subprocess.check_output(h.docker+['inspect',*names],env=h.env,text=True))
            found=[r['Name'].lstrip('/') for r in rows if 'MOOSURI_BROWSER_TENANT='+self.binding['tenant'] in r['Config']['Env']]
            assert len(found)==1;self.name=found[0]
        self.assert_no_chrome()

    def assert_no_chrome(self):
        code = "from pathlib import Path;print(sum(b'/opt/google/chrome/chrome' in p.read_bytes().split(b'\\0')[:1] for p in Path('/proc').glob('[0-9]*/cmdline')))"
        assert self.h.exec_owned(self.name, 'python3', '-c', code).strip() == '0', 'Chrome started before release'

    def deliver(self, email='selected@example.test', change=None):
        self.email = email
        self.pipe.write(dict(google_email=email, envelope=envelope(self.recipient, email, change)), 5)
        return self.pipe.read(30)['status']

    def release(self, expected='signed_in', verify=False):
        self.assert_no_chrome()
        self.pipe.write(dict(proceed=self.binding['challenge']), 5)
        result = self.pipe.read(50)['status']
        if verify:
            assert result == 'verifying', 'device wait not surfaced'
            result = self.pipe.read(15)['status']
        assert result == expected, 'unexpected authentication phase: ' + result

    def consent(self, expected='returned'):
        client = 'synthetic.apps.googleusercontent.com'
        query = dict(client_id=client, redirect_uri=self.binding['origin'] + '/api/v1/mail/oauth/callback',
            login_hint=self.email, code_challenge_method='S256', response_type='code', include_granted_scopes='false',
            access_type='offline', state='synthetic-state', code_challenge='a' * 43,
            scope='https://www.googleapis.com/auth/gmail.readonly')
        self.pipe.write(dict(url='https://accounts.google.com/o/oauth2/v2/auth?' + urlencode(query), client_id=client, service='gmail'), 5)
        result = self.pipe.read(30)
        assert result == {'status':expected}, 'same-profile consent failed: ' + str(result.get('status'))
        self.p.wait(timeout=15)
        assert (self.p.returncode==0)==(expected=='returned'), 'incorrect worker exit status'

    def stop(self):
        if self.p.stdin and not self.p.stdin.closed:
            self.p.stdin.close()
        try:
            self.p.wait(timeout=15)
        except subprocess.TimeoutExpired:
            self.p.terminate()
            self.p.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--engine', choices=['docker','kiri'], required=True)
    parser.add_argument('--image', required=True)
    parser.add_argument('--launcher', help='absolute path to the public google_capsule_smoke Rust example (Kiri only)')
    args = parser.parse_args()
    h = Harness(args.engine, args.image)
    h.launcher=args.launcher
    if h.launcher: assert args.engine=='kiri' and Path(h.launcher).is_absolute()
    # Keep the public browser mount/network/cgroup contract. Only the test proxy
    # entrypoint and test image differ; no host CDP port or profile bind mount.
    h.compose = h.docker + ['compose', '--env-file', '/dev/null', '-p', h.project, '-f', str(__import__('pathlib').Path(__file__).parent / 'compose.yaml')]
    fixture = tempfile.NamedTemporaryFile(mode='w',suffix='.json')
    json.dump({'services':{'browser':{'memswap_limit':'768m'},'proxy':{'user':'0:0','entrypoint':['/usr/bin/tini','--','/usr/bin/python3','/capsule/google-fixture.py']}}}, fixture)
    fixture.flush(); h.compose += ['-f',fixture.name]
    try:
        h.configure(); h.run('up','--no-build','-d','proxy')
        time.sleep(.5)
        if h.launcher:
            proxy_id=h.run('ps','-q','proxy').strip()
            proxy=json.loads(subprocess.check_output(h.docker+['inspect',proxy_id],env=h.env,text=True))[0]
            network=next(n for n in proxy['NetworkSettings']['Networks'].values() if n['IPAddress']==h.env['MOOSURI_BROWSER_PROXY_IP'])
            seccomp=Path(h.config_dir.name)/'seccomp.json'
            seccomp.write_bytes((Path(__file__).parent/'seccomp.json').read_bytes())
            seccomp.chmod(0o600)
            h.auth_config=dict(engine='kiri',image_id=args.image,network_id=network['NetworkID'],proxy_container_id=proxy['Id'],
                proxy_ip=h.env['MOOSURI_BROWSER_PROXY_IP'],owner=h.owner,project=h.project,seccomp_file=str(seccomp),
                verified=True,smoke_evidence_sha256=hashlib.sha256(args.image.encode()).hexdigest(),javascript_enabled=True)
        a=Attempt(h); assert a.deliver()=='verified'; a.release(); a.consent()
        print('GR-01 vault ciphertext -> official Chrome -> same-profile consent passed', flush=True)
        a=Attempt(h); assert a.deliver('verify@example.test')=='verified'; a.release(verify=True); a.consent()
        print('GR-02 device verification phase and continuation passed', flush=True)
        before=json.loads(h.proxy_exec("import urllib.request;print(urllib.request.urlopen('http://127.0.0.1:8081/evidence').read().decode())"))
        for email in ('mismatch@example.test','rejected@example.test'):
            a=Attempt(h); assert a.deliver(email)=='verified'; a.release(expected='blocked'); a.stop()
        after=json.loads(h.proxy_exec("import urllib.request;print(urllib.request.urlopen('http://127.0.0.1:8081/evidence').read().decode())"))
        assert before['password']==after['password']
        print('GR-03/04 wrong visible account and rejected browser never receive password', flush=True)
        a=Attempt(h); assert a.deliver(change={'volume':str(uuid.uuid4())})=='unavailable'; a.stop()
        print('GR-05 wrong allocation ciphertext rejected before Chrome passed', flush=True)
        a=Attempt(h); assert a.deliver()=='verified'; a.pipe.write({'proceed':str(uuid.uuid4())},5)
        if h.launcher:
            a.p.wait(timeout=15);assert a.p.returncode!=0
        else: assert a.pipe.read(5)=={'status':'unavailable'}
        a.stop()
        print('GR-06 stale release rejected before Chrome passed', flush=True)
        a=Attempt(h); b=Attempt(h); a.stop()
        assert b.deliver('sibling@example.test')=='verified'; b.release(); b.consent()
        print('GR-07 cancelling a waiting member preserves sibling attempt passed', flush=True)
        for how in ('eof','signal'):
            a=Attempt(h); assert a.deliver('waiting@example.test')=='verified'
            a.pipe.write(dict(proceed=a.binding['challenge']),5)
            assert a.pipe.read(50)=={'status':'verifying'}
            if how=='signal':
                h.verify_owned('container',[a.name])
                subprocess.run(h.docker+['kill','--signal','TERM',a.name],env=h.env,capture_output=True,check=True,timeout=10)
            else:
                a.p.stdin.close()
            if not h.launcher:
                try: assert a.pipe.read(10)=={'status':'unavailable'}
                except RuntimeError as error:
                    # Docker may close attach output when PID1 receives TERM.
                    # This is not a teardown receipt; verify nonzero exit and
                    # owned-container removal below, as the Rust launcher does.
                    if how!='signal' or str(error)!='worker closed stdout before a complete response': raise
            a.p.wait(timeout=15); assert a.p.returncode!=0
        print('GR-09 login EOF and SIGTERM abort without password submission passed', flush=True)
        for email in ('denied@example.test','wrongstate@example.test'):
            a=Attempt(h); assert a.deliver(email)=='verified'; a.release(); a.consent(expected='blocked')
        print('GR-10 denied and wrong-state callbacks do not report returned passed', flush=True)
        counts=json.loads(h.proxy_exec("import urllib.request;print(urllib.request.urlopen('http://127.0.0.1:8081/evidence').read().decode())"))
        assert counts==dict(identifier=9,password=5,consent=5,returned=5,wrong_password=0), counts
        assert len(h.resource_ids('container'))==1, 'authentication container remains'
        print('GR-08 exact provider effects and complete worker cleanup passed', flush=True)
    finally:
        errors=h.cleanup(); fixture.close()
        if errors: raise RuntimeError('owned fixture cleanup failed')


if __name__ == '__main__':
    main()
