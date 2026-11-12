export type StreamFrame = { event: string; data: string; id: string | null };
export class StreamFailure extends Error {
  constructor() {
    super("event_stream_unavailable");
  }
}

/** No private data is retained beyond the current bounded SSE frame. */
export class SseDecoder {
  private decoder = new TextDecoder("utf-8", { fatal: true });
  private line = "";
  private data: string[] = [];
  private event = "";
  private id: string | null = null;
  private eventBytes = 0;
  private afterCR = false;
  private first = true;
  push(bytes: Uint8Array): StreamFrame[] {
    if (bytes.byteLength > 1024 * 1024) throw new StreamFailure();
    let text: string;
    try {
      text = this.decoder.decode(bytes, { stream: true });
    } catch {
      throw new StreamFailure();
    }
    const frames: StreamFrame[] = [];
    let start = 0;
    for (let index = 0; index < text.length; index++) {
      const char = text[index];
      if (this.afterCR) {
        this.afterCR = false;
        if (char === "\n") {
          start = index + 1;
          continue;
        }
      }
      if (char !== "\n" && char !== "\r") continue;
      this.append(text.slice(start, index));
      this.consume(frames);
      this.afterCR = char === "\r";
      start = index + 1;
    }
    this.append(text.slice(start));
    return frames;
  }
  finish() {
    try {
      if (this.decoder.decode()) throw new StreamFailure();
    } catch {
      throw new StreamFailure();
    }
    if (this.line || this.data.length || this.event) throw new StreamFailure();
  }
  private append(text: string) {
    if (this.line.length + text.length > 1024 * 1024) throw new StreamFailure();
    this.line += text;
  }
  private consume(frames: StreamFrame[]) {
    let line = this.line;
    this.line = "";
    if (this.first) {
      this.first = false;
      line = line.replace(/^\uFEFF/, "");
    }
    if (!line) {
      if (this.data.length) {
        if (frames.length >= 1024) throw new StreamFailure();
        frames.push({
          event: this.event || "message",
          data: this.data.join("\n"),
          id: this.id,
        });
      }
      this.data = [];
      this.event = "";
      this.id = null;
      this.eventBytes = 0;
      return;
    }
    if (line.startsWith(":")) return;
    const colon = line.indexOf(":");
    const field = colon < 0 ? line : line.slice(0, colon);
    const value = colon < 0 ? "" : line.slice(colon + 1).replace(/^ /, "");
    if (field === "data") {
      this.eventBytes += new TextEncoder().encode(value).byteLength + 1;
      if (this.eventBytes > 512 * 1024) throw new StreamFailure();
      this.data.push(value);
    } else if (field === "event") {
      if (value.length > 64) throw new StreamFailure();
      this.event = value;
    } else if (field === "id") {
      if (value.length > 64 || value.includes("\0")) throw new StreamFailure();
      this.id = value;
    }
  }
}
