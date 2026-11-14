/// <reference lib="webworker" />
export {};
self.onmessage = (event: MessageEvent<Float32Array>) => {
    const samples = event.data;
    if (!(samples instanceof Float32Array) || samples.length < 1 || samples.length > 1920000) {
        self.postMessage({ error: true });
        return;
    }
    const out = new ArrayBuffer(44 + samples.length * 2), v = new DataView(out);
    const str = (at: number, s: string) => { for (let i = 0; i < s.length; i++)
        v.setUint8(at + i, s.charCodeAt(i)); };
    str(0, "RIFF");
    v.setUint32(4, out.byteLength - 8, true);
    str(8, "WAVEfmt ");
    v.setUint32(16, 16, true);
    v.setUint16(20, 1, true);
    v.setUint16(22, 1, true);
    v.setUint32(24, 16000, true);
    v.setUint32(28, 32000, true);
    v.setUint16(32, 2, true);
    v.setUint16(34, 16, true);
    str(36, "data");
    v.setUint32(40, samples.length * 2, true);
    for (let i = 0; i < samples.length; i++) {
        const s = Math.max(-1, Math.min(1, Number.isFinite(samples[i]) ? samples[i] : 0));
        v.setInt16(44 + i * 2, s < 0 ? s * 32768 : s * 32767, true);
    }
    self.postMessage(out, { transfer: [out] });
};
