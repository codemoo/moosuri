/** Only bounded locally recorded audio is accepted here. This is not a hostile-file sandbox. */
export class VoiceError extends Error {
}
export async function canonicalWav(blob: Blob, signal: AbortSignal): Promise<Blob> {
    if (blob.size === 0 || blob.size > 8 * 1024 * 1024)
        throw new VoiceError("녹음 분량을 확인하지 못했습니다. 다시 녹음해 주세요.");
    let live = true;
    let context: AudioContext | undefined, worker: Worker | undefined;
    const check = () => { if (signal.aborted || !live)
        throw new DOMException("Cancelled", "AbortError"); };
    let timer: ReturnType<typeof setTimeout> | undefined;
    let onAbort: (() => void) | undefined;
    try {
        check();
        const operation = async () => {
            context = new AudioContext();
            const decoded = await context.decodeAudioData(await blob.arrayBuffer());
            check();
            if (!Number.isFinite(decoded.duration) || decoded.duration <= 0 || decoded.duration > 120 || decoded.numberOfChannels * decoded.length * 4 > 32 * 1024 * 1024)
                throw new VoiceError("녹음은 2분까지 글로 바꿀 수 있습니다. 짧게 다시 녹음해 주세요.");
            const count = Math.ceil(decoded.duration * 16000);
            if (count < 1 || count > 1920000)
                throw new VoiceError("녹음 분량을 확인하지 못했습니다.");
            const offline = new OfflineAudioContext(1, count, 16000), source = offline.createBufferSource();
            source.buffer = decoded;
            source.connect(offline.destination);
            source.start();
            const rendered = await offline.startRendering();
            check();
            const samples = rendered.getChannelData(0).slice();
            worker = new Worker(new URL("./voice-wav.worker.ts", import.meta.url), { type: "module" });
            const output = await new Promise<ArrayBuffer>((resolve, reject) => { worker!.onmessage = (event: MessageEvent<unknown>) => event.data instanceof ArrayBuffer ? resolve(event.data) : reject(new VoiceError("녹음을 준비하지 못했습니다.")); worker!.onerror = () => reject(new VoiceError("녹음을 준비하지 못했습니다.")); worker!.postMessage(samples, [samples.buffer]); });
            check();
            return new Blob([output], { type: "audio/wav" });
        };
        return await Promise.race([operation(), new Promise<never>((_, reject) => { timer = setTimeout(() => reject(new VoiceError("녹음 준비 시간이 초과되었습니다. 짧게 다시 녹음해 주세요.")), 10000); onAbort = () => reject(new DOMException("Cancelled", "AbortError")); signal.addEventListener("abort", onAbort, { once: true }); })]);
    }
    catch (e) {
        if (e instanceof VoiceError || signal.aborted)
            throw e;
        throw new VoiceError("이 브라우저에서 녹음을 읽지 못했습니다. 다른 지원 브라우저로 다시 시도해 주세요.");
    }
    finally {
        live = false;
        if (timer)
            clearTimeout(timer);
        if (onAbort)
            signal.removeEventListener("abort", onAbort);
        worker?.terminate();
        if (context)
            void context.close().catch(() => { });
    }
}
export type RecorderState = {
    phase: "idle" | "permission" | "recording" | "paused" | "stopping" | "preview" | "failed";
    seconds: number;
    level: number;
    blob?: Blob;
    error?: string;
    interrupted?: boolean;
};
/** An instance is one irreversible microphone lifetime; a new record uses a new instance. */
export class VoiceRecorder {
    private active = true;
    private stream?: MediaStream;
    private recorder?: MediaRecorder;
    private context?: AudioContext;
    private meter?: AnalyserNode;
    private source?: MediaStreamAudioSourceNode;
    private timer?: ReturnType<typeof setInterval>;
    private chunks: Blob[] = [];
    private size = 0;
    private startAt = 0;
    private activeMs = 0;
    private wallAt = 0;
    private paused = false;
    private stopping = false;
    private invalid = false;
    private value: RecorderState = { phase: "idle", seconds: 0, level: 0 };
    constructor(private changed: (s: RecorderState) => void) { }
    private emit(p: Partial<RecorderState>) { if (this.active) {
        this.value = { ...this.value, ...p };
        this.changed(this.value);
    } }
    private release() { if (this.timer)
        clearInterval(this.timer); this.timer = undefined; this.stream?.getTracks().forEach(t => { t.onended = null; t.onmute = null; t.stop(); }); this.source?.disconnect(); this.meter?.disconnect(); if (this.context)
        void this.context.close().catch(() => { }); this.context = undefined; this.stream = undefined; }
    async start() {
        if (!this.active)
            return;
        if (!window.isSecureContext || !navigator.mediaDevices?.getUserMedia || typeof MediaRecorder === "undefined") {
            this.emit({ phase: "failed", error: "이 브라우저에서는 마이크 녹음을 사용할 수 없습니다." });
            return;
        }
        this.emit({ phase: "permission" });
        try {
            const types = ["audio/webm;codecs=opus", "audio/mp4", "audio/webm", "audio/ogg;codecs=opus"];
            const type = types.find(t => MediaRecorder.isTypeSupported(t));
            if (!type)
                throw new VoiceError("이 브라우저에서 지원하는 녹음 형식을 찾지 못했습니다.");
            const stream = await navigator.mediaDevices.getUserMedia({ audio: { channelCount: 1, echoCancellation: true }, video: false });
            if (!this.active) {
                stream.getTracks().forEach(t => t.stop());
                return;
            }
            this.stream = stream;
            const recorder = new MediaRecorder(stream, { mimeType: type, audioBitsPerSecond: 64000 });
            this.recorder = recorder;
            recorder.ondataavailable = e => { if (!this.active || this.invalid || e.data.size === 0)
                return; if (this.size + e.data.size > 8 * 1024 * 1024) {
                this.invalid = true;
                this.emit({ error: "녹음 용량을 넘었습니다. 짧게 다시 녹음해 주세요." });
                this.chunks = [];
                this.stop();
                return;
            } this.chunks.push(e.data); this.size += e.data.size; };
            recorder.onerror = () => { this.invalid = true; this.emit({ error: "녹음이 중단되었습니다. 다시 녹음해 주세요." }); this.chunks = []; this.stop(); };
            recorder.onstop = () => { this.release(); if (!this.active)
                return; const blob = new Blob(this.chunks, { type: recorder.mimeType }); this.chunks = []; this.emit(blob.size && !this.invalid ? { phase: "preview", blob, level: 0 } : { phase: "failed", error: this.value.error ?? "녹음된 소리가 없습니다. 다시 녹음해 주세요.", level: 0 }); };
            stream.getTracks().forEach(t => { t.onended = () => this.interrupt(); t.onmute = () => this.interrupt(); });
            try {
                this.context = new AudioContext();
                this.source = this.context.createMediaStreamSource(stream);
                this.meter = this.context.createAnalyser();
                this.meter.fftSize = 256;
                this.source.connect(this.meter);
            }
            catch { /* Recording does not depend on a meter. */ }
            this.startAt = performance.now();
            this.wallAt = this.startAt;
            recorder.start(500);
            this.emit({ phase: "recording" });
            this.timer = setInterval(() => { const now = performance.now(), ms = this.activeMs + (this.paused ? 0 : now - this.startAt); let level = 0; if (this.meter) {
                const b = new Uint8Array(this.meter.fftSize);
                this.meter.getByteTimeDomainData(b);
                level = Math.min(1, Math.sqrt(b.reduce((s, v) => s + (v - 128) ** 2, 0) / b.length) / 40);
            } this.emit({ seconds: Math.min(120, ms / 1000), level }); if (ms >= 119500 || now - this.wallAt >= 300000)
                this.stop(); }, 100);
        }
        catch (e) {
            this.release();
            this.emit({ phase: "failed", error: e instanceof VoiceError ? e.message : e instanceof DOMException && e.name === "NotAllowedError" ? "마이크 사용이 허용되지 않았습니다. 브라우저 권한을 확인해 주세요." : "마이크를 시작하지 못했습니다. 다른 앱에서 사용 중인지 확인해 주세요." });
        }
    }
    pause() { if (!this.active || this.stopping || !this.recorder)
        return; try {
        if (!this.paused && this.recorder.state === "recording") {
            this.recorder.pause();
            this.activeMs += performance.now() - this.startAt;
            this.paused = true;
            this.emit({ phase: "paused" });
        }
        else if (this.paused && this.recorder.state === "paused") {
            this.recorder.resume();
            this.startAt = performance.now();
            this.paused = false;
            this.emit({ phase: "recording" });
        }
    }
    catch {
        this.interrupt();
    } }
    stop() { if (!this.active || this.stopping)
        return; this.stopping = true; if (!this.paused && this.startAt)
        this.activeMs += performance.now() - this.startAt; this.emit({ phase: "stopping", seconds: Math.min(120, this.activeMs / 1000), level: 0 }); if (this.recorder && this.recorder.state !== "inactive") {
        try {
            this.recorder.stop();
        }
        catch {
            this.release();
            this.emit({ phase: "failed", error: "녹음을 마무리하지 못했습니다." });
        }
    }
    else {
        this.release();
        this.emit({ phase: "failed", error: "녹음된 소리가 없습니다." });
    } this.stream?.getTracks().forEach(t => { t.onended = null; t.onmute = null; t.stop(); }); }
    interrupt() { if (["permission", "recording", "paused"].includes(this.value.phase)) {
        if (this.value.phase === "permission") {
            this.emit({ phase: "failed", error: "화면이 바뀌어 녹음을 취소했습니다. 다시 시작해 주세요." });
            this.dispose();
            return;
        }
        this.emit({ interrupted: true });
        this.stop();
    } }
    dispose() { this.active = false; this.recorder && (this.recorder.ondataavailable = null, this.recorder.onstop = null, this.recorder.onerror = null); if (this.recorder?.state !== "inactive")
        try {
            this.recorder?.stop();
        }
        catch { } this.release(); this.chunks = []; }
}
