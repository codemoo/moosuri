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
for flag in ['--network=none','--log-driver=none','--read-only','--cap-drop=ALL','--security-opt=no-new-privileges','--pull=never','--pids-limit=32','--user=0:0','--ipc=none']:
 assert flag in a,flag
with (root/'launches').open('a') as f:f.write('run\n')
index=a.index('internal-claude-run');mode=a[index+1]
if mode=='login':
 behavior=(root/'behavior').read_text() if (root/'behavior').exists() else 'ok'
 if behavior.startswith('web-login'):
  assert '-i' in a and '-it' not in a
  origin='https://untrusted.example' if behavior=='web-login-untrusted' else 'https://claude.com'
  print(origin+'/cai/oauth/authorize?state=synthetic&code_challenge=synthetic',flush=True)
  print('Paste code here if prompted > ',end='',flush=True)
  code=sys.stdin.readline().rstrip('\n')
  if code!='synthetic-code#state':sys.exit(2)
  print('Login successful',flush=True)
  sys.exit(2 if behavior=='web-login-nonzero' else 0)
 (root/'entered').write_text('entered');time.sleep(120);sys.exit(2)
assert mode=='text'
model,session=a[index+2:index+4]
bridge=next(item.split('src=',1)[1].split(',dst=',1)[0] for item in a if 'dst=/bridge' in item)
settings=json.loads((pathlib.Path(bridge)/'settings.json').read_text())
assert settings['disableAllHooks'] is True and settings['autoMemoryEnabled'] is False
with (root/'settings-history').open('a') as f:f.write(json.dumps(settings)+'\n')
v=json.loads(sys.stdin.buffer.readline())
behavior=(root/'behavior').read_text() if (root/'behavior').exists() else 'ok'
if v.get('type')=='control_request':
 assert model=='default' and v['request']=={'subtype':'initialize'}
 print(json.dumps({'type':'system','subtype':'ui_invalidate'}),flush=True)
 print(json.dumps({'type':'control_response','response':{'subtype':'success','request_id':v['request_id'],'response':{'account':{} if behavior=='logged-out' else {'subscriptionType':'pro','apiProvider':'firstParty'},'models':[] if behavior=='catalog-empty' else [{'value':alias,'resolvedModel':concrete, 'supportsEffort': alias!='haiku' and behavior!='effort-withdrawn', 'supportedEffortLevels': ['low','medium','high','xhigh','max'] if alias!='haiku' else []} for alias,concrete in [('default','claude-fixture-v1'),('opus','claude-fixture-v1'),('sonnet','claude-fixture-sonnet'),('haiku','claude-fixture-haiku'),('fable','claude-fixture-fable')]]}}}),flush=True)
 next_line=sys.stdin.buffer.readline()
 if not next_line:sys.exit(0)
 command=json.loads(next_line)
 assert command=={'type':'user','uuid':command['uuid'],'session_id':session,'parent_tool_use_id':None,'message':{'role':'user','content':[{'type':'text','text':'/usage'}]}}
 assert sys.stdin.buffer.read()==b''
 (root/'usage-command').write_text('/usage')
 def lifecycle(state):
  frame={'type':'command_lifecycle','session_id':session,'command_uuid':command['uuid'],'state':state}
  if behavior=='usage-wrong-session':frame['session_id']='other-session'
  if behavior=='usage-wrong-command':frame['command_uuid']='other-command'
  print(json.dumps(frame),flush=True)
 lifecycle('queued');lifecycle('started')
 print(json.dumps({'type':'system','subtype':'init','session_id':session,'claude_code_version':'2.1.322','model':model,'cwd':'/work','tools':[],'mcp_servers':[],'plugins':[],'skills':[]}),flush=True)
 if behavior=='usage-hold':
  (root/'entered').write_text('entered');time.sleep(120);sys.exit(2)
 report={'session':{'secret':'never-expose-session'},'rate_limits':{'limits':[{'kind':'session','group':'session','percent':25,'resets_at':'2026-11-18T20:00:00Z','scope':None},{'kind':'weekly_scoped','group':'weekly','percent':70,'resets_at':None,'scope':{'model':{'display_name':'Opus'}}}],'extra_usage':{'secret':'never-expose-credits'}}}
 if behavior=='usage-null':report['rate_limits']=None
 if behavior=='usage-empty':report['rate_limits']['limits']=[]
 if behavior=='usage-malformed':report['rate_limits']['limits'][0]['percent']=101
 message={'type':'assistant','session_id':session,'parent_tool_use_id':None,'message':{'role':'assistant','content':[{'type':'text','text':'never-expose-free-output'}]}}
 if behavior!='usage-unsupported':message['usage_report']=report
 print(json.dumps(message),flush=True)
 print(json.dumps({'type':'result','session_id':session,'subtype':'success','is_error':False,'result':'never-expose-result'}),flush=True)
 lifecycle('completed')
 if behavior=='usage-duplicate-ack':lifecycle('completed')
 if behavior=='usage-after-result':print(json.dumps(message),flush=True)
 sys.exit(2 if behavior=='usage-nonzero' else 0)
assert v['client_composed'] is True and v['parent_tool_use_id'] is None
assert v['message']['role']=='user' and v['session_id']==session
behavior=(root/'behavior').read_text() if (root/'behavior').exists() else 'ok'
if behavior=='identity-mismatch':model='claude-not-requested'
if behavior=='hold':
 (root/'entered').write_text('entered');time.sleep(120);sys.exit(2)
def emit(value):
 value['session_id']=session;print(json.dumps(value,ensure_ascii=False),flush=True)
emit({'type':'system','subtype':'init','claude_code_version':'2.1.322','model':model,'cwd':'/work','tools':[],'mcp_servers':[],'plugins':[],'skills':[]})
def event(value):emit({'type':'stream_event','parent_tool_use_id':None,'event':value})
event({'type':'message_start','message':{'id':'m','role':'assistant','model':model}})
event({'type':'content_block_start','index':0,'content_block':{'type':'text','text':''}})
event({'type':'content_block_delta','index':0,'delta':{'type':'text_delta','text':'OK'}})
emit({'type':'assistant','parent_tool_use_id':None,'message':{'id':'m','role':'assistant','model':model,'content':[{'type':'text','text':'OK'}]}})
event({'type':'content_block_stop','index':0})
event({'type':'message_delta','delta':{'stop_reason':'end_turn'}})
event({'type':'message_stop'})
if behavior=='truncated':sys.exit(2)
emit({'type':'result','subtype':'success','is_error':False,'result':'OK','stop_reason':'end_turn','user_message_uuid':v['uuid'],'modelUsage':{model:{'inputTokens':8,'outputTokens':2,'cacheReadInputTokens':0,'cacheCreationInputTokens':0}}})
if behavior=='suffix':print('{bad json',flush=True)
if behavior=='nonzero' or behavior=='partial' and model=='claude-fixture-sonnet':sys.exit(2)
