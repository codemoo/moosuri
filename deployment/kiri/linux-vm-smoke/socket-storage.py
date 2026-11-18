#!/usr/bin/env python3
from engine_api import *
import threading,struct
phase=sys.argv[1]
# Set this from the actual pulled RepoDigest, never use a moving tag in acceptance.
PIN='python@sha256:9ef1a599f863231c3d91e941389bfdc4a2d6070557243d54df1a4e57015b960e'
image=api('GET','/images/'+PIN+'/json')
assert image['Id']=='sha256:67fd5f7b4b120eaa3308775fe87aff98a2492287576bd6656ef785d54a1b5605'
assert PIN in image.get('RepoDigests',[])
emit('socket-image',{'Id':image['Id'],'RepoDigests':image.get('RepoDigests')})
server='''import socket,pathlib,os
p=pathlib.Path('/data');label=os.environ['LAB_LABEL']
(p/'owner').write_text(label+'-private\\n')
with (p/'starts').open('a') as f:f.write(label+'-start\\n')
s=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)
try:(p/'runtime.sock').unlink()
except FileNotFoundError:pass
s.bind('/data/runtime.sock');os.chmod('/data/runtime.sock',0o600);s.listen()
while True:
 c,_=s.accept();msg=c.recv(100);c.sendall((label+':').encode()+msg);c.close()
'''
if phase=='setup':
 for i,n in enumerate(('a','b')):
  spec={'Image':image['Id'],'User':f'{10001+i}:{10001+i}','Env':['LAB_LABEL='+n],
  'Cmd':['python3','-u','-c',server],
  'HostConfig':{'NetworkMode':'none','ReadonlyRootfs':True,'CapDrop':['ALL'],'SecurityOpt':['no-new-privileges:true'],
  'Memory':67108864,'NanoCpus':500000000,'PidsLimit':32,'RestartPolicy':{'Name':'unless-stopped'},
  'Binds':[f'/srv/moosuri-lab/{n}:/data:rw','/srv/moosuri-lab/shared:/shared:ro']}}
  emit(n+'-create',api('POST','/containers/create?name=lab191-socket-'+n,spec))
  api('POST','/containers/lab191-socket-'+n+'/start')
 time.sleep(.3)
broker_sockets=[]
for i,label in enumerate(('a','b')):
 p='/srv/moosuri-lab/'+label+'/broker.sock'
 try:os.unlink(p)
 except FileNotFoundError:pass
 listener=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);listener.bind(p);os.chown(p,10001+i,10001+i);os.chmod(p,0o600);listener.listen()
 broker_sockets.append(listener)
 def broker(listener=listener,label=label,uid=10001+i):
  conn,_=listener.accept();peer=struct.unpack('3i',conn.getsockopt(socket.SOL_SOCKET,socket.SO_PEERCRED,12));msg=conn.recv(100)
  conn.sendall((label+':broker-ok').encode() if peer[1]==uid and msg==b'synthetic-request' else b'DENIED');conn.close()
 threading.Thread(target=broker,daemon=True).start()
for n in ('a','b'):
 name='lab191-socket-'+n;c=api('GET','/containers/'+name+'/json')
 emit(n+'-state',{'State':c['State'],'RestartCount':c['RestartCount']})
 cmd="""python3 - <<'INNER'
import socket,pathlib,os,json
s=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);s.connect('/data/runtime.sock');s.sendall(b'own-runtime');reply=s.recv(100).decode();assert reply==os.environ['LAB_LABEL']+':own-runtime';print('socket_reply',reply);s.close()
broker=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);broker.connect('/data/broker.sock');broker.sendall(b'synthetic-request');reply=broker.recv(100).decode();assert reply==os.environ['LAB_LABEL']+':broker-ok';print('broker_reply',reply);broker.close()
print('owner',pathlib.Path('/data/owner').read_text().strip());print('starts',pathlib.Path('/data/starts').read_text().splitlines())
for p in ['/run/kiri/docker.sock','/var/run/docker.sock','/srv/moosuri-lab/a','/srv/moosuri-lab/b','/etc/moosuri-kiri-lab-marker']:
 assert not pathlib.Path(p).exists(),p
 print('absent',p,True)
for p in ['/root-denied','/shared/module']:
 try:pathlib.Path(p).write_text('must-fail');raise AssertionError('Unexpected write '+p)
 except OSError as e:print('write-denied',p,e.errno)
assert os.getuid() in (10001,10002)
status=pathlib.Path('/proc/self/status').read_text();assert 'NoNewPrivs:\t1' in status and 'Seccomp:\t2' in status
assert all(int(l.split()[1],16)==0 for l in status.splitlines() if l.startswith('Cap'))
print('uid',os.getuid());print('status',[(l.strip()) for l in pathlib.Path('/proc/self/status').read_text().splitlines() if l.startswith(('Cap','NoNewPrivs','Seccomp'))])
INNER"""
 result=exe(name,cmd);emit(n+'-isolation',result);assert result['exit']==0
 if phase=='quota':
  cmd="""python3 - <<'INNER'
import os,pathlib,errno
p=pathlib.Path('/data/quota-fill');size=0
try:
 with p.open('wb',buffering=0) as f:
  for _ in range(64):size+=f.write(b'x'*1048576)
 raise AssertionError('No filesystem size limit')
except OSError as e:
 assert e.errno==28 and size<33554432
 print('quota-denied',e.errno,'written',size)
finally:p.unlink(missing_ok=True)
print('owner-after-quota',pathlib.Path('/data/owner').read_text().strip())
INNER"""
  result=exe(name,cmd);emit(n+'-quota',result);assert result['exit']==0
 cg=pathlib.Path('/proc/'+str(c['State']['Pid'])+'/cgroup').read_text().strip().split(':')[-1];r=pathlib.Path('/sys/fs/cgroup'+cg)
 limits={k:(r/k).read_text().strip() for k in ('memory.max','cpu.max','pids.max')};emit(n+'-cgroup',limits)
 assert limits=={'memory.max':'67108864','cpu.max':'50000 100000','pids.max':'32'}
 emit(n+'-mount',subprocess.check_output(['findmnt','-n','-o','TARGET,SOURCE,FSTYPE,OPTIONS','/srv/moosuri-lab/'+n],text=True))
emit('disk',subprocess.check_output(['df','-B1','/srv/moosuri-lab/a','/srv/moosuri-lab/b'],text=True))
