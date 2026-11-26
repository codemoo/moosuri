// Synthetic DOM journeys for the fixed Rust Console adapter; no provider calls.
// Run on Linux after `cd web && npm ci`: node audit/google-console-dom/run.mjs
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";
import assert from "node:assert/strict";
import { test } from "node:test";
const require = createRequire(new URL("../../web/package.json", import.meta.url));
const { JSDOM } = require("jsdom");
const source = readFileSync(new URL("../../crates/moosuri-connectors/src/browser/google_auth/console.rs", import.meta.url), "utf8");
const script = source.split('const SCRIPT: &str = r##"')[1].split('"##;')[0];
const scopes = ["gmail.readonly","drive.metadata.readonly","documents.readonly","spreadsheets.readonly","presentations.readonly","userinfo.email"].map(s=>"https://www.googleapis.com/auth/"+s).concat("openid");
const email = "member@example.test", project="moosuri-0123456789abcdef0123";
const origin = "https://assistant.example.test", host = "assistant.example.test";
function fixture(mode, html, account=email) {
  // The public-information operations drive the Branding / Audience pages.
  const page = {domains:"branding", publication:"audience"}[mode] || mode;
  const dom = new JSDOM(`<body><div id="toolbar"></div><main>${html}</main></body>`, {url:`https://console.cloud.google.com/auth/${page}?project=${project}&hl=en`,runScripts:"outside-only"});
  const w=dom.window;
  Object.defineProperty(w.document,"readyState",{get:()=>"complete"});
  Object.defineProperty(w.HTMLElement.prototype,"innerText",{get(){return this.textContent},configurable:true});
  w.Element.prototype.getClientRects=function(){return [{width:1,height:1}]};
  const shadow=w.document.getElementById("toolbar").attachShadow({mode:"open"});
  shadow.innerHTML=`<button aria-label="Account: Synthetic (${account})"></button><aside role="complementary">Accessibility Help</aside>`;
  const run=w.eval(`(${script})`);
  return {w, run:(last="",expected="")=>run(mode,project,email,last,scopes,expected,"Moosuri",origin,host),close:()=>w.close()};
}
const branding='<label for="app">App name</label><input id="app" value="Moosuri"><label id="support">User support email</label><div role="combobox" aria-labelledby="support">'+email+'</div><mat-form-field><mat-chip-row>'+email+'</mat-chip-row><input aria-label="Text field for emails"></mat-form-field>';
const audience=(users=email,type="External")=>`Publishing status\nTesting\nUser type\n${type}\n<table><tr><th>User information</th></tr>${users?`<tr><td>${users}</td></tr>`:""}</table><button>Add users</button>`;
function scopeTable(values){return '<button>Add or remove scopes</button><table><tr><th>API</th><th>Scope</th><th>User-facing description</th><th></th></tr>'+values.map(v=>`<tr><td></td><td>${v}</td><td>Description</td><td></td></tr>`).join('')+'</table>'}
test('persisted editable branding and shadow account toolbar read back without writes',()=>{
 const f=fixture('branding',branding);assert.equal(f.run(),'branding_match');assert.equal(f.w.document.getElementById('app').value,'Moosuri');f.close();
});
test('foreign account or project cannot be accepted',()=>{
 const f=fixture('branding',branding,'other@example.test');assert.equal(f.run(),'wrong_account');f.w.history.replaceState(null,'','?project=other');assert.equal(f.run(),'foreign');f.close();
});
test('brand or contact mismatch cannot be overwritten',()=>{
 const f=fixture('branding',branding.replace('value="Moosuri"','value="Other"'));assert.equal(f.run(),'branding_mismatch');assert.equal(f.run('','app_name'),'branding_mismatch');f.close();
});
test('External Testing audience requires the initiating account in the user table',()=>{
 const f=fixture('audience',audience());assert.equal(f.run(),'audience_match');f.close();
 const absent=fixture('audience',audience(''));assert.equal(absent.run(),'audience_partial');absent.close();
 const internal=fixture('audience',audience(email,'Internal'));assert.equal(internal.run(),'audience_mismatch');internal.close();
});
test('abbreviated Console scopes are normalized, but every extra scope is refused',()=>{
 const shortened=scopes.map(s=>s.replace('https://www.googleapis.com/auth/','.../auth/'));
 const f=fixture('scopes',scopeTable(shortened));assert.equal(f.run(),'scopes_match');f.close();
 for(const extra of ['.../auth/gmail.modify','https://foreign.example/scope','unrecognized.scope']){
  const bad=fixture('scopes',scopeTable([...shortened,extra]));assert.equal(bad.run(),'scopes_extra');bad.close();
 }
 const partial=fixture('scopes',scopeTable(shortened.slice(1)));assert.equal(partial.run(),'scopes_partial');partial.close();
});
// Synthetic fixtures of the OBSERVED branding labels; not a live Google result.
const labelled=(label,id,value='')=>`<label for="${id}">${label}</label><input id="${id}" value="${value}">`;
const domainsPage=({home='',priv='',terms='',domains=[host],saveDisabled=true}={})=>branding
 +labelled('Application home page','h',home)+labelled('Application privacy policy link','p',priv)
 +labelled('Application terms of service link','t',terms)
 +domains.map((d,i)=>`<label for="d${i}">Authorized domain ${i+1} </label><input id="d${i}" placeholder="example.com" value="${d}">`).join('')
 // Observed: the button's disabled property is false; only aria-disabled is true until dirty.
 +`<button id="save" aria-disabled="${saveDisabled}">Save</button>`;
