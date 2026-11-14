import { beforeEach, expect, it, vi } from "vitest";
import { IDBFactory } from "fake-indexeddb";
import { boundedBody, parseShare, receiveShare } from "./share-intake";
import { bindShareSession, fenceIncomingShares, readShareRow } from "./share-state";
const scope = "a".repeat(64);
const session = () => new Response(JSON.stringify({ user: { id: "synthetic-a", must_change_password: false }, password_change_required: false, chat_draft_scope: scope }));
function request(fields: [string, string | Blob][]) {
  const form = new FormData(); for (const [k, v] of fields) form.append(k, v);
  return new Request("https://example.test/share-target", { method: "POST", body: form });
}
beforeEach(async () => { vi.stubGlobal("indexedDB", new IDBFactory()); await fenceIncomingShares(); });
it("bounded multipart keeps HTML and URL inert; redirect contains only opaque ID and fresh auth is the only fetch", async () => {
  const fetch = vi.fn(async (..._args: unknown[]) => session()); vi.stubGlobal("fetch", fetch);
  const response = await receiveShare(request([["title", "Synthetic"], ["text", "<b>inert</b>"], ["url", "https://example.invalid/no-fetch"]]));
  expect(response.status).toBe(303);
  expect(response.headers.get("Location")).toMatch(/^\/app#share=[0-9a-f-]{36}$/);
  expect(response.headers.get("Cache-Control")).toBe("no-store");
  expect(fetch).toHaveBeenCalledTimes(1);
  expect(fetch.mock.calls[0][0]).toBe("/api/v1/auth/session");
  expect((await readShareRow()).pending?.payload?.text).toBe("<b>inert</b>");
});
it.each(([
  [["text", "one"], ["text", "duplicate"]],
  [["unknown", "private"]],
  [["text", new Blob(["file"], { type: "text/plain" })]],
  [["text", "한".repeat(2731)]],
  [["title", "a".repeat(8192)], ["text", "b"]],
  [["text", "x".repeat(33000)]],
] as [string, string | Blob][][]).map(fields => ({ fields })))("rejects duplicate/unknown/file/UTF-8/body-limit multipart %#", async ({ fields }) => {
  const fetch = vi.fn(); vi.stubGlobal("fetch", fetch);
  await expect(parseShare(request(fields))).rejects.toThrow();
  expect(fetch).not.toHaveBeenCalled();
});
it("stream bound cancels before collecting an oversized chunk", async () => {
  const cancel = vi.fn();
  const body = new ReadableStream<Uint8Array>({ start(c) { c.enqueue(new Uint8Array(32769)); }, cancel });
  await expect(boundedBody(body, 32768)).rejects.toThrow();
  expect(cancel).toHaveBeenCalledTimes(1);
});
it("no-account or failed auth cannot stage; known expired intended owner may stage opaquely", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => new Response(null, { status: 401 })));
  expect((await receiveShare(request([["text", "private"]]))).headers.get("Location")).toBe("/app#share-error=login");
  expect((await readShareRow()).pending).toBeUndefined();
  await bindShareSession(await readShareRow(), { account: "synthetic-a", scope });
  expect((await receiveShare(request([["text", "private"]]))).headers.get("Location")).toMatch(/^\/app#share=/);
  vi.stubGlobal("fetch", vi.fn(async () => { throw new Error("offline"); }));
  const before = (await readShareRow()).pending?.id;
  expect((await receiveShare(request([["text", "must not stage"]]))).headers.get("Location")).toBe("/app#share-error=again");
  expect((await readShareRow()).pending?.id).toBe(before);
});
it("intake captured before a logout cannot resurrect a payload after delayed session response", async () => {
  let release!: (r: Response) => void;
  vi.stubGlobal("fetch", vi.fn(() => new Promise<Response>(r => { release = r; })));
  const pending = receiveShare(request([["text", "late private"]]));
  await vi.waitFor(() => expect(release).toBeTypeOf("function"));
  await fenceIncomingShares(true);
  release(session());
  expect((await pending).headers.get("Location")).toBe("/app#share-error=again");
  expect((await readShareRow()).pending).toBeUndefined();
});

it("slow body deadline cancels instead of parsing a truncated multipart prefix", async () => {
  vi.useFakeTimers();
  try {
    const cancel = vi.fn();
    const pending = boundedBody(new ReadableStream<Uint8Array>({ cancel }), 32768);
    const rejected = expect(pending).rejects.toThrow("Body timed out");
    await vi.advanceTimersByTimeAsync(8001);
    await rejected;
    expect(cancel).toHaveBeenCalledTimes(1);
  } finally { vi.useRealTimers(); }
});
it("storage denial fails closed without authenticating or staging", async () => {
  vi.stubGlobal("indexedDB", { open() { throw new Error("denied"); } });
  const fetch = vi.fn(); vi.stubGlobal("fetch", fetch);
  const result = await receiveShare(request([["text", "not persisted"]]));
  expect(result.headers.get("Location")).toBe("/app#share-error=again");
  expect(fetch).not.toHaveBeenCalled();
});

it("a freshly observed password-change restriction destroys the prior pending payload", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => session()));
  await receiveShare(request([["text", "old private share"]]));
  vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({ user: { id: "synthetic-a", must_change_password: true }, password_change_required: true }))));
  expect((await receiveShare(request([["text", "restricted"]]))).headers.get("Location")).toBe("/app#share-error=again");
  expect((await readShareRow()).pending).toBeUndefined();
});
