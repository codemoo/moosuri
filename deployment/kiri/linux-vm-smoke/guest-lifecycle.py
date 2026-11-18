#!/usr/bin/env python3
from engine_api import *
# Dedicated synthetic guest only, enforced by engine_api's marker check.
a='lab191-socket-a';b='lab191-socket-b'
api('POST','/containers/'+a+'/stop?t=1')
assert not api('GET','/containers/'+a+'/json')['State']['Running']
assert api('GET','/containers/'+b+'/json')['State']['Running']
subprocess.run(['systemctl','restart','kiri'],check=True)
assert not api('GET','/containers/'+a+'/json')['State']['Running']
assert api('GET','/containers/'+b+'/json')['State']['Running']
emit('unless-stopped','PASS explicit A stop survives daemon restart; B resumes')
api('POST','/containers/'+a+'/start')