const exact=[origin+'/about',origin+'/privacy',origin+'/terms'];
const values=f=>['h','p','t'].map(id=>f.w.document.getElementById(id).value);
test('blank public URLs are filled with the exact callback origin, then saved only once the form is dirty',()=>{
 const f=fixture('domains',domainsPage());
 assert.equal(f.run(),'domains_fill');assert.deepEqual(values(f),['','','']);
 assert.equal(f.run('','domains_save'),'changed');assert.deepEqual(values(f),['','','']);
 assert.equal(f.run('','domains_fill'),'acted');assert.deepEqual(values(f),exact);
 // Save is aria-disabled until the framework marks the form dirty: wait, never click.
 let clicks=0;f.w.document.getElementById('save').addEventListener('click',()=>clicks++);
 assert.equal(f.run('domains_fill'),'waiting');assert.equal(f.run('domains_fill','domains_save'),'changed');assert.equal(clicks,0);
 f.w.document.getElementById('save').setAttribute('aria-disabled','false');
 assert.equal(f.run('domains_fill'),'domains_save');assert.equal(f.run('domains_fill','domains_save'),'acted');assert.equal(clicks,1);
 f.close();
 // A fresh persisted page reads back as a match; a dirty unsaved one is not trusted.
 const saved=fixture('domains',domainsPage({home:exact[0],priv:exact[1],terms:exact[2]}));
 assert.equal(saved.run(),'domains_match');assert.equal(saved.run('domains_fill'),'waiting');saved.close();
 // Only blank fields are filled; an already exact one is left alone.
 const part=fixture('domains',domainsPage({home:exact[0]}));
 assert.equal(part.run(),'domains_fill');assert.equal(part.run('','domains_fill'),'acted');assert.deepEqual(values(part),exact);part.close();
});
test('a differing URL, other branding or another account is never overwritten',()=>{
 for(const page of [domainsPage({priv:'https://other.example/privacy'}),domainsPage({home:exact[0]+'/',priv:exact[1]}),
   domainsPage({home:exact[0],terms:'http://assistant.example.test/terms'})]){
  const f=fixture('domains',page);const before=values(f);
  assert.equal(f.run(),'domains_mismatch');assert.equal(f.run('','domains_fill'),'domains_mismatch');assert.deepEqual(values(f),before);f.close();
 }
 const other=fixture('domains',domainsPage().replace('value="Moosuri"','value="Other"'));assert.equal(other.run('','domains_fill'),'branding_mismatch');assert.deepEqual(values(other),['','','']);other.close();
 const wrong=fixture('domains',domainsPage(),'other@example.test');assert.equal(wrong.run('','domains_fill'),'wrong_account');assert.deepEqual(values(wrong),['','','']);wrong.close();
});
test('an authorized domain must cover the hostname on a DNS label boundary or nothing is written',()=>{
 for(const [domains,verdict] of [[[host],'domains_fill'],[['example.test'],'domains_fill'],[['other.test',host],'domains_fill'],
   [['ample.test'],'domain_unproven'],[['ssistant.example.test'],'domain_unproven'],[['example.test.evil.test'],'domain_unproven'],
   [['test'],'domain_unproven'],[['*.example.test'],'domain_unproven'],[['other.test'],'domain_unproven'],[[],'domain_unproven']]){
  const f=fixture('domains',domainsPage({domains}));
  assert.equal(f.run(),verdict,domains.join());
  if(verdict==='domain_unproven'){assert.equal(f.run('','domains_fill'),verdict);assert.deepEqual(values(f),['','','']);}
  f.close();
 }
});
const status=(value,type='External',after='')=>`Publishing status\n${value}\nUser type\n${type}\n${after}`;
test('publication reads only the exact status field and needs an enabled Publish app action',()=>{
 const prod=fixture('publication',status('In production'));assert.equal(prod.run(),'publication_match');prod.close();
 // Banner text elsewhere is never evidence.
 const banner=fixture('publication',status('Testing','External','<p>In production</p><button>Publish app</button>'));assert.equal(banner.run(),'publish_open');banner.close();
 const none=fixture('publication',status('Testing')+'<p>Your app is in production</p>');assert.equal(none.run(),'unsupported');none.close();
 const off=fixture('publication',status('Testing','External','<button aria-disabled="true">Publish app</button><p>To publish your app, you must complete your configuration on the Branding page.</p>'));
 assert.equal(off.run(),'publish_blocked');assert.equal(off.run('','publish_open'),'publish_blocked');off.close();
 const internal=fixture('publication',status('Testing','Internal','<button>Publish app</button>'));assert.equal(internal.run(),'publication_mismatch');internal.close();
 const odd=fixture('publication',status('Pending verification','External','<button>Publish app</button>'));assert.equal(odd.run(),'unsupported');odd.close();
 // The old audience stage accepts a published app and never tries to add a test user.
 const aud=fixture('audience',status('In production'));assert.equal(aud.run(),'audience_match');assert.equal(aud.run('','audience_partial'),'audience_match');aud.close();
});
test('only the exact Push to production confirmation opened by our click is confirmed',()=>{
 const f=fixture('publication',status('Testing','External','<button id="pub">Publish app</button>'));
 let published=0;f.w.document.getElementById('pub').addEventListener('click',()=>published++);
 assert.equal(f.run(),'publish_open');assert.equal(f.run('','publish_confirm'),'changed');assert.equal(f.run('','publish_open'),'acted');assert.equal(published,1);
 // After our click and before a dialog: wait, never click Publish again.
 assert.equal(f.run('publish_open'),'waiting');assert.equal(f.run('publish_open','publish_open'),'changed');assert.equal(published,1);
 const main=f.w.document.querySelector('main');let confirms=0;
 main.insertAdjacentHTML('beforeend','<div role="dialog">Push to production?<button>Cancel</button><button id="ok">Confirm</button></div>');
 f.w.document.getElementById('ok').addEventListener('click',()=>confirms++);
 assert.equal(f.run('publish_open'),'publish_confirm');assert.equal(f.run('publish_open','publish_confirm'),'acted');assert.equal(confirms,1);
 // The same dialog without our own preceding click is not adopted.
 assert.equal(f.run(''),'unsupported');assert.equal(f.run('','publish_confirm'),'unsupported');assert.equal(confirms,1);
 f.close();
 for(const dialog of ['<div role="dialog">Delete project?<button>Cancel</button><button>Confirm</button></div>',
   '<div role="dialog">Push to production?<button>Cancel</button><button>Confirm</button><button>Delete everything</button></div>',
   '<div role="dialog">Push to production?<label><input type="checkbox">I agree</label><button>Confirm</button></div>',
   '<div role="dialog">Push to production?<button>Cancel</button><button>Confirm</button><button>Confirm</button></div>',
   '<div role="dialog">Push to production?<button>Cancel</button></div>']){
  const g=fixture('publication',status('Testing')+dialog);let n=0;
  for(const b of g.w.document.querySelectorAll('button'))b.addEventListener('click',()=>n++);
  const seen=(()=>{try{return g.run('publish_open','publish_confirm')}catch(e){return String(e)}})();
  assert.notEqual(seen,'acted',dialog);assert.equal(n,0,dialog);g.close();
 }
});
test('audience dialog adds only the bound account and requires exact stage before save',()=>{
 const f=fixture('audience',audience('')+'<div role="dialog">Add users<input aria-label="Text field for emails"><button>Save</button></div>');
 const d=f.w.document.querySelector('[role=dialog]');let clicks=0;d.querySelector('button').addEventListener('click',()=>clicks++);
 assert.equal(f.run('audience_partial'),'audience_input');assert.equal(f.run('audience_partial','audience_input'),'acted');
 assert.equal(d.querySelector('input').value,email);assert.equal(clicks,0);
 d.insertAdjacentHTML('beforeend',`<mat-chip-row>${email}</mat-chip-row>`);
 assert.equal(f.run('audience_input','audience_save'),'acted');assert.equal(clicks,1);f.close();
});
