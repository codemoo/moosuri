import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import { it, expect, vi } from "vitest";
const ids = {
  v: 1,
  notification_id: "10000000-0000-4000-8000-000000000001",
  binding_id: "20000000-0000-4000-8000-000000000001",
};
const code = ts
  .transpileModule(readFileSync(resolve("service-worker.ts"), "utf8"), {
    compilerOptions: {
      target: ts.ScriptTarget.ES2022,
      module: ts.ModuleKind.ESNext,
    },
  })
  .outputText.replace(/^import .*push-state.*\n/m, "")
  .replace(/^import .*share-intake.*\n/m, "")
  .replace(/^export \{\};?\s*$/m, "");
function fixture() {
  const events = new Map<string, (e: any) => void>();
  let row: any = {
    device_id: "device",
    account_id: "a",
    binding_id: ids.binding_id,
    generation: "one",
  };
  const show = vi.fn(async (..._args: any[]) => {});
  const network = vi.fn(
    async (..._args: any[]) => new Response(null, { status: 204 }),
  );
  const post = vi.fn();
  const focus = vi.fn();
  const open = vi.fn();
  let windows: any[] = [
    { url: "https://push.example.test/app", postMessage: post, focus },
  ];
  let notices: any[] = [];
  const clear = vi.fn(async () => {
    row = null;
  });
  runInNewContext(code, {
    MOOSURI_SHELL_VERSION: "test",
    MOOSURI_SHELL_ASSETS: [],
    readPushBinding: async () => row,
    clearPushBinding: clear,
    fetch: network,
    URL,
    Response,
    self: {
      location: new URL("https://push.example.test"),
      registration: {
        showNotification: show,
        getNotifications: async () => notices,
      },
      clients: { matchAll: async () => windows, openWindow: open },
      addEventListener: (type: string, fn: (e: any) => void) =>
        events.set(type, fn),
    },
  });
  async function event(type: string, extra: any) {
    let pending: Promise<unknown> | undefined;
    events.get(type)!({
      ...extra,
      waitUntil: (p: Promise<unknown>) => (pending = p),
    });
    await pending;
  }
  return {
    show,
    network,
    post,
    focus,
    open,
    clear,
    event,
    setRow: (r: any) => {
      row = r;
    },
    setWindows: (v: any[]) => {
      windows = v;
    },
    setNotices: (v: any[]) => {
      notices = v;
    },
    push: (payload: any) =>
      event("push", { data: { text: () => JSON.stringify(payload) } }),
  };
}
it("shows only fixed generic text and stable replacement identity, without private body/cache", async () => {
  const f = fixture();
  await f.push({
    ...ids,
    title: "PRIVATE TITLE",
    body: "PRIVATE BODY",
    route: "https://foreign.invalid",
  });
  await f.push(ids);
  expect(f.show).toHaveBeenCalledTimes(2);
  const [title, options] = f.show.mock.calls[0] as unknown as [string, any];
  expect(title).toBe("moosuri");
  expect(JSON.stringify(options)).not.toContain("PRIVATE");
  expect(options.tag).toBe(f.show.mock.calls[1][1]?.tag);
  expect(f.network.mock.calls[0][0]).toBe(
    "/api/v1/notifications/push/authorize",
  );
  expect(f.network.mock.calls[0][1]).toMatchObject({
    cache: "no-store",
    redirect: "error",
    credentials: "same-origin",
  });
});
it("a delayed authorization response cannot display A after B takes the binding", async () => {
  const f = fixture();
  let release!: (r: Response) => void;
  f.network.mockImplementation(
    () => new Promise((resolve) => (release = resolve)),
  );
  const pending = f.push(ids);
  await vi.waitFor(() => expect(f.network).toHaveBeenCalledOnce());
  f.setRow({ account_id: "b", binding_id: "new", generation: "two" });
  release(new Response(null, { status: 204 }));
  await pending;
  expect(f.show).not.toHaveBeenCalled();
});
it("offline/revoked authorization and malformed push data produce no display", async () => {
  const f = fixture();
  f.network.mockRejectedValue(new TypeError("offline"));
  await f.push(ids);
  f.network.mockResolvedValue(new Response(null, { status: 401 }));
  await f.push(ids);
  await f.push({ ...ids, binding_id: "not-a-uuid" });
  expect(f.show).not.toHaveBeenCalled();
});
it("click focuses an existing app through a fixed message and never reloads its draft", async () => {
  const f = fixture();
  const close = vi.fn();
  await f.event("notificationclick", {
    notification: { close, data: { ...ids, route: "https://foreign.invalid" } },
  });
  expect(close).toHaveBeenCalled();
  expect(f.post).toHaveBeenCalledWith({ type: "moosuri-open-schedules" });
  expect(f.focus).toHaveBeenCalled();
  expect(f.open).not.toHaveBeenCalled();
  f.setWindows([]);
  await f.event("notificationclick", { notification: { close, data: ids } });
  expect(f.open).toHaveBeenCalledWith("/app/schedules");
});
it("an old account cleanup cannot close B's generic notice", async () => {
  const f = fixture();
  const a = vi.fn(),
    b = vi.fn();
  f.setNotices([
    { tag: "moosuri-push-a", data: { account_id: "a" }, close: a },
    { tag: "moosuri-push-b", data: { account_id: "b" }, close: b },
  ]);
  await f.event("message", {
    data: { type: "moosuri-push-clear", expected_account: "a" },
  });
  expect(a).toHaveBeenCalled();
  expect(b).not.toHaveBeenCalled();
});
