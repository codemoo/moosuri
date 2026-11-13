/// <reference lib="webworker" />
export {};
declare const self: ServiceWorkerGlobalScope;
declare const MOOSURI_SHELL_VERSION: string;
declare const MOOSURI_SHELL_ASSETS: string[];

// Injected from the actual production build. Never learn cacheable paths from
// requests: account APIs, attachments, screenshots and shares stay network-only.
const cacheName = `moosuri-public-shell-${MOOSURI_SHELL_VERSION}`;
const publicAssets = new Set(MOOSURI_SHELL_ASSETS);
const navigationPaths = new Set([
  "/", "/app", "/login", "/admin", "/admin/providers", "/admin/google", "/admin/usage",
  "/settings/model", "/settings/mail", "/settings/sources", "/settings/usage",
  "/app/knowledge", "/app/memory", "/app/skills", "/app/artifacts",
]);

self.addEventListener("install", (event) => {
  event.waitUntil((async () => {
    const cache = await caches.open(cacheName);
    try {
      // Fetch the anonymous public build, never a credential-bearing page.
      await cache.addAll(MOOSURI_SHELL_ASSETS.map(path => new Request(path, {
        credentials: "omit", cache: "reload", redirect: "error",
      })));
    } catch (error) {
      await caches.delete(cacheName);
      throw error;
    }
    // No skipWaiting: an update cannot discard a running tab's unsent draft.
  })());
});
self.addEventListener("activate", (event) => {
  event.waitUntil((async () => {
    const names = await caches.keys();
    await Promise.all(names.filter(name => name.startsWith("moosuri-public-shell-") && name !== cacheName)
      .map(name => caches.delete(name)));
    await self.clients.claim();
  })());
});
self.addEventListener("fetch", (event) => {
  const request = event.request;
  const url = new URL(request.url);
  if (request.method !== "GET" || url.origin !== self.location.origin) return;
  const artifactPage = /^\/app\/artifacts\/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(url.pathname);
  if (request.mode === "navigate" && (navigationPaths.has(url.pathname) || artifactPage)) {
    event.respondWith((async () => {
      try { return await fetch(request); }
      catch {
        return (await (await caches.open(cacheName)).match("/index.html"))
          ?? new Response("moosuri is offline. Reconnect and try again.", {
            status: 503, headers: { "Content-Type": "text/plain; charset=utf-8", "Cache-Control": "no-store" },
          });
      }
    })());
    return;
  }
  if (!url.search && publicAssets.has(url.pathname)) {
    event.respondWith((async () => (await (await caches.open(cacheName)).match(url.pathname)) ?? fetch(request))());
  }
});
