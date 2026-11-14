import {createRef} from "react";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, afterEach, it, expect, vi } from "vitest";
import { VoiceComposer, type VoiceComposerHandle } from "./VoiceComposer";
import { canonicalWav } from "./voice-audio";
import { RequestScope } from "./api";
vi.mock("./voice-audio", async (original) => ({ ...await original<typeof import("./voice-audio")>(), canonicalWav: vi.fn() }));
class Recorder {
    static latest: Recorder;
    static isTypeSupported() { return true; }
    state = "inactive";
    mimeType = "audio/webm";
    ondataavailable: ((e: {
        data: Blob;
    }) => void) | null = null;
    onstop: (() => void) | null = null;
    onerror: null = null;
    constructor() { Recorder.latest = this; }
    start() { this.state = "recording"; }
    pause() { this.state = "paused"; }
    resume() { this.state = "recording"; }
    stop() { this.state = "inactive"; this.ondataavailable?.({ data: new Blob(["synthetic"]) }); this.onstop?.(); }
}
class Upload {
    static jobs: Upload[] = [];
    upload = { onprogress: null };
    onload: (() => void) | null = null;
    onabort: (() => void) | null = null;
    onerror: (()=>void) | null = null;
    body?:Blob;
    status = 202;
    responseText = "";
    withCredentials = false;
    path = "";
    constructor() { Upload.jobs.push(this); }
    open(_m: string, path: string) { this.path = path; }
    setRequestHeader() { }
    send(body:Blob) { this.body=body; }
    abort() { this.onabort?.(); }
    finish() { this.responseText = JSON.stringify({ id: "job", state: "ready", transcript: "음성 초안", error: null, expires_at: 9 }); this.onload?.(); }
}
let stop: ReturnType<typeof vi.fn>;
beforeEach(() => { vi.spyOn(HTMLMediaElement.prototype,"pause").mockImplementation(()=>{});vi.spyOn(HTMLMediaElement.prototype,"load").mockImplementation(()=>{}); Upload.jobs = []; vi.stubGlobal("XMLHttpRequest", Upload); vi.stubGlobal("MediaRecorder", Recorder); vi.stubGlobal("isSecureContext", true); stop = vi.fn(); Object.defineProperty(navigator, "mediaDevices", { configurable: true, value: { getUserMedia: vi.fn().mockResolvedValue({ getTracks: () => [{ stop, onended: null, onmute: null }] }) } }); Object.defineProperty(URL, "createObjectURL", { configurable: true, writable: true, value: vi.fn(() => "blob:synthetic") }); Object.defineProperty(URL, "revokeObjectURL", { configurable: true, writable: true, value: vi.fn() }); vi.mocked(canonicalWav).mockResolvedValue(new Blob(["canonical"])); vi.stubGlobal("fetch", vi.fn().mockImplementation(async (_url, init) => new Response(init?.method === "DELETE" ? null : JSON.stringify({ selection: { provider_id: "p", provider_revision: 1, model: "gpt-4o-mini-transcribe" }, available: true, max_seconds: 120 }), { status: init?.method === "DELETE" ? 204 : 200 }))); });
afterEach(() => vi.unstubAllGlobals());
async function preview() { fireEvent.click(screen.getByText("음성으로 입력")); fireEvent.click(screen.getByText("녹음 시작")); await screen.findByText("녹음 끝내기"); fireEvent.click(screen.getByText("녹음 끝내기")); await screen.findByText("글로 바꾸기"); }
it("text can send while recording and pending STT; edited transcript inserts once into original slot without a chat POST", async () => { const insert = vi.fn(() => true), send = vi.fn(); const props = { accountId: "a", slot: "first", onInsert: insert, onExpired: vi.fn() }; const view = render(<><VoiceComposer {...props}/><button onClick={send}>ordinary send</button></>); await preview(); fireEvent.click(screen.getByText("글로 바꾸기")); await waitFor(() => expect(Upload.jobs).toHaveLength(1)); fireEvent.click(screen.getByText("ordinary send")); expect(send).toHaveBeenCalledTimes(1); view.rerender(<><VoiceComposer {...props} slot="second"/><button onClick={send}>ordinary send</button></>); act(() => Upload.jobs[0].finish()); const transcript = await screen.findByLabelText("글을 확인하고 고쳐 주세요"); fireEvent.change(transcript, { target: { value: "고친 초안" } }); fireEvent.click(screen.getByText("초안에 넣기")); expect(insert).toHaveBeenCalledExactlyOnceWith("first", "고친 초안"); expect(screen.queryByText("초안에 넣기")).not.toBeInTheDocument(); expect(vi.mocked(fetch).mock.calls.some(([url]) => String(url).includes("/chat"))).toBe(false); expect(stop).toHaveBeenCalled(); });
it("cancel discards pending preparation and ignores its late result", async () => { let resolve!: (b: Blob) => void; vi.mocked(canonicalWav).mockImplementation(() => new Promise(r => resolve = r)); render(<VoiceComposer accountId="a" slot="first" onInsert={() => true} onExpired={() => { }}/>); await preview(); fireEvent.click(screen.getByText("글로 바꾸기")); await screen.findByText("녹음을 준비하고 있어요…"); fireEvent.click(screen.getByText("취소")); await act(async () => resolve(new Blob(["late"]))); expect(Upload.jobs).toHaveLength(0); expect(screen.queryByLabelText("글을 확인하고 고쳐 주세요")).not.toBeInTheDocument(); });
it("synchronous account closure cleans microphone before UI unmount", () => { const scope = new RequestScope("a"), cleanup = vi.fn(); scope.onClose(cleanup); scope.close(); expect(cleanup).toHaveBeenCalledOnce(); scope.close(); expect(cleanup).toHaveBeenCalledOnce(); });

