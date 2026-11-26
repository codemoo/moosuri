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
function fixture(mode,html,account=email,seg='fixture'){
 const path=mode==='list'?'':mode==='detail'?'/'+seg:'/create';
 const dom=new JSDOM(`<body><div id="toolbar"></div><main>${html}</main></body>`,{url:`https://console.cloud.google.com/auth/clients${path}?project=${project}&hl=en`,runScripts:'outside-only'});
 const w=dom.window;Object.defineProperty(w.document,'readyState',{get:()=> 'complete'});
 Object.defineProperty(w.HTMLElement.prototype,'innerText',{get(){return this.textContent},configurable:true});
 w.Element.prototype.getClientRects=function(){return [{width:1,height:1}]};
 w.document.getElementById('toolbar').attachShadow({mode:'open'}).innerHTML=`<button aria-label="Account: Synthetic (${account})"></button>`;
 const run=w.eval(`(${script})`);
 return {w,run:(last='',expected='')=>run(mode,project,email,last,expected,name,redirect,mode==='detail'?seg:''),close:()=>w.close()};
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
test('completed Console progress-button placeholder does not fence a loaded list',()=>{
 const f=fixture('list',table(row())+'<cfc-progress-button><div role="progressbar" aria-hidden="true" aria-label="Loading" style="height:0"></div></cfc-progress-button>');
 assert.equal(f.run(),'link:fixture');
 f.w.document.querySelector('[role=progressbar]').removeAttribute('aria-hidden');
 assert.equal(f.run(),'waiting');f.close();
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
// Markup shaped like the observed Console list: checkbox TD ahead of the TH headers,
// checkbox TD + rowheader name link per body row, and an empty-text icon link.
const href=`/auth/clients/${id}?project=${project}`;
const obsRow=(label=name,{icon='name',link=href,iconHref=href}={})=>`<tr role="row"><td role="gridcell" class="cfc-table-select-all-checkbox-column"><input type="checkbox" aria-label="Select row"></td><td role="rowheader"><a href="${link}">${label}</a>${icon==='name'?`<a href="${iconHref}"><i></i></a>`:''}</td><td role="gridcell">Nov 27, 2026</td><td role="gridcell">Web application</td><td role="gridcell">1234567890-syn…</td><td role="gridcell">${icon==='actions'?`<a href="${iconHref}"><i></i></a>`:''}<button>Delete</button></td></tr>`;
const obsTable=(rows,head=true)=>`<table role="grid"><thead><tr>${head?'<td role="gridcell" class="cfc-table-select-all-checkbox-header cfc-table-select-all-checkbox-column"><input type="checkbox" aria-label="Select all"></td>':''}<th role="columnheader">Name</th><th role="columnheader">Creation date</th><th role="columnheader">Type</th><th role="columnheader">Client ID</th><th role="columnheader">Actions</th></tr></thead><tbody>${rows}</tbody></table><a href="#">Create client</a>`;
test('observed list markup: header checkbox does not make the header a row or shift Name',()=>{
 for(const [html,result] of [
  [obsTable(obsRow()),'link:'+id],
  [obsTable(obsRow(name,{icon:'actions'})),'link:'+id],
  [obsTable(obsRow()+obsRow()),'client_duplicate'],
  [obsTable(obsRow(name+' x')),'client_mismatch'],
  [obsTable(obsRow(name,{iconHref:`/auth/clients/other.apps.googleusercontent.com?project=${project}`})),'client_mismatch'],
  [obsTable(obsRow(name,{iconHref:'https://evil.example/auth/clients/x'})),'link:'+id],
  [obsTable(obsRow('Moosuri 00000000000000000000000000000000')),'list_none'],
  [obsTable(obsRow('Moosuri 00000000000000000000000000000000')+obsRow()),'link:'+id],
  [obsTable(obsRow().replace('<td role="gridcell">Nov 27, 2026</td>','')),'waiting'],
  [obsTable(obsRow()+'<tr role="row"><td role="gridcell"></td><td role="rowheader"></td><td></td><td></td><td></td><td></td></tr>'),'waiting'],
 ]){
  const f=fixture('list',html);let clicks=0;f.w.document.addEventListener('click',()=>clicks++);assert.equal(f.run(),result,html);assert.equal(clicks,0);f.close();
 }
});
test('observed list: zero rows are not proven by a header with a checkbox alone',()=>{
 const f=fixture('list',obsTable(''));assert.equal(f.run(),'waiting');f.close();
});
test('observed list: two rows of the same icon link are one client but still paging/filters refuse',()=>{
 for(const extra of ['<button aria-label="Next page">Next</button>','<input aria-label="Filter" value="x">']){
  const f=fixture('list',obsTable(obsRow())+extra);assert.equal(f.run(),'unsupported');f.close();
 }
});
const obsDetail=({title='Client ID for Web application',nm=name,uri=redirect,checked=true,origin='',extraUri=false,cid=id}={})=>`<h1>${title}</h1><label for="n">Name</label><input id="n" value="${nm}"><p>The name of your OAuth 2.0 client. Not shown to end users.</p><h2>Authorized JavaScript origins</h2><p>For use with requests from a browser</p>${origin?`<label for="o">URIs 1</label><input id="o" value="${origin}">`:''}<button>Add URI</button><h2>Authorized redirect URIs</h2><p>For use with requests from a web server</p><label for="u">URIs 1</label><input id="u" value="${uri}">${extraUri?'<label for="u2">URIs 2</label><input id="u2" value="">':''}<button>Add URI</button><label><input type="checkbox" ${checked?'checked':''}>Use this client for an AI-powered agent</label><h2>Additional information</h2><label>Client ID</label><span>${cid}</span><div>Creation date</div><div>Nov 27, 2026</div><div>Last used date</div><div>Client secrets</div><p>Secrets are masked.</p><div>Client secret</div><div>****abcd</div><div>Status</div><div>Enabled</div><button>Add secret</button><button>Save</button><button>Cancel</button>`;
test('observed detail markup binds title, label-associated name, uri, agent and no origins',()=>{
 for(const [html,result] of [
  [obsDetail(),'match:'+id],
  [obsDetail({title:'Client ID for Desktop app'}),'client_mismatch'],
  [obsDetail({nm:'Moosuri short'}),'client_mismatch'],
  [obsDetail({uri:redirect+'/x'}),'client_mismatch'],
  [obsDetail({checked:false}),'client_mismatch'],
  [obsDetail({origin:'https://assistant.example.test'}),'client_mismatch'],
  [obsDetail({extraUri:true}),'client_mismatch'],
  [obsDetail()+'<h1>Client ID for Web application</h1>','ambiguous'],
 ]){
  const f=fixture('detail',html,email,id);let clicks=0;f.w.document.addEventListener('click',()=>clicks++);
  assert.equal(f.run(),result,html);assert.equal(clicks,0);f.close();
 }
 let f=fixture('detail',obsDetail(),email,'9-other.apps.googleusercontent.com');assert.equal(f.run(),'client_mismatch');f.close();
 f=fixture('detail',obsDetail(),'foreign@example.test',id);assert.equal(f.run(),'wrong_account');f.close();
});
test('observed detail never returns the secret text',()=>{
 const f=fixture('detail',obsDetail(),email,id);assert.ok(!f.run().includes('abcd'));f.close();
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
