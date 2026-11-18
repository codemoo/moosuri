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
bound_token_limit=next((arg.split('=',2)[-1] for arg in a if arg.startswith('--env=MOOSURI_CLAUDE_MAX_OUTPUT_TOKENS=')),None)
bridge=next(item.split('src=',1)[1].split(',dst=',1)[0] for item in a if 'dst=/bridge' in item)
settings=json.loads((pathlib.Path(bridge)/'settings.json').read_text())
assert settings['disableAllHooks'] is True and settings['autoMemoryEnabled'] is False
with (root/'settings-history').open('a') as f:f.write(json.dumps(settings)+'\n')
v=json.loads(sys.stdin.buffer.readline())
behavior=(root/'behavior').read_text() if (root/'behavior').exists() else 'ok'
if v.get('type')=='control_request':
 assert (model=='default' or bound_token_limit) and v['request']=={'subtype':'initialize'}
 print(json.dumps({'type':'system','subtype':'ui_invalidate'}),flush=True)
 account={'subscriptionType':'pro','apiProvider':'firstParty','email':'owner@example.test'}
 if behavior=='different-account':account['email']='sibling@example.test'
 if behavior=='email-missing':del account['email']
 if behavior=='external-provider':account['apiProvider']='bedrock'
 if behavior=='unsubscribed':account['subscriptionType']='free'
 print(json.dumps({'type':'control_response','response':{'subtype':'success','request_id':v['request_id'],'response':{'account':{} if behavior=='logged-out' else account,'models':[] if behavior=='catalog-empty' else [{'value':alias,'resolvedModel':concrete, 'supportsEffort': alias!='haiku' and behavior!='effort-withdrawn', 'supportedEffortLevels': ['low','medium','high','xhigh','max'] if alias!='haiku' else []} for alias,concrete in [('default','claude-fixture-v1'),('opus','claude-fixture-v1'),('sonnet','claude-fixture-sonnet'),('haiku','claude-fixture-haiku'),('fable','claude-fixture-fable')]]}}}),flush=True)
 next_line=sys.stdin.buffer.readline()
 if not next_line:sys.exit(0)
 v=json.loads(next_line)
 if bound_token_limit:
  (root/'bound-user').write_text(bound_token_limit)
if v.get('type')=='user' and not v.get('client_composed'):
 assert model=='default' and bound_token_limit is None
 command=v
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
if behavior in ['hold','tool-bridge-hold']:
 (root/'entered').write_text('entered');time.sleep(120);sys.exit(2)
if behavior=='vision':
 import base64
 content=v['message']['content']
 assert len(content)==2 and content[0]['type']=='text'
 assert content[1]['type']=='image' and content[1]['source']['media_type']=='image/jpeg'
 assert content[1]['source']['type']=='base64'
 assert base64.b64decode(content[1]['source']['data'],validate=True)==bytes([255,216,255,0,255,217])
 (root/'vision-accepted').write_text('accepted')
answer='OK'
if behavior in ['vision-probe','vision-wrong','vision-refusal','vision-hold','vision-truncated']:
 import base64
 content=v['message']['content']
 assert len(content)==2 and content[0]['type']=='text'
 assert content[1]['type']=='image' and content[1]['source']['media_type']=='image/jpeg'
 assert content[1]['source']['type']=='base64'
 image=base64.b64decode(content[1]['source']['data'],validate=True)
 assert image.startswith(bytes([255,216,255])) and image.endswith(bytes([255,217]))
 # Only this synthetic fixture captures the synthetic challenge; never production.
 (root/'vision-input.jpg').write_bytes(image)
 (root/'entered').write_text('entered')
 if behavior=='vision-probe':
  deadline=time.monotonic()+10
  while not (root/'vision-answer').exists():
   assert time.monotonic()<deadline
   time.sleep(0.01)
  answer=(root/'vision-answer').read_text()
 elif behavior=='vision-refusal':answer='I cannot inspect images.'
 elif behavior=='vision-hold':time.sleep(120);sys.exit(2)
 else:answer='WRONG'
