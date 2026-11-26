#!/usr/bin/env python3
"""Six real egress/isolation checks in an installed disposable Kiri guest.

No login, credentials or page content. HTTPS probes contact Google's public
login entrypoint; only HTTP status is retained. All created resources are owned
by this invocation and removed. Existing installation resources are read-only.
"""
import hashlib
import ipaddress
import json
from pathlib import Path
import subprocess
import uuid

DOCKER = ['docker', '--host=unix:///run/kiri/docker.sock']


def run(args):
    r = subprocess.run(args, capture_output=True, text=True, timeout=30)
    if r.returncode:
        raise RuntimeError('egress_command_failed_' + Path(args[0]).name)
    return r.stdout.strip()


def docker(*args):
    return run(DOCKER + list(args))


def default_route(cid):
    routes = docker('exec', cid, 'cat', '/proc/net/route')
    return any(p[1] == '00000000' and p[2] != '00000000' and int(p[3], 16) & 3 == 3
               for line in routes.splitlines()[1:] if len(p := line.split()) >= 8)


def main():
    assert Path('/etc/moosuri-kiri-lab-marker').read_text().strip() == 'SYNTHETIC_DISPOSABLE_VM'
    path = Path('/etc/moosuri/google-browser.json')
    original = path.read_bytes()
    config = json.loads(original)
    project = 'moosuri-egress-' + uuid.uuid4().hex
    label = 'io.moosuri.egress-check=' + project
    networks, containers, evidence = [], [], {}
    used = [ipaddress.ip_network(c['Subnet'])
            for n in json.loads(docker('network', 'inspect', *docker('network', 'ls', '-q').splitlines()))
            for c in n.get('IPAM', {}).get('Config', []) if c.get('Subnet')]
    subnet = next(ipaddress.ip_network(f'10.177.{n}.0/24') for n in range(70, 220)
                  if not any(ipaddress.ip_network(f'10.177.{n}.0/24').overlaps(x) for x in used))
    ip = str(subnet.network_address + 2)
    try:
        public = docker('network', 'create', '--label', label, project + '_public')
        networks.append(public)
        internal = docker('network', 'create', '--internal', '--subnet', str(subnet), '--label', label, project + '_private')
        networks.append(internal)
        def create(name, network, command):
            cid = docker('create', '--name', project + '-' + name, '--label', label,
                         '--entrypoint', '/usr/bin/tini', '--user', '10001:10001',
                         '--read-only', '--cap-drop', 'ALL', '--security-opt', 'no-new-privileges:true',
                         '--network', network, '--pids-limit', '64', '--memory', '128m',
                         config['image_id'], '--', *command)
            containers.append(cid)
            return cid
        proxy = create('proxy', public, ['/usr/local/bin/browser-proxy'])
        docker('network', 'connect', '--ip', ip, internal, proxy)
        docker('start', proxy)
        assert default_route(proxy)
        evidence['public_primary_default_route'] = True
        def status(url):
            return run(['curl', '--silent', '--show-error', '--max-time', '20', '--noproxy', '',
                        '--proxy', f'http://{ip}:3128', '--output', '/dev/null', '--write-out', '%{http_code}', url])
        assert status('https://accounts.google.com/ServiceLogin') in ('200', '302', '303')
        evidence['real_google_https'] = True
        assert status('http://169.254.169.254/') == '403'
        assert status('http://127.0.0.1/') == '403'
        evidence['private_destinations_refused'] = True
        worker = create('private', internal, ['/bin/sleep', '120'])
        docker('start', worker)
        assert not default_route(worker)
        evidence['private_worker_has_no_external_route'] = True
        docker('stop', proxy)
        docker('start', proxy)
        assert default_route(proxy)
        assert status('https://accounts.google.com/ServiceLogin') in ('200', '302', '303')
        evidence['proxy_restart_retains_egress'] = True
    finally:
        for cid in reversed(containers):
            obj = json.loads(docker('inspect', cid))[0]
            assert obj['Config']['Labels'].get('io.moosuri.egress-check') == project
            docker('rm', '-f', cid)
        for net in reversed(networks):
            obj = json.loads(docker('network', 'inspect', net))[0]
            assert obj['Labels'].get('io.moosuri.egress-check') == project
            docker('network', 'rm', net)
    assert path.read_bytes() == original
    assert not docker('ps', '-a', '-q', '--filter', 'label=' + label)
    assert not docker('network', 'ls', '-q', '--filter', 'label=' + label)
    evidence['owned_cleanup_existing_config_unchanged'] = True
    print(json.dumps({'checks': evidence, 'passed': len(evidence), 'config_sha256': hashlib.sha256(original).hexdigest()}))


if __name__ == '__main__':
    main()
