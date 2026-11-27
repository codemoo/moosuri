#!/usr/bin/env python3
"""Real setup CLI/PTY input boundaries, only in a marked disposable Ubuntu VM.

Uses an intentionally absent bundle so accepted public inputs stop before any
product changes. It is not evidence of successful installation/cert issuance.
No real secret, provider call, stdin transcript or product write is required.
"""
from pathlib import Path
import argparse
import hashlib
import os
import pty
import select
import subprocess
import sys
import time


def main():
    p=argparse.ArgumentParser();p.add_argument('--binary',type=Path,required=True)
    p.add_argument('--entry-script',type=Path)
    args=p.parse_args();binary=args.binary.resolve()
    assert sys.platform=='linux' and os.geteuid()==0
    assert Path('/etc/moosuri-kiri-lab-marker').read_text().strip()=='SYNTHETIC_DISPOSABLE_VM'
    missing='/root/moosuri-setup-inputs-absent-bundle'
    assert not Path(missing).exists()
    watched=[Path('/etc/moosuri/kiri-install.json'),Path('/etc/moosuri/kiri-members.json'),Path('/etc/moosuri/vault-adapter.key')]
    before={p:hashlib.sha256(p.read_bytes()).hexdigest() if p.exists() else None for p in watched}
    base=[str(binary),'setup-kiri','--bundle',missing]

    def terminal(replies,expected):
        master,slave=pty.openpty()
        proc=subprocess.Popen(base,stdin=slave,stdout=slave,stderr=slave,close_fds=True)
        os.close(slave); output=b''; sent=0; deadline=time.monotonic()+15
        prompts=[b'Assistant HTTPS address',b'Separate Vault HTTPS address',b'Certificate contact email',b'[y/N]']
        try:
            while time.monotonic()<deadline:
                if select.select([master],[],[],.1)[0]:
                    try:block=os.read(master,4096)
                    except OSError:break
                    if not block:break
                    output+=block
                    assert len(output)<16384
                if sent<len(replies) and prompts[sent] in output:
                    os.write(master,(replies[sent]+'\n').encode());sent+=1
                if proc.poll() is not None:break
            assert proc.wait(timeout=2)!=0 and expected.encode() in output and sent==len(replies)
        finally:
            if proc.poll() is None:proc.kill();proc.wait()
            os.close(master)

    terminal(['http://assistant.example.com','https://vault.example.com'],'kiri_install_origin_invalid')
    terminal(['https://assistant.example.com','https://assistant.example.com'],'kiri_install_vault_origin_must_differ')
    print('PASS I09 actual terminal invalid/same origins stop before product state',flush=True)
    addresses=['https://assistant.example.com','https://vault.example.com','operator@example.com']
    terminal(addresses+[''],'kiri_install_certificate_terms_declined_no_product_changes')
    terminal(addresses+['yes'],'kiri_install_bundle_path_invalid')
    print('PASS I08/I09 actual terminal explicit consent; accepted choices reach bundle validation only',flush=True)
    read,write=os.pipe(); synthetic=b'Synthetic-pipe-only42\n'
    try:
        os.write(write,synthetic);os.close(write);write=None
        result=subprocess.run(base+['--password-stdin'],stdin=read,capture_output=True,timeout=5)
        assert result.returncode!=0 and b'kiri_install_public_options_required' in result.stderr
        assert os.read(read,1024)==synthetic
    finally:
        os.close(read)
        if write is not None:os.close(write)
    flags=['--public-origin',addresses[0],'--vault-origin',addresses[1],'--tls-certificate','/synthetic/cert.pem','--tls-key','/synthetic/key.pem','--password-stdin']
    result=subprocess.run(base+flags,input=synthetic,capture_output=True,timeout=5)
    assert result.returncode!=0 and b'kiri_install_bundle_path_invalid' in result.stderr
    assert b'HTTPS address' not in result.stdout
    print('PASS I10 actual nonterminal preserves unread password pipe; supplied public flags skip prompts',flush=True)
    if args.entry_script:
        for flags in [[],['--public-origin','https://assistant.example.com'],['--public-origin=','--password-stdin']]:
            result=subprocess.run(['bash',str(args.entry_script.resolve()),*flags],input=synthetic,capture_output=True,timeout=5)
            assert result.returncode==2 and b'Unattended setup needs' in result.stderr
        # Execute only the entry's host/input preflight for accepted options.
        # Downloads and source selection begin at this existing boundary.
        parts=args.entry_script.read_text().split('\nsource_root=',1)
        assert len(parts)==2
        flags=['--public-origin='+addresses[0],'--vault-origin',addresses[1],
               '--admin-email=operator@example.com','--admin-name','Synthetic Operator','--password-stdin']
        for tls in [['--tls-certificate=/synthetic/cert.pem','--tls-key','/synthetic/key.pem'],
                    ['--certbot-email','operator@example.com','--accept-certbot-terms']]:
            result=subprocess.run(['bash','-s','--',*flags,*tls],input=parts[0].encode(),capture_output=True,timeout=5)
            assert result.returncode==0
        print('PASS I10 source entry rejects missing/partial/empty flags; preflight accepts both complete TLS modes',flush=True)
    assert before=={p:hashlib.sha256(p.read_bytes()).hexdigest() if p.exists() else None for p in watched}
    print('PASS setup input boundaries retain existing product configuration and key',flush=True)


if __name__=='__main__':
    if not __debug__:raise SystemExit('assertions_required')
    try:main()
    except Exception as error:
        print('FAIL setup-inputs: '+type(error).__name__,file=sys.stderr)
        raise SystemExit(1)
