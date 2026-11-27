// @vitest-environment jsdom
import { afterEach, expect, it, vi } from "vitest";
import {cancelGooglePending,connectInKiri,type KiriOptions} from "./kiri-google-connection";
import {clearGooglePending,readGooglePending,writeGooglePending,type GoogleKiriPending} from "./kiri-google-pending";
import {postVaultForm} from "./VaultLaunch";
import {ApiFailure} from "./api";

vi.mock("./VaultLaunch",()=>({postVaultForm:vi.fn()}));
const origin="https://vault.example.test";
afterEach(()=>{vi.useRealTimers();vi.restoreAllMocks();vi.clearAllMocks();sessionStorage.clear()});
function setup(states:string[], options:{blocked?:boolean;wrongOrigin?:boolean;loseDeliver?:boolean;rejectDeliver?:boolean;deliverError?:Error;statusError?:Error;startError?:Error;cancelError?:Error;foreignJob?:boolean}={}) {
  vi.useFakeTimers();
  const popup={closed:false,close(){this.closed=true}};
  vi.spyOn(window,"open").mockReturnValue(options.blocked?null:popup as unknown as Window);
  const ops:string[]=[];
  const request=vi.fn(async (path:string,_method?:string,body?:unknown)=>{
    const data=body as {op:string;job:string};
    if(path.endsWith("handoff"))return {token:"a".repeat(64),vault_origin:options.wrongOrigin?"https://other.test":origin};
    ops.push(data.op);
    if(data.op==="start"&&options.startError)throw options.startError;
    if(data.op==="cancel"&&options.cancelError)throw options.cancelError;
    if(data.op==="deliver"&&options.deliverError)throw options.deliverError;
    if(data.op==="status"&&options.statusError){const error=options.statusError;options.statusError=undefined;throw error;}
    if(data.op==="deliver"&&options.loseDeliver)throw new TypeError("lost deliver response");
    if(data.op==="deliver"&&options.rejectDeliver)throw new ApiFailure(409,"unavailable");
    return {job:options.foreignJob?crypto.randomUUID():data.job,challenge:crypto.randomUUID(),state:data.op==="deliver"?"validating":states.shift(),flow:"b".repeat(64),handle:"a".repeat(64),email:"selected@example.test"};
  });
  const controller=new AbortController();
  function start(kiri?:KiriOptions,read?:{thread_id:string;request_id:string}){return connectInKiri(request as Parameters<typeof connectInKiri>[0],"owner","member@example.test","gmail",origin,read,controller.signal,vi.fn(),kiri)}
  return {ops,popup,start,controller,request};
}
it("reserves the job, checks the vault first, and starts Kiri only after credential delivery",async()=>{
  const {start,ops}=setup(["vault","validating","opening","consent","complete"]);
  const result=start();await vi.advanceTimersByTimeAsync(0);
  // Start has only reserved the server challenge. No Deliver means no Chrome sign-in.
  expect(ops).toEqual(["start"]);
  const handoff=vi.mocked(postVaultForm).mock.calls[0][5]!;
  await handoff.receive({key:"sealed",iv:"nonce",body:"ciphertext"},"selected@example.test");
  expect(ops).toEqual(["start","deliver"]);
  await vi.advanceTimersByTimeAsync(5300);
  expect(await result).toMatchObject({handle:"a".repeat(64),flow:"b".repeat(64),email:"selected@example.test"});
  expect(ops).toEqual(["start","deliver","status","status","status","status"]);
});
it("continues the accepted Kiri job when the Deliver response is lost",async()=>{
  const {start,ops,popup}=setup(["vault","validating","opening","consent","complete"],{loseDeliver:true});
  const result=start();await vi.advanceTimersByTimeAsync(0);
  const handoff=vi.mocked(postVaultForm).mock.calls[0][5]!;
  await expect(handoff.receive({key:"sealed",iv:"nonce",body:"ciphertext"},"selected@example.test")).rejects.toThrow("lost deliver response");
  popup.close();
  await vi.advanceTimersByTimeAsync(5300);
  expect(await result).toMatchObject({handle:"a".repeat(64),flow:"b".repeat(64),email:"selected@example.test"});
  expect(ops).toEqual(["start","deliver","status","status","status","status"]);
});
it("rechecks after a stale vault status before cancelling a lost Deliver",async()=>{
  const {start,ops,popup}=setup(["vault","vault","validating","opening","consent","complete"],{loseDeliver:true});
  const result=start();await vi.advanceTimersByTimeAsync(1400);
  const handoff=vi.mocked(postVaultForm).mock.calls[0][5]!;
  await expect(handoff.receive({key:"sealed",iv:"nonce",body:"ciphertext"},"selected@example.test")).rejects.toThrow("lost deliver response");
  popup.close();
  await vi.advanceTimersByTimeAsync(5300);
  expect(await result).toMatchObject({handle:"a".repeat(64),flow:"b".repeat(64),email:"selected@example.test"});
  expect(ops).toEqual(["start","status","deliver","status","status","status","status"]);
});
it("keeps the vault selection when a lost Deliver never reached the server",async()=>{
  const {start,ops,popup,controller}=setup(["vault","vault"],{loseDeliver:true});
  const result=start();await vi.advanceTimersByTimeAsync(0);
  const handoff=vi.mocked(postVaultForm).mock.calls[0][5]!;
  await expect(handoff.receive({key:"sealed",iv:"nonce",body:"ciphertext"},"selected@example.test")).rejects.toThrow("lost deliver response");
  expect(popup.closed).toBe(false);await vi.advanceTimersByTimeAsync(1400);expect(popup.closed).toBe(false);
  expect(ops).toEqual(["start","deliver","status","deliver"]);
  const outcome=expect(result).rejects.toThrow();controller.abort();await vi.advanceTimersByTimeAsync(1400);await outcome;
});
it("surfaces a definite Deliver rejection and cancels the rejected job",async()=>{
  const {start,ops,popup}=setup(["vault","vault"],{rejectDeliver:true});
  const result=start();await vi.advanceTimersByTimeAsync(0);
  const handoff=vi.mocked(postVaultForm).mock.calls[0][5]!;
  await expect(handoff.receive({key:"sealed",iv:"nonce",body:"ciphertext"},"selected@example.test")).rejects.toMatchObject({status:409});
  expect(popup.closed).toBe(false);expect(ops).toEqual(["start","deliver"]);
  const outcome=expect(result).rejects.toMatchObject({status:409});await vi.advanceTimersByTimeAsync(1400);await outcome;
  expect(popup.closed).toBe(true);expect(ops).toEqual(["start","deliver","status","cancel"]);
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
it.each(["setup_required","setup_pending","setup_recovery_required","setup_cleanup_pending","setup_busy","needs_human","blocked"])("never reports a grant on %s",async state=>{
  const {start}=setup([state]);await expect(start()).rejects.toThrow();expect(postVaultForm).not.toHaveBeenCalled();
});

it("keeps one delivered attempt through first-install setup longer than the old login window",async()=>{
  const {start,ops}=setup(["vault","signed_in",...Array<string>(340).fill("setup"),"consent","complete"]);
  const result=start();await vi.advanceTimersByTimeAsync(0);
  await vi.mocked(postVaultForm).mock.calls[0][5]!.receive({key:"sealed"},"selected@example.test");
  await vi.advanceTimersByTimeAsync(450000);
  expect((await result).email).toBe("selected@example.test");
  expect(ops.filter(op=>op==="start")).toHaveLength(1);
  expect(ops.filter(op=>op==="deliver")).toHaveLength(1);
  expect(ops).not.toContain("cancel");
});

it("cancels the same first-install setup on navigation without claiming a connection",async()=>{
  const {start,ops,controller,popup}=setup(["vault","setup","setup"]);
  const result=start();const outcome=expect(result).rejects.toThrow();
  await vi.advanceTimersByTimeAsync(0);
  await vi.mocked(postVaultForm).mock.calls[0][5]!.receive({key:"sealed"},"selected@example.test");
  await vi.advanceTimersByTimeAsync(1400);controller.abort();
  await vi.advanceTimersByTimeAsync(1400);await outcome;
  expect(ops.filter(op=>op==="cancel")).toHaveLength(1);expect(popup.closed).toBe(true);
});

it.each([408,429,502,503,504])("reconciles ambiguous Deliver HTTP %s without cancelling an accepted job",async status=>{
  const {start,ops}=setup(["vault","validating","complete"],{deliverError:new ApiFailure(status,"ambiguous"),statusError:new ApiFailure(503,"retry_status")});
  const result=start();await vi.advanceTimersByTimeAsync(0);
  const handoff=vi.mocked(postVaultForm).mock.calls[0][5]!;
  await expect(handoff.receive({key:"sealed"},"selected@example.test")).rejects.toMatchObject({status});
  await vi.advanceTimersByTimeAsync(4000);
  expect((await result).email).toBe("selected@example.test");expect(ops).not.toContain("cancel");
});
it.each([401,403,404])("terminates reconciliation on definite Status HTTP %s",async status=>{
  const {start,ops,popup}=setup(["vault"],{loseDeliver:true,statusError:new ApiFailure(status,"session_ended")});
  const result=start();const outcome=expect(result).rejects.toMatchObject({status});
  await vi.advanceTimersByTimeAsync(0);
  const handoff=vi.mocked(postVaultForm).mock.calls[0][5]!;
  await expect(handoff.receive({key:"sealed"},"selected@example.test")).rejects.toThrow();
  await vi.advanceTimersByTimeAsync(1400);await outcome;
  expect(popup.closed).toBe(true);expect(ops).toEqual(["start","deliver","status","cancel"]);
});

it.each([408,429,502,503,504])("keeps the delivered setup through transient status HTTP %s",async status=>{
  const {start,ops}=setup(["vault","setup","complete"],{statusError:new ApiFailure(status,"temporary")});
  const result=start();await vi.advanceTimersByTimeAsync(0);
  await vi.mocked(postVaultForm).mock.calls[0][5]!.receive({key:"sealed"},"selected@example.test");
  await vi.advanceTimersByTimeAsync(4000);
  expect((await result).email).toBe("selected@example.test");
  expect(ops.filter(op=>op==="deliver")).toHaveLength(1);expect(ops).not.toContain("cancel");
});

it("keeps polling the same Kiri attempt while Google waits for device verification",async()=>{
  const {start,ops,popup}=setup(["vault","opening","verifying","verifying","consent","complete"]);
  const result=start();await vi.advanceTimersByTimeAsync(0);
  await vi.mocked(postVaultForm).mock.calls[0][5]!.receive({key:"sealed"},"selected@example.test");
  await vi.advanceTimersByTimeAsync(6600);
  expect((await result).email).toBe("selected@example.test");
  expect(ops.filter(op=>op==="start")).toHaveLength(1);expect(ops).not.toContain("cancel");expect(popup.closed).toBe(true);
});

const scope="s".repeat(64);
const savedJob=(extra:Partial<GoogleKiriPending>={}):GoogleKiriPending=>({job:crypto.randomUUID(),account:"owner",scope,service:"gmail",created:Date.now(),updated:Date.now(),...extra});
const lastHandoff=()=>vi.mocked(postVaultForm).mock.calls.at(-1)![5]!;

it("persists only bounded metadata before Start and reload resumes the same job without a second Start",async()=>{
  const first=setup(["vault"]);
  void first.start({scope},{thread_id:"t1",request_id:"r1"}).catch(()=>{});
  await vi.advanceTimersByTimeAsync(0);
  const saved=readGooglePending("owner",scope,"gmail")!;
  expect(first.request.mock.calls[0][2]).toMatchObject({op:"start",job:saved.job});
  expect(Object.keys(saved).sort()).toEqual(["account","created","job","read","scope","service","updated"]);
  expect(saved.read).toEqual({thread_id:"t1",request_id:"r1"});
  // Reload destroys the old realm and its timers without sending Cancel.
  vi.clearAllTimers();
  // A new page resumes by Status.
  const second=setup(["vault","validating","complete"]);
  vi.mocked(window.open).mockClear();
  const result=second.start({scope,resume:saved});
  await vi.advanceTimersByTimeAsync(0);
  expect(second.ops).toEqual(["status"]);
  expect(window.open).toHaveBeenCalledOnce();
  expect(vi.mocked(window.open).mock.calls[0][1]).toBe("moosuri-vault-"+saved.job);
  await lastHandoff().receive({key:"sealed"},"selected@example.test");
  await vi.advanceTimersByTimeAsync(2700);
  expect(await result).toMatchObject({job:saved.job});
  expect(second.ops).toEqual(["status","deliver","status","status"]);
});
it("polls an already accepted job after reload without opening the vault or delivering again",async()=>{
  const saved=savedJob();writeGooglePending(saved);
  const run=setup(["validating","opening","consent","complete"]);
  const result=run.start({scope,resume:saved});
  await vi.advanceTimersByTimeAsync(4000);
  expect(await result).toMatchObject({job:saved.job,handle:"a".repeat(64)});
  expect(run.ops).toEqual(["status","status","status","status"]);
  expect(run.popup.closed).toBe(true);expect(postVaultForm).not.toHaveBeenCalled();
});
it("keeps the record when the completion acknowledgment is lost and removes only the exact job afterwards",async()=>{
  const saved=savedJob();writeGooglePending(saved);
  const run=setup(["complete","complete"]);
  const first=await run.start({scope,resume:saved});
  expect(readGooglePending("owner",scope,"gmail")?.job).toBe(saved.job);
  clearGooglePending({...saved,job:crypto.randomUUID()});
  expect(readGooglePending("owner",scope,"gmail")?.job).toBe(saved.job);
  // The retry reads fresh status and finalizes the same flow/handle.
  expect(await run.start({scope,resume:saved})).toEqual(first);
  expect(run.ops).toEqual(["status","status"]);
  clearGooglePending(saved);
  expect(readGooglePending("owner",scope,"gmail")).toBeNull();
});
it("returns to a fresh choice only after expiry, terminal failure, or confirmed cancel",async()=>{
  for (const run of [setup([],{statusError:new ApiFailure(404,"gone")}),setup(["denied"])]) {
    const saved=savedJob();writeGooglePending(saved);
    await expect(run.start({scope,resume:saved})).rejects.toThrow();
    expect(readGooglePending("owner",scope,"gmail")).toBeNull();
    expect(run.ops).toEqual(["status"]);
  }
  const confirmed=setup([]);let saved=savedJob();writeGooglePending(saved);
  expect(await cancelGooglePending(confirmed.request as never,saved)).toBe("cleared");
  expect(readGooglePending("owner",scope,"gmail")).toBeNull();
  const lost=setup([],{cancelError:new ApiFailure(503,"down")});saved=savedJob();writeGooglePending(saved);
  expect(await cancelGooglePending(lost.request as never,saved)).toBe("kept");
  expect(readGooglePending("owner",scope,"gmail")?.job).toBe(saved.job);
  const missing=setup([],{cancelError:new ApiFailure(404,"gone")});
  expect(await cancelGooglePending(missing.request as never,saved)).toBe("cleared");
});
it("never lets another login, account, service or foreign job adopt or delete saved work",async()=>{
  const saved=savedJob();writeGooglePending(saved);
  expect(readGooglePending("owner","t".repeat(64),"gmail")).toBeNull();
  expect(readGooglePending("other",scope,"gmail")).toBeNull();
  expect(readGooglePending("owner",scope,"docs")).toBeNull();
  expect(readGooglePending("owner",undefined,"gmail")).toBeNull();
  expect(writeGooglePending(savedJob())).toBe(false);
  expect(readGooglePending("owner",scope,"gmail")?.job).toBe(saved.job);
  const wrong=setup(["vault"]);
  await expect(wrong.start({scope:"t".repeat(64),resume:saved})).rejects.toThrow();
  await expect(wrong.start({scope,resume:{...saved,account:"other"}})).rejects.toThrow();
  expect(wrong.ops).toEqual([]);
  // A status describing another run is never adopted and never cancelled.
  const foreign=setup(["vault"],{foreignJob:true});
  await expect(foreign.start({scope,resume:saved})).rejects.toThrow();
  expect(foreign.ops).toEqual(["status"]);
});
it("keeps resumable work through popup, network and storage failures",async()=>{
  let saved=savedJob();writeGooglePending(saved);
  const blocked=setup(["vault"],{blocked:true});
  await expect(blocked.start({scope,resume:saved})).rejects.toThrow("새 창");
  const flaky=setup([],{statusError:new ApiFailure(503,"down")});
  await expect(flaky.start({scope,resume:saved})).rejects.toMatchObject({status:503});
  expect(readGooglePending("owner",scope,"gmail")?.job).toBe(saved.job);
  expect([...blocked.ops,...flaky.ops]).toEqual(["status","status"]);
  clearGooglePending(saved);
  vi.spyOn(Storage.prototype,"setItem").mockImplementation(()=>{throw new Error("quota")});
  const persisted=vi.fn();
  const memory=setup(["vault","complete"]);
  const result=memory.start({scope,persisted});
  await vi.advanceTimersByTimeAsync(1400);
  expect(persisted).toHaveBeenCalledWith(false);
  expect(await result).toMatchObject({handle:"a".repeat(64)});
  expect(memory.ops).toEqual(["start","status"]);
});
it("does not cancel a resumed job whose late packet was accepted while a new handoff conflicted",async()=>{
  const saved=savedJob();writeGooglePending(saved);
  const run=setup(["vault","validating","complete"],{rejectDeliver:true});
  const result=run.start({scope,resume:saved});
  await vi.advanceTimersByTimeAsync(0);
  await expect(lastHandoff().receive({key:"sealed"},"selected@example.test")).rejects.toMatchObject({status:409});
  await vi.advanceTimersByTimeAsync(3000);
  expect(await result).toMatchObject({job:saved.job});
  expect(run.ops).toEqual(["status","deliver","status","status"]);
});

it("keeps the popup open while the portable recipient is prepared",async()=>{
  const {start,popup,ops}=setup(["preparing","preparing","vault","validating","complete"]);
  const result=start();await vi.advanceTimersByTimeAsync(1400);
  expect(popup.closed).toBe(false);expect(postVaultForm).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(1300);expect(postVaultForm).toHaveBeenCalledOnce();
  const handoff=vi.mocked(postVaultForm).mock.calls[0][5]!;
  await handoff.receive({key:"sealed",iv:"nonce",body:"ciphertext"},"selected@example.test");
  await vi.advanceTimersByTimeAsync(2700);expect((await result).email).toBe("selected@example.test");
  expect(ops.filter(op=>op==="deliver")).toHaveLength(1);
});

it("preserves an uncertain Start and reconciles the same id instead of cancelling or starting twice",async()=>{
  const first=setup([],{startError:new TypeError("response lost")});
  await expect(first.start({scope})).rejects.toThrow("response lost");
  const saved=readGooglePending("owner",scope,"gmail")!;
  expect(saved.job).toBe((first.request.mock.calls[0][2] as {job:string}).job);
  expect(first.ops).toEqual(["start"]);
  const second=setup(["complete"]);
  expect((await second.start({scope,resume:saved})).job).toBe(saved.job);
  expect(second.ops).toEqual(["status"]);
});

it("ignores a late Vault handoff after a recoverable status failure has ended this page's run",async()=>{
  const run=setup(["vault"],{statusError:new ApiFailure(503,"temporary")});
  const outcome=expect(run.start({scope})).rejects.toMatchObject({status:503});
  await vi.advanceTimersByTimeAsync(0);const handoff=lastHandoff();
  await vi.advanceTimersByTimeAsync(1400);await outcome;
  await handoff.receive({key:"sealed"},"selected@example.test");
  expect(run.ops).toEqual(["start","status"]);
  expect(readGooglePending("owner",scope,"gmail")).not.toBeNull();
});

it("confirms explicit cancellation while the resumed initial status is still pending",async()=>{
  const saved=savedJob();writeGooglePending(saved);
  const cancelled=vi.fn(),controller=new AbortController(),ops:string[]=[];
  const popup={closed:false,close(){this.closed=true}};
  vi.spyOn(window,"open").mockReturnValue(popup as unknown as Window);
  const request=vi.fn(async (_p:string,_m?:string,body?:unknown,signal?:AbortSignal)=>{
    const data=body as {op:string};ops.push(data.op);
    if(data.op==="status")return await new Promise((_resolve,reject)=>signal!.addEventListener("abort",()=>reject(new DOMException("cancelled","AbortError")),{once:true}));
    return {state:"cancelled"};
  });
  const outcome=expect(connectInKiri(request as never,"owner","member@example.test","gmail",origin,undefined,controller.signal,vi.fn(),{scope,resume:saved,cancelled})).rejects.toThrow();
  controller.abort();await outcome;
  expect(ops).toEqual(["status","cancel"]);expect(cancelled).toHaveBeenCalledWith(true);
  expect(readGooglePending("owner",scope,"gmail")).toBeNull();
});
