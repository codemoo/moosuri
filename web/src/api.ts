/** An account lifetime owns every request. Closing it aborts transport and fences
 * responses even when a transport/server ignores AbortSignal. No private cache. */
export class RequestScope {
  constructor(private readonly expectedAccountId?: string) {}
  private active = true;
  private cleanups = new Set<()=>void>();
  onClose(cleanup:()=>void) { if(!this.active){cleanup();return ()=>{};}this.cleanups.add(cleanup);return ()=>{this.cleanups.delete(cleanup);}; }
  private urls = new Set<string>();
  private uploads = new Set<XMLHttpRequest>();
  releaseUrl(url: string) {
    URL.revokeObjectURL(url);
    this.urls.delete(url);
  }
  private keepUrl(blob: Blob) {
    if (!this.active)
      throw new DOMException("Closed account lifetime", "AbortError");
    const url = URL.createObjectURL(blob);
    this.urls.add(url);
    return url;
  }
  async imageUrl(path: string, signal?: AbortSignal): Promise<string> {
    if (!this.active)
      throw new DOMException("Closed account lifetime", "AbortError");
    const controller = new AbortController();
    const abort = () => controller.abort();
    signal?.addEventListener("abort", abort, { once: true });
    if (signal?.aborted) controller.abort();
    this.controllers.add(controller);
    try {
      const response = await fetch(`/api/v1${path}`, {
        credentials: "same-origin",
        cache: "no-store",
        signal: controller.signal,
        headers: this.expectedAccountId
          ? { "X-Moosuri-Account": this.expectedAccountId }
          : {},
      });
      if (!response.ok)
        throw new ApiFailure(response.status, "media_unavailable");
      const blob = await response.blob();
      if (!this.active || controller.signal.aborted)
        throw new DOMException("Closed account lifetime", "AbortError");
      return this.keepUrl(blob);
    } finally {
      this.controllers.delete(controller);
      signal?.removeEventListener("abort", abort);
    }
  }
  localUrl(blob: Blob) {
    return this.keepUrl(blob);
  }
  upload<T>(
    id: string,
    blob: Blob,
    progress: (percent: number) => void,
    signal: AbortSignal,
    filename?: string,
  ): Promise<T> {
    return this.uploadBinary<T>(filename === undefined ? `/api/v1/media/${encodeURIComponent(id)}` : `/api/v1/files/${encodeURIComponent(id)}?filename=${encodeURIComponent(filename)}`, blob, progress, signal);
  }
  audioUpload<T>(id: string, blob: Blob, progress: (percent:number)=>void, signal:AbortSignal, probe?:{provider_id:string;provider_revision:number}):Promise<T> {
    return this.uploadBinary<T>(probe ? "/api/v1/admin/audio/probe" : `/api/v1/audio/${encodeURIComponent(id)}`,blob,progress,signal,probe);
  }
  private uploadBinary<T>(path:string,blob:Blob,progress:(percent:number)=>void,signal:AbortSignal,probe?:{provider_id:string;provider_revision:number}):Promise<T> {
    if (!this.active || signal.aborted)
      return Promise.reject(
        new DOMException("Closed account lifetime", "AbortError"),
      );
    return new Promise((resolve, reject) => {
      const xhr = new XMLHttpRequest();
      this.uploads.add(xhr);
      const abort = () => xhr.abort();
      signal.addEventListener("abort", abort, { once: true });
      const done = () => {
        this.uploads.delete(xhr);
        signal.removeEventListener("abort", abort);
      };
      xhr.open("POST", path);
      if(probe){xhr.setRequestHeader("X-Audio-Provider",probe.provider_id);xhr.setRequestHeader("X-Audio-Revision",String(probe.provider_revision));}
      xhr.withCredentials = true;
      xhr.setRequestHeader("Content-Type", "application/octet-stream");
      if (this.expectedAccountId)
        xhr.setRequestHeader("X-Moosuri-Account", this.expectedAccountId);
      xhr.upload.onprogress = (e) => {
        if (this.active && !signal.aborted && e.lengthComputable)
          progress(Math.round((e.loaded / e.total) * 100));
      };
      xhr.onload = () => {
        done();
        if (!this.active || signal.aborted) {
          reject(new DOMException("Closed account lifetime", "AbortError"));
          return;
        }
        if (xhr.status < 200 || xhr.status >= 300) {
          let code = "media_upload_failed";
          try {
            if (xhr.responseText.length <= 1024) {
              const body: unknown = JSON.parse(xhr.responseText);
              const value = body && typeof body === "object" && "code" in body ? body.code : undefined;
              if (typeof value === "string" && ["audio_invalid", "audio_probe_mismatch", "audio_probe_too_long", "audio_credentials_rejected", "audio_rate_limited", "audio_outcome_unknown", "audio_response_invalid", "audio_unavailable", "pdf_encrypted", "pdf_no_text", "pdf_unsupported_content", "pdf_extraction_limit", "pdf_extraction_timeout", "pdf_extractor_unavailable", "pdf_invalid"].includes(value)) code = value;
            }
          } catch { /* Unknown error bodies keep the fixed generic failure. */ }
          reject(new ApiFailure(xhr.status, code));
          return;
        }
        try {
          resolve(JSON.parse(xhr.responseText) as T);
        } catch {
          reject(new ApiFailure(502, "media_upload_failed"));
        }
      };
      xhr.onerror = () => {
        done();
        reject(new TypeError("Upload unavailable"));
      };
      xhr.onabort = () => {
        done();
        reject(new DOMException("Upload cancelled", "AbortError"));
      };
      xhr.send(blob);
    });
  }
  private controllers = new Set<AbortController>();
  close() {
    this.active = false;
    for(const cleanup of this.cleanups){try{cleanup();}catch{/* Continue fencing every request even if a local resource cleanup fails. */}}
    this.cleanups.clear();
    for (const xhr of this.uploads) xhr.abort();
    this.uploads.clear();
    for (const url of this.urls) URL.revokeObjectURL(url);
    this.urls.clear();
    for (const controller of this.controllers) controller.abort();
    this.controllers.clear();
  }
  async request<T>(
    path: string,
    method = "GET",
    body?: unknown,
    signal?: AbortSignal,
  ): Promise<T> {
    if (!this.active)
      throw new DOMException("Closed account lifetime", "AbortError");
    const controller = new AbortController();
    const abort = () => controller.abort();
    signal?.addEventListener("abort", abort, { once: true });
    if (signal?.aborted) controller.abort();
    this.controllers.add(controller);
    try {
      const response = await fetch(`/api/v1${path}`, {
        method,
        credentials: "same-origin",
        signal: controller.signal,
        cache: "no-store",
        headers: {
          ...(this.expectedAccountId
            ? { "X-Moosuri-Account": this.expectedAccountId }
            : {}),
          ...(body === undefined ? {} : { "Content-Type": "application/json" }),
        },
        ...(body === undefined
          ? {}
          : {
              body: JSON.stringify(body),
            }),
      });
      if (!this.active || controller.signal.aborted)
        throw new DOMException("Closed account lifetime", "AbortError");
      if (!response.ok) {
        const value = await response.json().catch(() => ({}));
        if (!this.active || controller.signal.aborted)
          throw new DOMException("Closed account lifetime", "AbortError");
        throw new ApiFailure(
          response.status,
          typeof value.code === "string" ? value.code : "request_failed",
        );
      }
      const result =
        response.status === 204 ? undefined : await response.json();
      if (!this.active || controller.signal.aborted)
        throw new DOMException("Closed account lifetime", "AbortError");
      return result as T;
    } catch (error) {
      if (!this.active || controller.signal.aborted)
        throw new DOMException("Closed account lifetime", "AbortError");
      throw error;
    } finally {
      this.controllers.delete(controller);
      signal?.removeEventListener("abort", abort);
    }
  }

