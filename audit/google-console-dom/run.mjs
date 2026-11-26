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
function fixture(mode, html, account=email) {
  const dom = new JSDOM(`<body><div id="toolbar"></div><main>${html}</main></body>`, {url:`https://console.cloud.google.com/auth/${mode}?project=${project}&hl=en`,runScripts:"outside-only"});
  const w=dom.window;
  Object.defineProperty(w.document,"readyState",{get:()=>"complete"});
  Object.defineProperty(w.HTMLElement.prototype,"innerText",{get(){return this.textContent},configurable:true});
  w.Element.prototype.getClientRects=function(){return [{width:1,height:1}]};
  const shadow=w.document.getElementById("toolbar").attachShadow({mode:"open"});
  shadow.innerHTML=`<button aria-label="Account: Synthetic (${account})"></button><aside role="complementary">Accessibility Help</aside>`;
  const run=w.eval(`(${script})`);
  return {w, run:(last="",expected="")=>run(mode,project,email,last,scopes,expected,"Moosuri"),close:()=>w.close()};
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
test('audience dialog adds only the bound account and requires exact stage before save',()=>{
 const f=fixture('audience',audience('')+'<div role="dialog">Add users<input aria-label="Text field for emails"><button>Save</button></div>');
 const d=f.w.document.querySelector('[role=dialog]');let clicks=0;d.querySelector('button').addEventListener('click',()=>clicks++);
 assert.equal(f.run('audience_partial'),'audience_input');assert.equal(f.run('audience_partial','audience_input'),'acted');
 assert.equal(d.querySelector('input').value,email);assert.equal(clicks,0);
 d.insertAdjacentHTML('beforeend',`<mat-chip-row>${email}</mat-chip-row>`);
 assert.equal(f.run('audience_input','audience_save'),'acted');assert.equal(clicks,1);f.close();
});