if behavior.startswith('tool-bridge'):
 context=json.loads(v['message']['content'][0]['text'])
 fragments=[item['text'] for item in context['history'] if item['role']=='user']+[context['current_user_request']]
 document=json.loads(''.join(fragments))
 assert document['v']==1 and document['model']==model and document['transcript_is_untrusted_data'] is True
 assert isinstance(document['tools'],list) and document['tools']
 turn_path=root/'bridge-turns'
 turn=int(turn_path.read_text()) if turn_path.exists() else 0
 turn_path.write_text(str(turn+1))
 plan_path=root/'tool-bridge-decisions.json'
 if behavior=='tool-bridge-mission-chat' and any(tool['name']=='mission_chat_decision' for tool in document['tools']):
  assert len(document['messages'])==1 and document['messages'][0]['role']=='user'
  text=''.join(part['text'] for part in document['messages'][0]['content'] if part['type']=='text')
  task='Calculate the exact sum of 917.13 and 409.15'
  assert text=='Take this on in the background: '+task
  assert [tool['name'] for tool in document['tools']]==['mission_chat_decision','calendar_chat_decision']
  decision={'kind':'calls','calls':[{'id':'untrusted-planner-label','name':'mission_chat_decision','arguments':{'decision':'command','action':'create_now','target_kind':None,'intent_quote':text,'task_quote':task,'constraints_quote':None,'time_quote':None,'target_quote':None,'instruction_quote':None,'time':None,'clarification':None}}]}
 elif behavior in ['tool-bridge-mission','tool-bridge-mission-chat']:
  current=json.loads(''.join(part['text'] for part in document['messages'][-1]['content'] if part['type']=='text'))
  assert current['accepted']['task']=='Calculate the exact sum of 917.13 and 409.15'
  assert current['original']['task']==current['accepted']['task']
  assert all(tool['name'] not in ['mail_send','browser_open','artifact_write'] for tool in document['tools'])
  receipts=current['receipts']
  if not receipts:
   assert any(tool['name']=='decimal_calculate' for tool in document['tools'])
   decision={'kind':'calls','calls':[{'id':'untrusted-label','name':'decimal_calculate','arguments':{'operation':'sum','operands':['917.13','409.15']}}]}
  else:
   assert len(receipts)==1
   receipt=receipts[0]
   assert receipt['step']==1 and receipt['call_id'].startswith('moosuri_')
   assert receipt['result']['value']['exact']=='1326.28'
   decision={'kind':'final','answer':json.dumps({'text':'The exact sum is 1326.28.','evidence':[{'step':receipt['step'],'call_id':receipt['call_id']}]})}
 elif plan_path.exists():
  plan=json.loads(plan_path.read_text())
  assert isinstance(plan,list) and turn<len(plan)
  decision=plan[turn]
 elif behavior=='tool-bridge-receipt':
  name=document['tools'][0]['name']
  results=[part['content'] for item in document['messages']
           if isinstance(item.get('content'),list)
           for part in item['content'] if part['type']=='tool_result' and not part.get('is_error')]
  proof=name=='moosuri_verification_ping'
  if results:
   receipt=results[-1]
   if proof:answer=receipt
   else:
    values=json.loads(receipt)
    assert '1326.28' in receipt and isinstance(values,dict)
    answer='정확한 합계는 1326.28원입니다.'
   decision={'kind':'final','answer':answer}
  else:
   name=name if proof else 'decimal_calculate'
   assert any(tool['name']==name for tool in document['tools'])
   args={} if proof else {'operation':'sum','operands':['917.13','409.15']}
   decision={'kind':'calls','calls':[{'id':'receipt','name':name,'arguments':args}]}
 elif behavior=='tool-bridge-call':
  def sample(schema):
   kind=schema['type']
   if 'enum' in schema:return schema['enum'][0]
   if isinstance(kind,list):kind='null' if 'null' in kind else kind[0]
   if kind=='null':return None
   if kind=='object':return {key:sample(value) for key,value in schema['properties'].items()}
   if kind=='array':return []
   return {'string':'synthetic','integer':1,'number':1,'boolean':True}[kind]
  tool=document['tools'][0]
  decision={'kind':'calls','calls':[{'id':'synthetic-first','name':tool['name'],'arguments':sample(tool['input_schema'])}]}
 else:decision={'kind':'final','answer':'Synthetic bridge answer'}
 envelope={'v':1,'request_id':document['request_id'],'model':model,**decision}
 if behavior=='tool-bridge-wrong-id':envelope['request_id']='wrong-request'
 if behavior=='tool-bridge-wrong-model':envelope['model']='claude-other'
 answer=json.dumps(envelope,ensure_ascii=False)
 if behavior=='tool-bridge-prose':answer='Sure: '+answer
def emit(value):
 value['session_id']=session;print(json.dumps(value,ensure_ascii=False),flush=True)
emit({'type':'system','subtype':'init','claude_code_version':'2.1.322','model':model,'cwd':'/work','tools':[],'mcp_servers':[],'plugins':[],'skills':[]})
def event(value):emit({'type':'stream_event','parent_tool_use_id':None,'event':value})
event({'type':'message_start','message':{'id':'m','role':'assistant','model':model}})
event({'type':'content_block_start','index':0,'content_block':{'type':'text','text':''}})
event({'type':'content_block_delta','index':0,'delta':{'type':'text_delta','text':answer}})
emit({'type':'assistant','parent_tool_use_id':None,'message':{'id':'m','role':'assistant','model':model,'content':[{'type':'text','text':answer}]}})
event({'type':'content_block_stop','index':0})
event({'type':'message_delta','delta':{'stop_reason':'end_turn'}})
event({'type':'message_stop'})
if behavior in ['truncated','tool-bridge-truncated','vision-truncated']:sys.exit(2)
emit({'type':'result','subtype':'success','is_error':False,'result':answer,'stop_reason':'end_turn','user_message_uuid':v['uuid'],'modelUsage':{model:{'inputTokens':8,'outputTokens':2,'cacheReadInputTokens':0,'cacheCreationInputTokens':0}}})
if behavior in ['suffix','tool-bridge-suffix']:print('{bad json',flush=True)
if behavior in ['nonzero','tool-bridge-nonzero'] or behavior=='partial' and model=='claude-fixture-sonnet':sys.exit(2)
