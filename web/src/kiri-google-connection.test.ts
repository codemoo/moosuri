// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import {connectInKiri} from "./kiri-google-connection";
import {postVaultForm} from "./VaultLaunch";
import {ApiFailure} from "./api";

vi.mock("./VaultLaunch",()=>({postVaultForm:vi.fn()}));
const origin="https://vault.example.test";
afterEach(()=>{vi.useRealTimers();vi.restoreAllMocks();vi.clearAllMocks()});
function setup(states:string[], options:{blocked?:boolean;wrongOrigin?:boolean;loseDeliver?:boolean;rejectDeliver?:boolean}={}) {
  vi.useFakeTimers();
  const popup={closed:false,close(){this.closed=true}};
  vi.spyOn(window,"open").mockReturnValue(options.blocked?null:popup as unknown as Window);
  const ops:string[]=[];
  const request=vi.fn(async (path:string,_method?:string,body?:unknown)=>{
    const data=body as {op:string;job:string};
    if(path.endsWith("handoff"))return {token:"a".repeat(64),vault_origin:options.wrongOrigin?"https://other.test":origin};
    ops.push(data.op);
    if(data.op==="deliver"&&options.loseDeliver)throw new TypeError("lost deliver response");
    if(data.op==="deliver"&&options.rejectDeliver)throw new ApiFailure(409,"unavailable");
    return {job:data.job,challenge:crypto.randomUUID(),state:data.op==="deliver"?"validating":states.shift(),flow:"b".repeat(64),handle:"a".repeat(64)};
  });
  const controller=new AbortController();
  function start(){return connectInKiri(request as Parameters<typeof connectInKiri>[0],"owner","member@example.test","gmail",origin,undefined,controller.signal,vi.fn())}
  return {ops,popup,start,controller};
}
it("reserves the job, checks the vault first, and starts Kiri only after credential delivery",async()=>{
  const {start,ops}=setup(["vault","validating","opening","consent","complete"]);
  const result=start();await vi.advanceTimersByTimeAsync(0);
  // Start has only reserved the server challenge. No Deliver means no Kiri process.
  expect(ops).toEqual(["start"]);
  const handoff=vi.mocked(postVaultForm).mock.calls[0][5]!;
  await handoff.receive({key:"sealed",iv:"nonce",body:"ciphertext"});
  expect(ops).toEqual(["start","deliver"]);
  await vi.advanceTimersByTimeAsync(5300);
  expect(await result).toEqual({handle:"a".repeat(64),flow:"b".repeat(64)});
  expect(ops).toEqual(["start","deliver","status","status","status","status"]);
});
it("continues the accepted Kiri job when the Deliver response is lost",async()=>{
  const {start,ops,popup}=setup(["vault","validating","opening","consent","complete"],{loseDeliver:true});
  const result=start();await vi.advanceTimersByTimeAsync(0);
  const handoff=vi.mocked(postVaultForm).mock.calls[0][5]!;
  await expect(handoff.receive({key:"sealed",iv:"nonce",body:"ciphertext"})).rejects.toThrow("lost deliver response");
  popup.close();
  await vi.advanceTimersByTimeAsync(5300);
  expect(await result).toEqual({handle:"a".repeat(64),flow:"b".repeat(64)});
  expect(ops).toEqual(["start","deliver","status","status","status","status"]);
});
it("rechecks after a stale vault status before cancelling a lost Deliver",async()=>{
  const {start,ops,popup}=setup(["vault","vault","validating","opening","consent","complete"],{loseDeliver:true});
  const result=start();await vi.advanceTimersByTimeAsync(1400);
  const handoff=vi.mocked(postVaultForm).mock.calls[0][5]!;
  await expect(handoff.receive({key:"sealed",iv:"nonce",body:"ciphertext"})).rejects.toThrow("lost deliver response");
  popup.close();
  await vi.advanceTimersByTimeAsync(5300);
  expect(await result).toEqual({handle:"a".repeat(64),flow:"b".repeat(64)});
  expect(ops).toEqual(["start","status","deliver","status","status","status","status"]);
});
it("keeps the vault selection when a lost Deliver never reached the server",async()=>{
  const {start,ops,popup,controller}=setup(["vault","vault"],{loseDeliver:true});
  const result=start();await vi.advanceTimersByTimeAsync(0);
  const handoff=vi.mocked(postVaultForm).mock.calls[0][5]!;
  await expect(handoff.receive({key:"sealed",iv:"nonce",body:"ciphertext"})).rejects.toThrow("lost deliver response");
  expect(popup.closed).toBe(false);await vi.advanceTimersByTimeAsync(1400);expect(popup.closed).toBe(false);
  await expect(handoff.receive({key:"sealed",iv:"nonce",body:"ciphertext"})).rejects.toThrow("lost deliver response");
  expect(ops).toEqual(["start","deliver","status","deliver"]);
  const outcome=expect(result).rejects.toThrow();controller.abort();await vi.advanceTimersByTimeAsync(1400);await outcome;
});
it("keeps the vault open and surfaces a definite Deliver rejection",async()=>{
  const {start,ops,popup,controller}=setup(["vault"],{rejectDeliver:true});
  const result=start();await vi.advanceTimersByTimeAsync(0);
  const handoff=vi.mocked(postVaultForm).mock.calls[0][5]!;
  await expect(handoff.receive({key:"sealed",iv:"nonce",body:"ciphertext"})).rejects.toMatchObject({status:409});
  expect(popup.closed).toBe(false);expect(ops).toEqual(["start","deliver"]);
  const outcome=expect(result).rejects.toThrow();controller.abort();await vi.advanceTimersByTimeAsync(1400);await outcome;
});
it("does not launch a remote login if the secure popup cannot open",async()=>{
  const {start,ops}=setup([],{blocked:true});await expect(start()).rejects.toThrow("새 창");expect(ops).toEqual([]);
});
it("cancels the remote job if the registered vault origin changes",async()=>{
  const {start,ops}=setup(["vault"],{wrongOrigin:true});await expect(start()).rejects.toThrow();
  expect(postVaultForm).not.toHaveBeenCalled();expect(ops).toEqual(["start","cancel"]);
});
it("cancels when the member closes the vault before credentials are delivered",async()=>{
  const {start,ops,popup}=setup(["vault","vault"]);const outcome=expect(start()).rejects.toThrow("취소");
  await vi.advanceTimersByTimeAsync(0);popup.close();await vi.advanceTimersByTimeAsync(1400);await outcome;
  expect(ops).toContain("cancel");expect(postVaultForm).toHaveBeenCalledOnce();
});
it.each(["setup_required","needs_human","blocked"])("never reports a grant on %s",async state=>{
  const {start}=setup([state]);await expect(start()).rejects.toThrow();expect(postVaultForm).not.toHaveBeenCalled();
});