it("new-thread slot migration keeps recording continuous and inserts into its assigned thread",async()=>{
 const ref=createRef<VoiceComposerHandle>(),insert=vi.fn(()=>true);
 const props={accountId:"a",slot:"new",onInsert:insert,onExpired:vi.fn()};
 const view=render(<VoiceComposer ref={ref} {...props}/>);
 fireEvent.click(screen.getByText("음성으로 입력"));fireEvent.click(screen.getByText("녹음 시작"));await screen.findByText("녹음 끝내기");
 act(()=>ref.current?.moveSlot("new","created-thread"));view.rerender(<VoiceComposer ref={ref} {...props} slot="created-thread"/>);
 expect(Recorder.latest.state).toBe("recording");
 fireEvent.click(screen.getByText("녹음 끝내기"));await screen.findByText("글로 바꾸기");fireEvent.click(screen.getByText("글로 바꾸기"));await waitFor(()=>expect(Upload.jobs).toHaveLength(1));act(()=>Upload.jobs[0].finish());await screen.findByLabelText("글을 확인하고 고쳐 주세요");fireEvent.click(screen.getByText("초안에 넣기"));expect(insert).toHaveBeenCalledExactlyOnceWith("created-thread","음성 초안");
});

it("cleanup failure cannot prevent account-wide resource fencing",()=>{const scope=new RequestScope("a"),later=vi.fn();scope.onClose(()=>{throw new Error("synthetic cleanup failure")});scope.onClose(later);expect(()=>scope.close()).not.toThrow();expect(later).toHaveBeenCalledOnce();});

