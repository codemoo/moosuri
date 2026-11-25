"""Exercise the real Linux bridge over a Unix socket using a synthetic owner guest."""
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
from uuid import uuid4

os.umask(0o077)
binary = sys.argv[1]
fixture = '''#!/usr/bin/env python3
import json,sys
from pathlib import Path
root=Path(__file__).parent
mode=(root/'case').read_text()
workspace_mode='moosuri_workspace.py' in sys.argv[1]
if 'secure_google.py' in sys.argv[1] and sys.argv[1].endswith(' session'):
 request=json.loads(input())
 assert request['request']=={'op':'finish'}
 print('{"finished":true}',flush=True);sys.exit(0)
print('{"prompt":"identifier","origin":"https://accounts.google.com"}',flush=True)
received=json.loads(input())
assert received['value']=='member@example.test'
assert ('workspace' in received)==workspace_mode
if mode=='challenge': result={'status':'needs_human','reason':'provider_sign_in_required'}
elif mode=='bad': result={'status':'ready','continuation':'member_workspace','session':'synthetic_extra'}
elif workspace_mode: result={'status':'ready','continuation':'member_workspace'}
else: result={'status':'ready','profile':'fresh','tab':'b'*32,'session':'a'*32}
print(json.dumps({'result':result}),flush=True)
if workspace_mode and mode=='workspace':
 assert json.loads(input())=={'retain':received['workspace']}
 data=json.loads((root/'state'/received['workspace']).read_text())
 assert data=={'v':1,'status':'ready','reason':'verified'}
 print('{"retained":true}',flush=True)
'''
with tempfile.TemporaryDirectory() as name:
    root=Path(name)
    state=root/'state'
    state.mkdir(mode=0o700)
    email=root/'email'
    email.write_text('member@example.test')
    guest=root/'guest'
    guest.write_text(fixture)
    guest.chmod(0o700)
    runid=None
    for mode, use_workspace, want in [
        ('default',False,('ready','verified')), ('workspace',True,('ready','verified')),
        ('replay',True,('ready','verified')), ('challenge',True,('needs_human','sign_in_required')),
        ('bad',True,('unavailable','bridge_error')),
    ]:
        if mode!='replay':
            runid=str(uuid4())
        (root/'case').write_text(mode)
        owner,relay=socket.socketpair()
        cmd=[binary,'kiri-chrome-signin','--state-directory',str(state),'--email-file',str(email),
             '--vm-ssh',str(guest),'--relay-uid',str(os.getuid())]
        if use_workspace: cmd+=['--workspace-continuation']
        with subprocess.Popen(cmd,stdin=owner,stdout=subprocess.DEVNULL,stderr=subprocess.PIPE) as child:
            owner.close()
            relay.settimeout(8)
            relay.sendall(json.dumps({'v':1,'operation':'google_sign_in','request_id':runid}).encode()+b'\n')
            result=json.loads(relay.makefile('rb').readline())
            assert result=={'v':1,'status':want[0],'reason':want[1]}, mode
            assert child.wait(timeout=3)==0
        relay.close()
        print(mode,'passed')
