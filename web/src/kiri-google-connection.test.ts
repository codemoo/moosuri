// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import {connectInKiri} from "./kiri-google-connection";
import {postVaultForm} from "./VaultLaunch";

vi.mock("./VaultLaunch",()=>({postVaultForm:vi.fn()}));
const origin="https://vault.example.test";
afterEach(()=>{vi.useRealTimers();vi.restoreAllMocks();vi.clearAllMocks()});
function setup(states:string[], options:{blocked?:boolean;wrongOrigin?:boolean}={}) {
  vi.useFakeTimers();
  const popup={closed:false,close(){this.closed=true}};
  vi.spyOn(window,"open").mockReturnValue(options.blocked?null:popup as unknown as Window);
  const ops:string[]=[];
  const request=vi.fn(async (path:string,_method?:string,body?:unknown)=>{
    const data=body as {op:string;job:string};
    if(path.endsWith("handoff"))return {token:"a".repeat(64),vault_origin:options.wrongOrigin?"https://other.test":origin};
    ops.push(data.op);
    return {job:data.job,challenge:crypto.randomUUID(),state:data.op==="deliver"?"signing_in":states.shift(),flow:"b".repeat(64),handle:"a".repeat(64)};
  });
  function start(){return connectInKiri(request as Parameters<typeof connectInKiri>[0],"owner","member@example.test","gmail",origin,undefined,new AbortController().signal,vi.fn())}
  return {ops,popup,start};
}
it("delivers only after the vault responds and returns the captured consent receipt",async()=>{
  const {start,ops}=setup(["vault","consent","complete"]);
  const result=start();await vi.advanceTimersByTimeAsync(0);
  expect(ops).toEqual(["start"]);
  const handoff=vi.mocked(postVaultForm).mock.calls[0][5]!;
  await handoff.receive({key:"sealed",iv:"nonce",body:"ciphertext"});
  await vi.advanceTimersByTimeAsync(2700);
  expect(await result).toEqual({handle:"a".repeat(64),flow:"b".repeat(64)});
  expect(ops).toEqual(["start","deliver","status","status"]);
});
it("does not launch a remote login if the secure popup cannot open",async()=>{
  const {start,ops}=setup([],{blocked:true});await expect(start()).rejects.toThrow("새 창");expect(ops).toEqual([]);
});
it("cancels the remote job if the registered vault origin changes",async()=>{
  const {start,ops}=setup(["vault"],{wrongOrigin:true});await expect(start()).rejects.toThrow();
  expect(postVaultForm).not.toHaveBeenCalled();expect(ops).toEqual(["start","cancel"]);
});
it("cancels when the member closes the vault before credentials are delivered",async()=>{
  const {start,ops,popup}=setup(["opening","vault"]);const outcome=expect(start()).rejects.toThrow("취소");
  await vi.advanceTimersByTimeAsync(0);popup.close();await vi.advanceTimersByTimeAsync(1400);await outcome;
  expect(ops).toContain("cancel");expect(postVaultForm).not.toHaveBeenCalled();
});
it.each(["setup_required","needs_human","blocked"])("never reports a grant on %s",async state=>{
  const {start}=setup([state]);await expect(start()).rejects.toThrow();expect(postVaultForm).not.toHaveBeenCalled();
});
