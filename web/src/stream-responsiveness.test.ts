import { afterEach, describe, expect, it, vi } from "vitest";
import { ApiFailure, RequestScope, isAborted } from "./api";
import { StreamFailure } from "./event-stream";

afterEach(() => vi.useRealTimers());

describe("bounded chat transport", () => {
  it("bounds connecting even when fetch ignores cancellation and discards its late body", async () => {
    vi.useFakeTimers();
    let respond!: (response: Response) => void;
    let signal!: AbortSignal;
    vi.stubGlobal("fetch", vi.fn((_url, init) => {
      signal = init.signal;
      return new Promise<Response>(resolve => { respond = resolve; });
    }));
    const scope = new RequestScope("synthetic-account");
    const observed = scope.stream("/synthetic/events", vi.fn()).catch(error => error);
    await vi.advanceTimersByTimeAsync(15000);
    expect(await observed).toBeInstanceOf(StreamFailure);
    expect(signal.aborted).toBe(true);
    const cancel = vi.fn();
    respond(new Response(new ReadableStream({ cancel }), { headers: { "Content-Type": "text/event-stream" } }));
    await vi.advanceTimersByTimeAsync(0);
    expect(cancel).toHaveBeenCalledOnce();
    expect(vi.getTimerCount()).toBe(0);
    scope.close();
  });

  it("keeps idle comment heartbeats alive and cancels a silent half-open stream", async () => {
    vi.useFakeTimers();
    let body!: ReadableStreamDefaultController<Uint8Array>;
    const cancel = vi.fn();
    vi.stubGlobal("fetch", vi.fn(async () => new Response(new ReadableStream<Uint8Array>({
      start(controller) { body = controller; }, cancel,
    }), { headers: { "Content-Type": "text/event-stream" } })));
    const receive = vi.fn();
    const opened = vi.fn();
    const scope = new RequestScope("synthetic-account");
    const observed = scope.stream("/synthetic/events", receive, undefined, opened).catch(error => error);
    await vi.advanceTimersByTimeAsync(0);
    expect(opened).toHaveBeenCalledOnce();
    for (let heartbeat = 0; heartbeat < 5; heartbeat++) {
      await vi.advanceTimersByTimeAsync(15000);
      body.enqueue(new TextEncoder().encode(": keepalive\n\n"));
      await vi.advanceTimersByTimeAsync(0);
    }
    expect(receive).not.toHaveBeenCalled();
    expect(cancel).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(45000);
    expect(await observed).toBeInstanceOf(StreamFailure);
    expect(cancel).toHaveBeenCalledOnce();
    expect(vi.getTimerCount()).toBe(0);
    scope.close();
  });

  it("account closure promptly settles a connecting transport that ignores abort", async () => {
    vi.useFakeTimers();
    vi.stubGlobal("fetch", vi.fn(() => new Promise<Response>(() => {})));
    const scope = new RequestScope("synthetic-account");
    const observed = scope.stream("/synthetic/events", vi.fn()).catch(error => error);
    scope.close();
    expect(isAborted(await observed)).toBe(true);
    expect(vi.getTimerCount()).toBe(0);
  });
});


describe("bounded read-only snapshot requests", () => {
  it("bounds GET connecting and cancels an abort-ignoring late response", async () => {
    vi.useFakeTimers();
    let respond!: (response: Response) => void;
    let signal!: AbortSignal;
    vi.stubGlobal("fetch", vi.fn((_url, init) => {
      signal = init.signal;
      return new Promise<Response>(resolve => { respond = resolve; });
    }));
    const scope = new RequestScope("synthetic-account");
    const observed = scope.request("/synthetic/messages").catch(error => error);
    await vi.advanceTimersByTimeAsync(30000);
    const error = await observed;
    expect(error).toBeInstanceOf(ApiFailure);
    expect(error).toMatchObject({ status: 504 });
    expect(isAborted(error)).toBe(false);
    expect(signal.aborted).toBe(true);
    const cancel = vi.fn();
    respond(new Response(new ReadableStream({ cancel })));
    await vi.advanceTimersByTimeAsync(0);
    expect(cancel).toHaveBeenCalledOnce();
    expect(vi.getTimerCount()).toBe(0);
    scope.close();
  });

  it("uses one total GET deadline across connecting and a stalled JSON body", async () => {
    vi.useFakeTimers();
    let respond!: (response: Response) => void;
    const cancel = vi.fn();
    vi.stubGlobal("fetch", vi.fn(() => new Promise<Response>(resolve => { respond = resolve; })));
    const scope = new RequestScope("synthetic-account");
    let settled = false;
    const observed = scope.request("/synthetic/messages").catch(error => error).finally(() => { settled = true; });
    await vi.advanceTimersByTimeAsync(20000);
    respond(new Response(new ReadableStream<Uint8Array>({
      start(controller) { controller.enqueue(new TextEncoder().encode('{"turns":[')); }, cancel,
    })));
    await vi.advanceTimersByTimeAsync(9999);
    expect(settled).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    expect(await observed).toMatchObject({ status: 504, code: "request_timed_out" });
    expect(cancel).toHaveBeenCalledOnce();
    expect(vi.getTimerCount()).toBe(0);
    scope.close();
  });

  it.each(["connecting", "body"])("account close promptly settles an abort-ignoring GET at %s", async stage => {
    vi.useFakeTimers();
    const cancel = vi.fn();
    vi.stubGlobal("fetch", vi.fn(() => stage === "connecting"
      ? new Promise<Response>(() => {})
      : Promise.resolve(new Response(new ReadableStream({ cancel })))));
    const scope = new RequestScope("synthetic-account");
    const observed = scope.request("/synthetic/messages").catch(error => error);
    await vi.advanceTimersByTimeAsync(0);
    scope.close();
    expect(isAborted(await observed)).toBe(true);
    if (stage === "body") expect(cancel).toHaveBeenCalledOnce();
    expect(vi.getTimerCount()).toBe(0);
  });

  it("leaves mutation timing and dispatch unchanged", async () => {
    vi.useFakeTimers();
    let respond!: (response: Response) => void;
    const fetch = vi.fn(() => new Promise<Response>(resolve => { respond = resolve; }));
    vi.stubGlobal("fetch", fetch);
    const scope = new RequestScope("synthetic-account");
    let settled = false;
    const observed = scope.request("/synthetic/messages", "POST", { request_id: "synthetic-id" }).finally(() => { settled = true; });
    await vi.advanceTimersByTimeAsync(45000);
    expect(settled).toBe(false);
    expect(fetch).toHaveBeenCalledOnce();
    respond(new Response('{"accepted":true}'));
    expect(await observed).toEqual({ accepted: true });
    expect(vi.getTimerCount()).toBe(0);
    scope.close();
  });
});
