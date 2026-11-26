// Synthetic DOM evidence only. Run on Linux: node audit/google-console-dom/client.mjs
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import assert from 'node:assert/strict';
import { test } from 'node:test';
const require=createRequire(new URL('../../web/package.json',import.meta.url));
const {JSDOM}=require('jsdom');
const source=readFileSync(new URL('../../crates/moosuri-connectors/src/browser/google_auth/client.rs',import.meta.url),'utf8');
const script=source.split('const SCRIPT: &str = r##"')[1].split('"##;')[0];
const email='member@example.test',project='moosuri-0123456789abcdef0123',name='Moosuri 0123456789abcdef0123456789abcdef';
const redirect='https://assistant.example.test/api/v1/mail/oauth/callback',id='1234567890-synthetic.apps.googleusercontent.com';
function fixture(mode,html,account=email){
 const path=mode==='list'?'':mode==='detail'?'/fixture':'/create';
 const dom=new JSDOM(`<body><div id="toolbar"></div><main>${html}</main></body>`,{url:`https://console.cloud.google.com/auth/clients${path}?project=${project}&hl=en`,runScripts:'outside-only'});
 const w=dom.window;Object.defineProperty(w.document,'readyState',{get:()=> 'complete'});
 Object.defineProperty(w.HTMLElement.prototype,'innerText',{get(){return this.textContent},configurable:true});
 w.Element.prototype.getClientRects=function(){return [{width:1,height:1}]};
 w.document.getElementById('toolbar').attachShadow({mode:'open'}).innerHTML=`<button aria-label="Account: Synthetic (${account})"></button>`;
 const run=w.eval(`(${script})`);
 return {w,run:(last='',expected='')=>run(mode,project,email,last,expected,name,redirect,mode==='detail'?'fixture':''),close:()=>w.close()};
}
const row=(label=name)=>`<tr><td><a href="/auth/clients/fixture?project=${project}">${label}</a></td></tr>`;
const table=(rows='')=>`<table><tr><th>Name</th></tr>${rows || '<tr><td>No rows to display</td></tr>'}</table><button>Create client</button>`;
test('client list is read only and refuses duplicate or mismatching stable identity',()=>{
 for(const [html,result] of [[table(),'list_none'],[table(row()),'link:fixture'],[table(row()+row()),'client_duplicate'],[table(row(name+' changed')),'client_mismatch']]){
  const f=fixture('list',html);let clicks=0;f.w.document.addEventListener('click',()=>clicks++);assert.equal(f.run(),result);assert.equal(clicks,0);f.close();
 }
});
test('unseen pages, filters and loading do not prove a client absent',()=>{
 for(const [extra,result] of [['<button aria-label="Next page">Next</button>','unsupported'],['<span class="mat-mdc-paginator-range-label">1 – 10 of 21</span>','unsupported'],['<input aria-label="Filter" value="another">','unsupported'],['<div role="progressbar"></div>','waiting']]){
  const f=fixture('list',table()+extra);assert.equal(f.run(),result);f.close();
 }
});
test('blank tables and skeleton rows do not prove absence',()=>{
 for(const html of ['<table><tr><th>Name</th></tr></table><button>Create client</button>',table('<tr><td></td></tr>'),'<button>Create client</button>']){
  const f=fixture('list',html);assert.equal(f.run(),'waiting');f.close();
 }
});
test('foreign account and project cannot name a reusable client',()=>{
 const f=fixture('list',table(row()),'foreign@example.test');assert.equal(f.run(),'wrong_account');f.w.history.replaceState(null,'','?project=foreign');assert.equal(f.run(),'foreign');f.close();
});
const detail=()=>`<div>Name</div><input value="${name}"><div>Application type</div><div>Web application</div><div>Client ID</div><div>${id}</div><label>Use this client for an AI-powered agent<input type="checkbox" checked></label><div>Authorized JavaScript origins</div><div>Authorized redirect URIs</div><input value="${redirect}"><div>Client secrets</div>`;
test('persisted client requires exact callback and agentic flag',()=>{
 for(const [html,result] of [[detail(),'match:'+id],[detail().replace('checked',''),'client_mismatch'],[detail().replace(redirect,redirect+'?other'),'client_mismatch']]){
  const f=fixture('detail',html);assert.equal(f.run(),result);f.close();
 }
});
test('creation dialogue exposes only its fixed synthetic fields and refuses a masked secret',()=>{
 for(const [secret,result] of [['GOCSPX-synthetic-only',`created\n${id}\nGOCSPX-synthetic-only`],['GOCSPX-****','secret_hidden']]){
  const f=fixture('result',`<div role="dialog">Accessibility help</div><div role="dialog"><div>Client ID</div><input value="${id}"><div>Client secret</div><input value="${secret}"></div>`);assert.equal(f.run('create'),result);f.close();
 }
});
test('create checks the actual state before every fixed action',()=>{
 const form=`<div role="combobox" aria-label="Application type">Web application</div><input aria-label="Name" value="${name}"><h3>Authorized JavaScript origins</h3><button>Add URI</button><h3>Authorized redirect URIs</h3><input value="${redirect}"><button>Add URI</button><label>Use this client for an AI-powered agent<input type="checkbox" checked></label><button>Create</button>`;
 const f=fixture('create',form);let clicks=0;f.w.document.addEventListener('click',e=>{if(e.target.textContent==='Create')clicks++});
 assert.equal(f.run('agent_check'),'create');assert.equal(f.run('agent_check','name_fill'),'changed');assert.equal(clicks,0);
 assert.equal(f.run('agent_check','create'),'acted');assert.equal(clicks,1);f.close();
});
