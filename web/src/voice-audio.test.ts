import { beforeEach, afterEach, expect, it, vi } from "vitest";
import { VoiceRecorder, canonicalWav, type RecorderState } from "./voice-audio";
class FakeRecorder {
    static latest: FakeRecorder;
    static isTypeSupported = vi.fn(() => true);
    state = "inactive";
    mimeType = "audio/webm";
    ondataavailable: ((e: {
        data: Blob;
    }) => void) | null = null;
    onstop: (() => void) | null = null;
    onerror: (() => void) | null = null;
    constructor() { FakeRecorder.latest = this; }
    start() { this.state = "recording"; }
    pause() { this.state = "paused"; }
    resume() { this.state = "recording"; }
    stop() { this.state = "inactive"; this.ondataavailable?.({ data: new Blob(["synthetic audio"]) }); this.onstop?.(); }
}
let track: {
    stop: ReturnType<typeof vi.fn>;
    onended: (() => void) | null;
    onmute: (() => void) | null;
};
let stream: MediaStream;
beforeEach(() => { track = { stop: vi.fn(), onended: null, onmute: null }; stream = { getTracks: () => [track] } as unknown as MediaStream; vi.stubGlobal("isSecureContext", true); vi.stubGlobal("MediaRecorder", FakeRecorder); FakeRecorder.isTypeSupported.mockReturnValue(true); Object.defineProperty(navigator, "mediaDevices", { configurable: true, value: { getUserMedia: vi.fn().mockResolvedValue(stream) } }); });
afterEach(() => vi.unstubAllGlobals());
it("late permission after owner disposal releases every track and emits no preview", async () => { let resolve!: (s: MediaStream) => void; vi.mocked(navigator.mediaDevices.getUserMedia).mockImplementation(() => new Promise(r => resolve = r)); const events: RecorderState[] = []; const recorder = new VoiceRecorder(s => events.push(s)); const start = recorder.start(); recorder.dispose(); resolve(stream); await start; expect(track.stop).toHaveBeenCalledTimes(1); expect(events.map(v => v.phase)).toEqual(["permission"]); });
it("pause/resume preview and interruption stop the owned tracks without automatic resume", async () => { const events: RecorderState[] = []; const recorder = new VoiceRecorder(s => events.push(s)); await recorder.start(); recorder.pause(); expect(FakeRecorder.latest.state).toBe("paused"); recorder.pause(); expect(FakeRecorder.latest.state).toBe("recording"); track.onmute?.(); expect(events.at(-1)?.phase).toBe("preview"); expect(events.at(-1)?.interrupted).toBe(true); expect(events.at(-1)?.blob?.size).toBeGreaterThan(0); expect(track.stop).toHaveBeenCalled(); recorder.dispose(); });
it("unsupported codec fails before requesting a microphone", async () => { FakeRecorder.isTypeSupported.mockReturnValue(false); let state: RecorderState | undefined; await new VoiceRecorder(s => state = s).start(); expect(state?.phase).toBe("failed"); expect(navigator.mediaDevices.getUserMedia).not.toHaveBeenCalled(); });
it("canonical preparation rejects unsupported decode and closes its context", async () => { const close = vi.fn().mockResolvedValue(undefined); vi.stubGlobal("AudioContext", class {
    close = close;
    decodeAudioData = vi.fn().mockRejectedValue(new Error("decoder"));
}); const blob = { size: 12, arrayBuffer: async () => new ArrayBuffer(12) } as Blob; await expect(canonicalWav(blob, new AbortController().signal)).rejects.toThrow("브라우저"); expect(close).toHaveBeenCalled(); });
it("aborted decode cannot create a late worker", async () => { let resolve!: (v: AudioBuffer) => void; const worker = vi.fn(); vi.stubGlobal("Worker", worker); vi.stubGlobal("AudioContext", class {
    close = vi.fn().mockResolvedValue(undefined);
    decodeAudioData = () => new Promise<AudioBuffer>(r => resolve = r);
}); const abort = new AbortController(); const pending = canonicalWav({ size: 12, arrayBuffer: async () => new ArrayBuffer(12) } as Blob, abort.signal); await Promise.resolve(); await Promise.resolve(); abort.abort(); await expect(pending).rejects.toMatchObject({ name: "AbortError" }); resolve({ duration: 1, numberOfChannels: 1, length: 16000 } as AudioBuffer); await Promise.resolve(); expect(worker).not.toHaveBeenCalled(); });
