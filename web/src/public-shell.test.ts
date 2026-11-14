import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import { expect, it, vi } from "vitest";

const origin = "https://moosuri.example.test";
const assets = ["/index.html", "/assets/app-fixed.js", "/assets/app-fixed.css", "/manifest.webmanifest", "/icon.svg"];
const code = ts.transpileModule(readFileSync(resolve("service-worker.ts"), "utf8"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
}).outputText.replace(/^import .*push-state.*\n/m, "").replace(/^import .*share-intake.*\n/m, "").replace(/^export \{\};?\s*$/m, "");

function worker() {
  const events = new Map<string, (e: any) => void>();
  const entries = new Map<string, Map<string, Response>>();
  const network = vi.fn(async (request: Request) => new Response(request.url.endsWith("/index.html") ? "public-shell" : "public-asset"));
  const cache = (name: string) => {
    if (!entries.has(name)) entries.set(name, new Map());
    const data = entries.get(name)!;
    return {
      addAll: async (requests: Request[]) => {
        for (const request of requests) data.set(new URL(request.url).pathname, await network(request));
      },
      match: async (path: string) => data.get(path)?.clone(),
    };
  };
  const claim = vi.fn(async () => {});
  const skipWaiting = vi.fn();
  class WorkerRequest extends Request {
    constructor(input: string | Request, init?: RequestInit) {
      super(typeof input === "string" ? new URL(input, origin) : input, init);
    }
  }
  runInNewContext(code, {
    MOOSURI_SHELL_VERSION: "test-revision", MOOSURI_SHELL_ASSETS: assets,
    self: { location: new URL(origin), clients: { claim }, skipWaiting, addEventListener: (name: string, fn: (e: any) => void) => events.set(name, fn) },
    caches: { open: async (name: string) => cache(name), keys: async () => [...entries.keys()], delete: async (name: string) => entries.delete(name) },
    Request: WorkerRequest, Response, URL, fetch: network,
  });
  const lifecycle = async (name: string) => {
    let pending: Promise<void> | undefined;
    events.get(name)!({ waitUntil: (promise: Promise<void>) => { pending = promise; } });
    await pending;
  };
  const fetchEvent = (path: string, mode = "cors", method = "GET") => {
    let response: Promise<Response> | undefined;
    events.get("fetch")!({ request: { url: new URL(path, origin).href, mode, method }, respondWith: (value: Promise<Response>) => { response = value; } });
    return response;
  };
  return { entries, network, claim, skipWaiting, lifecycle, fetchEvent };
}

it("installs only an anonymous public build and never forces a tab update", async () => {
  const w = worker();
  await w.lifecycle("install");
  expect(w.network.mock.calls.map(([request]) => new URL(request.url).pathname)).toEqual(assets);
  for (const [request] of w.network.mock.calls) {
    expect(request.credentials).toBe("omit");
    expect(request.redirect).toBe("error");
  }
  expect(w.skipWaiting).not.toHaveBeenCalled();
});

it("keeps API, private files, screenshots, shares, foreign origins and mutations out of caching", async () => {
  const w = worker(); await w.lifecycle("install");
  for (const path of ["/api/v1/auth/session", "/api/v1/schedules", "/api/v1/notifications", "/api/v1/threads", "/api/v1/browser/frame", "/files/private.png", "/share", "/assets/unknown.js", "/assets/app-fixed.js?private=1", "https://other.example.test/index.html"]) {
    expect(w.fetchEvent(path)).toBeUndefined();
    expect(w.fetchEvent(path, "navigate")).toBeUndefined();
  }
  expect(w.fetchEvent("/app", "navigate", "POST")).toBeUndefined();
  expect(w.fetchEvent("/index.html", "cors", "POST")).toBeUndefined();
  expect(w.entries.get("moosuri-public-shell-test-revision")?.size).toBe(assets.length);
});

it("online navigation never stores a credential-bearing response; offline uses only public shell", async () => {
  const w = worker(); await w.lifecycle("install");
  w.network.mockImplementation(async () => new Response("PRIVATE-CANARY"));
  expect(await (await w.fetchEvent("/app?thread=opaque", "navigate")!).text()).toBe("PRIVATE-CANARY");
  w.network.mockRejectedValue(new TypeError("offline"));
  expect(await (await w.fetchEvent("/app?thread=opaque", "navigate")!).text()).toBe("public-shell");
  expect(await (await w.fetchEvent("/app/schedules", "navigate")!).text()).toBe("public-shell");
  expect(await (await w.fetchEvent("/app/missions", "navigate")!).text()).toBe("public-shell");
  expect(await (await w.fetchEvent("/settings/calendar", "navigate")!).text()).toBe("public-shell");
  expect(await (await w.fetchEvent("/assets/app-fixed.js")!).text()).toBe("public-asset");
  expect([...w.entries.values()].every(data => !data.has("/app"))).toBe(true);
});

it("failed installation removes partial shell; activation only removes this app's older public builds", async () => {
  const w = worker();
  w.entries.set("other-app-cache", new Map());
  w.entries.set("moosuri-public-shell-old", new Map());
  w.network.mockRejectedValueOnce(new TypeError("interrupted"));
  await expect(w.lifecycle("install")).rejects.toThrow("interrupted");
  expect(w.entries.has("moosuri-public-shell-test-revision")).toBe(false);
  await w.lifecycle("install");
  await w.lifecycle("activate");
  expect([...w.entries.keys()].sort()).toEqual(["moosuri-public-shell-test-revision", "other-app-cache"]);
  expect(w.claim).toHaveBeenCalledOnce();
});
