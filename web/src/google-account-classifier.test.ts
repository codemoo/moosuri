// @vitest-environment jsdom
import {readFileSync} from "node:fs";
import {resolve} from "node:path";
import {expect, it, vi} from "vitest";

// Exercise the actual fixed script embedded in the private Rust worker. These
// are synthetic pages; no provider or credential is accessed by this suite.
const source=readFileSync(resolve(__dirname,"../../crates/moosuri-connectors/src/browser/google_auth/account.js"),"utf8");
function fixture(html:string,origin="https://accounts.google.com") {
  const w=window;
  w.document.body.innerHTML=html;
  vi.spyOn(w.document,"readyState","get").mockReturnValue("complete");
  vi.spyOn(HTMLElement.prototype,"getClientRects").mockImplementation(function(this:HTMLElement){return (this.hidden?[]:[{}]) as unknown as DOMRectList});
  let clicks=0;const events:string[]=[];
  for(const b of w.document.querySelectorAll("button"))b.addEventListener("click",e=>{e.preventDefault();clicks++});
  for(const e of w.document.querySelectorAll("input"))for(const kind of ["input","change"])e.addEventListener(kind,()=>events.push(kind));
  const classify=new Function("location","document","getComputedStyle","HTMLInputElement","Event","return ("+source+")")(new URL(origin),document,getComputedStyle,HTMLInputElement,Event) as (op:string,email:string,secret:string|null)=>string;
  return {w,events,get clicks(){return clicks},call:(op="observe")=>classify(op,"selected@example.test",op==="password"?"synthetic-password":null),close:()=>{vi.restoreAllMocks();document.body.innerHTML=""}};
}

it.each(["text","email"])("submits the exact Google %s identifier and leaves unrelated text untouched",type=>{
  const f=fixture(`<input type="text" id="recovery"><input type="${type}" id="identifierId"><button id="identifierNext">Next</button>`);
  try {
    expect(f.call()).toBe("identifier");expect(f.clicks).toBe(0);
    expect(f.call("identifier")).toBe("submitted");expect(f.clicks).toBe(1);
    expect(f.w.document.querySelector<HTMLInputElement>("#identifierId")?.value).toBe("selected@example.test");
    expect(f.w.document.querySelector<HTMLInputElement>("#recovery")?.value).toBe("");
    expect(f.events).toEqual(["input","change"]);
  } finally {f.close()}
});

it.each([
  '<input type="text" id="verificationCode">',
  '<input type="text" id="identifierId" hidden>',
  '<input type="text" id="identifierId" disabled>',
  '<input type="text" id="identifierId" readonly>',
  '<input type="text" id="identifierId" style="visibility:hidden">',
  '<input type="text" id="identifierId"><input type="email">',
])("does not submit an unrelated, inaccessible or ambiguous identifier: %s",fields=>{
  const f=fixture(fields+'<button id="identifierNext">Next</button>');
  try {expect(f.call("identifier")).toBe("unknown");expect(f.clicks).toBe(0);expect(f.events).toEqual([])} finally {f.close()}
});

it.each(["https://accounts.google.com.attacker.test","https://example.test"])("refuses identifier delivery outside the exact provider origin: %s",origin=>{
  const f=fixture('<input type="text" id="identifierId"><button id="identifierNext">Next</button>',origin);
  try {expect(f.call("identifier")).toBe("rejected");expect(f.clicks).toBe(0);expect(f.events).toEqual([])} finally {f.close()}
});

it.each(["", "hidden", "disabled"])("does not fill when the next action is absent or unavailable: %s",attribute=>{
  const f=fixture('<input type="text" id="identifierId">'+(attribute?`<button id="identifierNext" ${attribute}>Next</button>`:""));
  try {expect(f.call("identifier")).toBe("unknown");expect(f.clicks).toBe(0);expect(f.events).toEqual([])} finally {f.close()}
});

it.each([["selected@example.test","submitted",1],["other@example.test","mismatch",0]])("binds password delivery to the visible selected account: %s",(email,state,clicks)=>{
  const f=fixture(`<input type="text" id="identifierId" hidden><div id="profileIdentifier">${email}</div><input type="password"><button id="passwordNext">Next</button>`);
  try {expect(f.call("password")).toBe(state);expect(f.clicks).toBe(clicks);expect(f.events).toHaveLength(Number(clicks)*2)} finally {f.close()}
});
