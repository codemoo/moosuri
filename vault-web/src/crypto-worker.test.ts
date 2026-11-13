import {afterEach,expect,it,vi} from "vitest";
const owner={account:"00000000-0000-0000-0000-000000000001",tenant:"00000000-0000-0000-0000-000000000002",installation:"00000000-0000-0000-0000-000000000003",epoch:0};
const originalHead={header_sha256:"a".repeat(64),revision:1,envelope_epoch:1,generation:1};
async function fixture(changeInventory=false){
  vi.resetModules();vi.useFakeTimers();
  let head={...originalHead},reads=0;
  const calls:Record<string,unknown>[]=[];
  const scope={postMessage:vi.fn(),onmessage:null as null|((e:unknown)=>void)};
  vi.stubGlobal("self",scope);
  vi.stubGlobal("fetch",vi.fn(async(path:string)=>new Response(JSON.stringify(path==="/session/current"?{owner,head}:{owner,binding:null,header:{revision:1,envelope_epoch:1,generation:1},header_sha256:originalHead.header_sha256,inventory_sha256:changeInventory&&++reads>=3?"c".repeat(64):"b".repeat(64),items:[],pending:[]}))));
  vi.doMock("./wasm/vault_crypto.js",()=>({default:async()=>{},VaultEngine:class{
    rpc(raw:string){const command=JSON.parse(raw);calls.push(command);return JSON.stringify({ok:true,value:command.op==="load"?{initialized:true}:command.op==="archive_export"?"SYNTHETIC ENCRYPTED ARCHIVE":{eligible:1,rows:[{row:1,status:"new"}]}})}free(){}
  }}));
  await import("./crypto-worker");
  const flush=async()=>{for(let n=0;n<100;n++)await Promise.resolve()};
  const send=(id:number,command:Record<string,unknown>)=>scope.onmessage?.({data:{id,epoch:7,command}});
  send(1,{op:"initialize",owner});await flush();
  expect(scope.postMessage).toHaveBeenCalledWith(expect.objectContaining({id:1,ok:true}));
  return {scope,calls,send,flush,changeHead:()=>{head={...head,header_sha256:"d".repeat(64),revision:2,envelope_epoch:2}}};
}
afterEach(()=>{vi.useRealTimers();vi.unstubAllGlobals();vi.doUnmock("./wasm/vault_crypto.js")});
it("source archive file read disposed after destination generation changes",async()=>{
  const f=await fixture();let finish!:(s:string)=>void;
  const file=new File(["synthetic ciphertext"],"synthetic.encrypted.json");
  Object.defineProperty(file,"text",{value:()=>new Promise<string>(resolve=>{finish=resolve})});
  f.send(2,{op:"archive_file",file,secret:"Synthetic fixture password",recovery:false});await f.flush();
  f.changeHead();finish("synthetic ciphertext");await f.flush();
  expect(f.calls.some(c=>c.op==="archive_preview")).toBe(false);
  expect(f.scope.postMessage).toHaveBeenCalledWith({kind:"locked",reason:"vault_generation_changed"});
  expect(f.scope.postMessage).not.toHaveBeenCalledWith(expect.objectContaining({id:2,ok:true}));
});
it("export rejects inventory changed between fresh snapshot and final check",async()=>{
  const f=await fixture(true);f.send(2,{op:"archive_export"});await f.flush();
  expect(f.calls.some(c=>c.op==="archive_export")).toBe(true);
  expect(f.scope.postMessage).toHaveBeenCalledWith(expect.objectContaining({id:2,ok:false,error:"vault_conflict"}));
  expect(f.scope.postMessage).not.toHaveBeenCalledWith(expect.objectContaining({id:2,ok:true}));
});
