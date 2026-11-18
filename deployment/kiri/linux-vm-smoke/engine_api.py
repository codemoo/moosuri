#!/usr/bin/env python3
"""Synthetic Kiri smoke; run as root only inside a disposable marked VM."""
import http.client,json,socket,os,sys,time,pathlib,subprocess
assert pathlib.Path('/etc/moosuri-kiri-lab-marker').read_text().strip()=='SYNTHETIC_DISPOSABLE_VM'
class Unix(http.client.HTTPConnection):
 def connect(self):
  self.sock=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);self.sock.connect('/run/kiri/docker.sock')
def api(method,path,body=None):
 c=Unix('localhost',timeout=30);data=None if body is None else json.dumps(body)
 c.request(method,'/v1.47'+path,data,{'Content-Type':'application/json'});r=c.getresponse();b=r.read();c.close()
 if r.status>=400: raise RuntimeError((method,path,r.status,b.decode()))
 try:return json.loads(b)
 except:return b
def emit(k,v):print(json.dumps({'check':k,'result':v},sort_keys=True),flush=True)
def exe(n,cmd):
 e=api('POST','/containers/'+n+'/exec',{'Cmd':['sh','-c',cmd],'AttachStdout':True,'AttachStderr':True})
 sock=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM);sock.settimeout(15);sock.connect('/run/kiri/docker.sock')
 body=b'{"Detach":false,"Tty":false}'
 sock.sendall(('POST /v1.47/exec/'+e['Id']+'/start HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nConnection: Upgrade\r\nUpgrade: tcp\r\nContent-Length: '+str(len(body))+'\r\n\r\n').encode()+body)
 buf=b''
 while b'\r\n\r\n' not in buf:buf+=sock.recv(8192)
 head,b=buf.split(b'\r\n\r\n',1)
 while True:
  v=sock.recv(8192)
  if not v:break
  b+=v
 sock.close()
 if isinstance(b,bytes):
  out=b''
  while len(b)>=8 and b[0] in (0,1,2) and b[1:4]==b'\0\0\0':
   size=int.from_bytes(b[4:8],'big');out+=b[8:8+size];b=b[8+size:]
  result=(out+b).decode(errors='replace')
 else:result=b
 return {'exit':api('GET','/exec/'+e['Id']+'/json')['ExitCode'],'output':result}