  async stream(
    path: string,
    receive: (frame: StreamFrame) => void,
    signal?: AbortSignal,
    opened?: () => void,
  ): Promise<void> {
    if (!this.active)
      throw new DOMException("Closed account lifetime", "AbortError");
    const controller = new AbortController();
    const abort = () => controller.abort();
    signal?.addEventListener("abort", abort, { once: true });
    if (signal?.aborted) controller.abort();
    this.controllers.add(controller);
    let reader: ReadableStreamDefaultReader<Uint8Array> | undefined;
    const cancelReader = () => {
      void reader?.cancel().catch(() => {});
    };
    controller.signal.addEventListener("abort", cancelReader);
    const check = () => {
      if (!this.active || controller.signal.aborted)
        throw new DOMException("Closed account lifetime", "AbortError");
    };
    try {
      check();
      const response = await fetch(`/api/v1${path}`, {
        credentials: "same-origin",
        cache: "no-store",
        signal: controller.signal,
        headers: {
          Accept: "text/event-stream",
          ...(this.expectedAccountId
            ? { "X-Moosuri-Account": this.expectedAccountId }
            : {}),
        },
      });
      check();
      if (!response.ok) {
        const value = await response.json().catch(() => ({}));
        check();
        throw new ApiFailure(
          response.status,
          typeof value.code === "string" ? value.code : "request_failed",
        );
      }
      if (
        !response.headers
          .get("Content-Type")
          ?.toLowerCase()
          .startsWith("text/event-stream") ||
        !response.body
      )
        throw new StreamFailure();
      reader = response.body.getReader();
      // A quiet authenticated stream is already connected before any model data.
      check();
      opened?.();
      const decoder = new SseDecoder();
      while (true) {
        const result = await reader.read();
        check();
        if (result.done) {
          decoder.finish();
          return;
        }
        for (const frame of decoder.push(result.value)) {
          check();
          receive(frame);
        }
      }
    } catch (error) {
      check();
      throw error;
    } finally {
      controller.signal.removeEventListener("abort", cancelReader);
      controller.abort();
      void reader?.cancel().catch(() => {});
      this.controllers.delete(controller);
      signal?.removeEventListener("abort", abort);
    }
  }
}
export class ApiFailure extends Error {
  constructor(
    public status: number,
    public code: string,
  ) {
    super(code);
  }
}
export function isAborted(error: unknown) {
  return error instanceof DOMException && error.name === "AbortError";
}
export function errorText(error: unknown): string {
  if (!(error instanceof ApiFailure))
    return "Could not reach moosuri. Check your connection and try again.";
  if (error.status === 401)
    return "Your session has ended. Please sign in again.";
  if (error.status === 403)
    return "This action is not available for your account.";
  if (error.status === 429)
    return "Too many attempts. Please wait a moment and try again.";
  if (error.code === "invalid_credentials")
    return "The email or password is incorrect, or the invitation has expired.";
  if (error.code === "password_too_short" || error.code === "invalid_password")
    return "Choose a password with at least 12 characters.";
  if (error.code === "account_exists" || error.code === "invalid_account")
    return "Could not create this account. Check the details and try again.";
  return "The action could not be completed. Please try again.";
}
import { SseDecoder, type StreamFrame, StreamFailure } from "./event-stream";
