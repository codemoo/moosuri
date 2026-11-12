/** An account lifetime owns every request. Closing it aborts transport and fences
 * responses even when a transport/server ignores AbortSignal. No private cache. */
export class RequestScope {
  constructor(private readonly expectedAccountId?: string) {}
  private active = true;
  private controllers = new Set<AbortController>();
  close() {
    this.active = false;
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
