#!/usr/bin/python3
# Synthetic test process, never a consumer account or provider.
import sys,json,os,time,pathlib
root=pathlib.Path(FIXTURE_ROOT)
a=sys.argv[1:]
assert a[0]=='--host' and a[1].startswith('unix://')
assert os.environ.get('DOCKER_CONFIG')==str(root/'scratch'/'engine-empty')
assert 'ANTHROPIC_API_KEY' not in os.environ and 'CLAUDE_CODE_OAUTH_TOKEN' not in os.environ
if 'info' in a:
 print('["name=rootless"]');sys.exit(0)
if 'ps' in a:
 assert '--no-trunc' in a
 if (root/'stale').exists():print('a'*64)
 sys.exit(0)
if 'rm' in a:
 if (root/'behavior').exists() and (root/'behavior').read_text()=='login-hold':
  (root/'remove-entered').write_text('entered')
  deadline=time.monotonic()+5
  while not (root/'allow-remove').exists():
   assert time.monotonic()<deadline
   time.sleep(0.01)
 with (root/'removed').open('a') as f:f.write('removed\n')
 sys.exit(0)
assert 'run' in a
for flag in ['--network=none','--read-only','--cap-drop=ALL','--security-opt=no-new-privileges','--pull=never','--pids-limit=32','--user=0:0','--ipc=none']:
 assert flag in a,flag
with (root/'launches').open('a') as f:f.write('run\n')
index=a.index('internal-claude-run');mode=a[index+1]
if mode=='login':
 (root/'entered').write_text('entered');time.sleep(120);sys.exit(2)
assert mode=='text'
model,session=a[index+2:index+4]
v=json.loads(sys.stdin.buffer.read())
assert v['client_composed'] is True and v['parent_tool_use_id'] is None
assert v['message']['role']=='user' and v['session_id']==session
behavior=(root/'behavior').read_text() if (root/'behavior').exists() else 'ok'
if behavior=='hold':
 (root/'entered').write_text('entered');time.sleep(120);sys.exit(2)
def emit(value):
 value['session_id']=session;print(json.dumps(value,ensure_ascii=False),flush=True)
emit({'type':'system','subtype':'init','claude_code_version':'2.1.322','model':model,'cwd':'/work','tools':[],'mcp_servers':[],'plugins':[],'skills':[]})
def event(value):emit({'type':'stream_event','parent_tool_use_id':None,'event':value})
event({'type':'message_start','message':{'id':'m','role':'assistant','model':model}})
event({'type':'content_block_start','index':0,'content_block':{'type':'text','text':''}})
event({'type':'content_block_delta','index':0,'delta':{'type':'text_delta','text':'OK'}})
event({'type':'content_block_stop','index':0})
event({'type':'message_delta','delta':{'stop_reason':'end_turn'}})
event({'type':'message_stop'})
emit({'type':'assistant','parent_tool_use_id':None,'message':{'id':'m','role':'assistant','model':model,'content':[{'type':'text','text':'OK'}]}})
if behavior=='truncated':sys.exit(2)
emit({'type':'result','subtype':'success','is_error':False,'result':'OK','stop_reason':'end_turn','user_message_uuid':v['uuid'],'modelUsage':{model:{'inputTokens':8,'outputTokens':2,'cacheReadInputTokens':0,'cacheCreationInputTokens':0}}})
if behavior=='suffix':print('{bad json',flush=True)
if behavior=='nonzero':sys.exit(2)
