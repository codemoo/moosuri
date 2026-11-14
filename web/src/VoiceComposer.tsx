import { forwardRef, useEffect, useImperativeHandle, useRef, useState } from "react";
import type { AudioJob, AudioSettings } from "../shared/api-types";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";
import { canonicalWav, VoiceRecorder, VoiceError, type RecorderState } from "./voice-audio";
import "./voice.css";
export type VoiceComposerHandle = {
    moveSlot: (from: string, to: string) => void;
};
const initial: RecorderState = { phase: "idle", seconds: 0, level: 0 };
const time = (seconds: number) => `${Math.floor(seconds / 60)}:${String(Math.floor(seconds % 60)).padStart(2, "0")}`;
function message(error: unknown) { if (error instanceof ApiFailure) {
    if (error.status === 429 || error.code === "budget_exhausted")
        return "오늘의 음성 사용 한도에 도달했습니다. 글로 계속 대화할 수 있습니다.";
    if (error.code === "audio_credentials_rejected")
        return "음성 연결 인증을 확인해 주세요.";
    if (error.code === "audio_outcome_unknown")
        return "변환 결과를 확인하지 못했습니다. 제공업체에서 처리했을 수 있습니다. 다시 변환하면 사용량이 추가될 수 있습니다.";
} return error instanceof VoiceError ? error.message : "음성을 글로 바꾸지 못했습니다. 녹음을 확인한 뒤 다시 시도해 주세요."; }
export const VoiceComposer = forwardRef<VoiceComposerHandle, {
    accountId: string;
    slot: string;
    onInsert: (slot: string, text: string) => boolean;
    onExpired: () => void;
}>(function VoiceComposer({ accountId, slot, onInsert, onExpired }, ref) {
    const scope = useScope(accountId), recorder = useRef<VoiceRecorder | null>(null), operation = useRef<AbortController | null>(null), job = useRef<string | null>(null), attempt = useRef<{id:string;wav:Blob}|null>(null), recoveryBusy=useRef(false), origin = useRef(slot), generation = useRef(0), live = useRef(true), inserted = useRef(false), urlRef = useRef<string | null>(null), preview = useRef<HTMLAudioElement | null>(null);
    const [open, setOpen] = useState(false), [state, setState] = useState(initial), [url, setUrl] = useState<string | null>(null), [settings, setSettings] = useState<AudioSettings | null>(null), [stage, setStage] = useState<"idle" | "preparing" | "uploading" | "transcribing" | "ready" | "recover" | "closed">("idle"), [progress, setProgress] = useState(0), [transcript, setTranscript] = useState(""), [error, setError] = useState(""), [notice, setNotice] = useState("");
    const callbacks = useRef({ onInsert, onExpired });
    callbacks.current = { onInsert, onExpired };
    function cancelJob() { attempt.current=null; recoveryBusy.current=false; operation.current?.abort(); operation.current = null; const id = job.current; job.current = null; if (id)
        void scope.request(`/audio/${id}`, "DELETE").catch(() => { }); }
    function stopPreview(){const audio=preview.current;if(audio){audio.pause();audio.removeAttribute("src");audio.load();}}
    function clear() { stopPreview(); generation.current++; recorder.current?.dispose(); recorder.current = null; cancelJob(); if (urlRef.current)
        scope.releaseUrl(urlRef.current); urlRef.current = null; setUrl(null); setState(initial); setStage("idle"); setTranscript(""); setError(""); setNotice(""); inserted.current = false; }
    useImperativeHandle(ref, () => ({ moveSlot(from, to) { if (origin.current === from)
            origin.current = to; } }), []);
    useEffect(() => { live.current = true; const stop = () => { stopPreview(); live.current = false; generation.current++; recorder.current?.dispose(); operation.current?.abort(); }; const remove = scope.onClose(stop); return () => { stop(); cancelJob(); remove(); if (urlRef.current)
        scope.releaseUrl(urlRef.current); }; }, [scope]);
    useEffect(() => { if (origin.current !== slot) recorder.current?.interrupt(); }, [slot]);
    useEffect(() => { const interrupt = () => { if (document.visibilityState === "hidden")
        recorder.current?.interrupt(); }; const pagehide = () => recorder.current?.interrupt(); document.addEventListener("visibilitychange", interrupt); window.addEventListener("pagehide", pagehide); return () => { document.removeEventListener("visibilitychange", interrupt); window.removeEventListener("pagehide", pagehide); }; }, []);
    useEffect(() => { const controller = new AbortController(); void scope.request<AudioSettings>("/audio/settings", "GET", undefined, controller.signal).then(setSettings).catch(e => { if (e instanceof ApiFailure && e.status === 401)
        callbacks.current.onExpired(); }); return () => controller.abort(); }, [scope]);
    function start() { clear(); origin.current = slot; const before = generation.current; const current = new VoiceRecorder(value => { if (!live.current || before !== generation.current)
        return; setState(value); if (value.blob) {
        if (urlRef.current)
            scope.releaseUrl(urlRef.current);
        urlRef.current = scope.localUrl(value.blob);
        setUrl(urlRef.current);
    } }); recorder.current = current; void current.start(); }
    async function convert() {
        if (recoveryBusy.current || !state.blob || (!attempt.current&&!settings?.available)) return;
        recoveryBusy.current=true;
        // Transport recovery retires only this request. It never cancels/replaces its
        // durable attempt; the UUID and canonical Blob remain immutable for this recording.
        operation.current?.abort();
        const controller=new AbortController();operation.current=controller;
        const before=++generation.current;
        const current=()=>live.current&&!controller.signal.aborted&&generation.current===before;
        setError("");setNotice("");inserted.current=false;
        try {
            let value:AudioJob;
            if (attempt.current) {
                const original=attempt.current;
                setStage("transcribing");
                try { value=await scope.request<AudioJob>(`/audio/${original.id}`,"GET",undefined,controller.signal); }
                catch(error) {
                    if (!(error instanceof ApiFailure&&error.status===404)) throw error;
                    // No receipt: the original POST may not have arrived. Reuse the
                    // exact ID/bytes; backend dedup resolves a racing original admission.
                    if(!current())return;setStage("uploading");
                    value=await scope.audioUpload<AudioJob>(original.id,original.wav,p=>{if(current())setProgress(p);},controller.signal);
                }
            } else {
                setStage("preparing");
                const wav=await canonicalWav(state.blob,controller.signal);if(!current())return;
                const original={id:crypto.randomUUID(),wav};attempt.current=original;job.current=original.id;
                setStage("uploading");setProgress(0);
                value=await scope.audioUpload<AudioJob>(original.id,original.wav,p=>{if(current())setProgress(p);},controller.signal);
            }
            if(!current())return;
            setStage("transcribing");
            while(value.state==="processing") {
                await new Promise<void>((resolve,reject)=>{const onAbort=()=>{clearTimeout(timer);reject(new DOMException("Cancelled","AbortError"));};const timer=setTimeout(()=>{controller.signal.removeEventListener("abort",onAbort);resolve();},500);controller.signal.addEventListener("abort",onAbort,{once:true});});
                value=await scope.request<AudioJob>(`/audio/${attempt.current!.id}`,"GET",undefined,controller.signal);if(!current())return;
            }
            if(value.state!=="ready"||!value.transcript) {
                setStage("closed");setError(message(new ApiFailure(502,value.error??"audio_outcome_unknown")));return;
            }
            setTranscript(value.transcript);setStage("ready");
        } catch(error) {
            if(!current()||isAborted(error))return;
            if(error instanceof ApiFailure&&error.status===401){callbacks.current.onExpired();return;}
            if(attempt.current) {
                // A network/ACK/status error is not permission for a second bill.
                // A definitive rejection closes the attempt; all ambiguous failures
                // expose only original-receipt recovery.
                if(error instanceof ApiFailure&&[400,403,409].includes(error.status)) {
                    setStage("closed");setError(message(error));
                } else {
                    setStage("recover");setError("변환 결과를 확인하지 못했습니다. 결과 확인을 누르면 같은 녹음의 처리 상태를 다시 조회합니다.");
                }
            } else {setStage("idle");setError(message(error));}
        } finally {if(current())recoveryBusy.current=false;}
    }
    const busy = ["preparing", "uploading", "transcribing"].includes(stage), capturing = ["permission", "recording", "paused", "stopping"].includes(state.phase);
    function insert() { if (inserted.current || !transcript.trim())
        return; if (!callbacks.current.onInsert(origin.current, transcript)) {
        setError("초안과 합해 8,192바이트까지 넣을 수 있습니다. 글을 줄여 주세요.");
        return;
    } clear(); inserted.current = true; setOpen(false); setNotice(origin.current === slot ? "초안에 넣었습니다. 내용을 확인하고 전송하세요." : "녹음을 시작한 대화의 초안에 넣었습니다."); }
    return <section className="voice-composer" aria-label="음성 입력">
  <button type="button" className="voice-toggle" aria-expanded={open} onClick={() => setOpen(v => !v)}><span aria-hidden="true">◉</span> {capturing && !open ? `녹음 중 · ${time(state.seconds)}` : "음성으로 입력"}</button>
  {!open && notice && <p className="voice-help" role="status">{notice}</p>}
  {open && <div className="voice-card">
   <div className="voice-header"><strong>{capturing ? state.phase === "paused" ? "잠시 멈춤" : state.phase === "permission" ? "마이크 권한 확인 중" : state.phase === "stopping" ? "녹음 마무리 중" : "듣고 있어요" : state.blob ? "녹음을 확인하세요" : "말을 글로, 내 속도로"}</strong><time aria-label={`녹음 시간 ${time(state.seconds)}`}>{time(state.seconds)}</time></div>
   {capturing && <div className="voice-meter" role="meter" aria-label="마이크 입력" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(state.level * 100)}><span style={{ width: `${Math.max(3, state.level * 100)}%` }}/></div>}
   {state.interrupted && <p role="status">녹음이 중단되었습니다. 남은 소리를 확인하거나 다시 녹음하세요.</p>}
   {url && <audio ref={preview} controls src={url} aria-label="녹음 미리 듣기" preload="metadata"/>}
   <div className="voice-actions">
    {!capturing && !state.blob && <button type="button" className="primary" onClick={start}>녹음 시작</button>}
    {(state.phase === "recording" || state.phase === "paused") && <><button type="button" onClick={() => recorder.current?.pause()}>{state.phase === "paused" ? "계속 녹음" : "잠시 멈춤"}</button><button type="button" className="primary" onClick={() => recorder.current?.stop()}>녹음 끝내기</button></>}
    {state.blob && !busy && <>{!["ready","closed"].includes(stage) && !notice && <button type="button" className="primary" onClick={() => void convert()} disabled={stage!=="recover"&&!settings?.available}>{stage==="recover"?"결과 확인":"글로 바꾸기"}</button>}<button type="button" onClick={start}>다시 녹음</button></>}
    {(state.phase !== "idle" || busy) && <button type="button" onClick={clear}>{busy || capturing ? "취소" : "녹음 삭제"}</button>}
   </div>
   {state.blob && !busy && stage !== "ready" && <p className="voice-help">{settings?.available ? "글로 바꾸면 녹음이 OpenAI API로 전송됩니다. 먼저 글을 확인하고 고칠 수 있습니다." : "음성을 글로 바꾸려면 관리자의 음성 연결 설정이 필요합니다. 녹음은 미리 들을 수 있습니다."}</p>}
   {busy && <p role="status">{stage === "preparing" ? "녹음을 준비하고 있어요…" : stage === "uploading" ? `녹음을 올리고 있어요… ${progress}%` : "말을 글로 옮기고 있어요…"}</p>}
   {stage === "ready" && <div className="voice-transcript"><label htmlFor="voice-transcript">글을 확인하고 고쳐 주세요</label><textarea id="voice-transcript" value={transcript} onChange={e => setTranscript(e.target.value)} rows={4}/><button type="button" className="primary" onClick={insert} disabled={!transcript.trim()}>초안에 넣기</button><p className="voice-help">초안에 넣은 뒤 메시지를 전송하세요.</p></div>}
   {(error || state.error) && <p role="alert">{error || state.error}</p>}{notice && <p role="status">{notice}</p>}
  </div>}
 </section>;
});