it("lost committed upload ACK recovers the original UUID without a second upload or deletion",async()=>{
 const insert=vi.fn(()=>true);let reads=0;
 vi.mocked(fetch).mockImplementation(async(url,init)=>{
  if(String(url).includes("/audio/settings"))return new Response(JSON.stringify({available:true,selection:{provider_id:"p",provider_revision:1,model:"gpt-4o-mini-transcribe"},max_seconds:120}));
  if(init?.method==="DELETE")return new Response(null,{status:204});
  reads++;return new Response(JSON.stringify({id:"job",state:"ready",transcript:"원래 결과",error:null,expires_at:9}));
 });
 render(<VoiceComposer accountId="a" slot="first" onInsert={insert} onExpired={()=>{}}/>);await preview();fireEvent.click(screen.getByText("글로 바꾸기"));await waitFor(()=>expect(Upload.jobs).toHaveLength(1));
 const first=Upload.jobs[0];act(()=>first.onerror?.());await screen.findByRole("alert");
 fireEvent.click(screen.getByRole("button",{name:/결과 확인|글로 바꾸기/}));
 await waitFor(()=>expect(reads).toBe(1));expect(Upload.jobs).toHaveLength(1);
 expect(vi.mocked(fetch).mock.calls.filter(([,init])=>init?.method==="DELETE")).toHaveLength(0);
 expect(String(vi.mocked(fetch).mock.calls.at(-1)?.[0])).toBe(first.path);await screen.findByLabelText("글을 확인하고 고쳐 주세요");
});
it("failed status poll keeps its immutable attempt and closes outcome unknown without a new charge",async()=>{
 let polls=0;vi.mocked(fetch).mockImplementation(async(url,init)=>{
  if(String(url).includes("/audio/settings"))return new Response(JSON.stringify({available:true,selection:{provider_id:"p",provider_revision:1,model:"gpt-4o-mini-transcribe"},max_seconds:120}));
  if(init?.method==="DELETE")return new Response(null,{status:204});
  polls++;if(polls===1)throw new TypeError("synthetic lost status");return new Response(JSON.stringify({id:"job",state:"outcome_unknown",transcript:null,error:"audio_outcome_unknown",expires_at:9}));
 });
 render(<VoiceComposer accountId="a" slot="first" onInsert={()=>true} onExpired={()=>{}}/>);await preview();fireEvent.click(screen.getByText("글로 바꾸기"));await waitFor(()=>expect(Upload.jobs).toHaveLength(1));const first=Upload.jobs[0];act(()=>{first.responseText=JSON.stringify({id:"job",state:"processing",transcript:null,error:null,expires_at:9});first.onload?.();});await screen.findByRole("alert");
 fireEvent.click(screen.getByRole("button",{name:/결과 확인|글로 바꾸기/}));await waitFor(()=>expect(polls).toBe(2));expect(Upload.jobs).toHaveLength(1);expect(screen.getByRole("alert")).toHaveTextContent("처리했을 수 있습니다");expect(screen.queryByRole("button",{name:/결과 확인|글로 바꾸기/})).not.toBeInTheDocument();
});

it("missing ACK with no visible receipt resubmits exactly the original UUID and WAV",async()=>{
 vi.mocked(fetch).mockImplementation(async(url)=>{
  if(String(url).includes("/audio/settings"))return new Response(JSON.stringify({available:true,selection:{provider_id:"p",provider_revision:1,model:"gpt-4o-mini-transcribe"},max_seconds:120}));
  return new Response(JSON.stringify({code:"not_found"}),{status:404});
 });
 render(<VoiceComposer accountId="a" slot="first" onInsert={()=>true} onExpired={()=>{}}/>);await preview();fireEvent.click(screen.getByText("글로 바꾸기"));await waitFor(()=>expect(Upload.jobs).toHaveLength(1));const original=Upload.jobs[0];act(()=>original.onerror?.());await screen.findByRole("alert");fireEvent.click(screen.getByRole("button",{name:"결과 확인"}));await waitFor(()=>expect(Upload.jobs).toHaveLength(2));expect(Upload.jobs[1].path).toBe(original.path);expect(Upload.jobs[1].body).toBe(original.body);expect(canonicalWav).toHaveBeenCalledTimes(1);act(()=>Upload.jobs[1].finish());await screen.findByLabelText("글을 확인하고 고쳐 주세요");expect(vi.mocked(fetch).mock.calls.some(([,init])=>init?.method==="DELETE")).toBe(false);
});
