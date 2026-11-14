import { receiveShare } from "./src/share-intake";
import {
  readPushBinding,
  clearPushBinding,
  type PushBinding,
} from "./src/push-state";
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
  "/",
  "/app",
  "/login",
  "/admin",
  "/admin/providers",
  "/admin/google",
  "/admin/usage",
  "/settings/model",
  "/settings/mail",
  "/settings/calendar",
  "/settings/sources",
  "/settings/usage",
  "/app/knowledge",
  "/app/memory",
  "/app/skills",
  "/app/artifacts",
  "/app/schedules",
  "/app/missions",
]);

self.addEventListener("install", (event) => {
  event.waitUntil(
    (async () => {
      const cache = await caches.open(cacheName);
      try {
        // Fetch the anonymous public build, never a credential-bearing page.
        await cache.addAll(
          MOOSURI_SHELL_ASSETS.map(
            (path) =>
              new Request(path, {
                credentials: "omit",
                cache: "reload",
                redirect: "error",
              }),
          ),
        );
      } catch (error) {
        await caches.delete(cacheName);
        throw error;
      }
      // No skipWaiting: an update cannot discard a running tab's unsent draft.
    })(),
  );
});
self.addEventListener("activate", (event) => {
  event.waitUntil(
    (async () => {
      const names = await caches.keys();
      await Promise.all(
        names
          .filter(
            (name) =>
              name.startsWith("moosuri-public-shell-") && name !== cacheName,
          )
          .map((name) => caches.delete(name)),
      );
      await self.clients.claim();
    })(),
  );
});
self.addEventListener("fetch", (event) => {
  const request = event.request;
  const url = new URL(request.url);
  if (url.origin === self.location.origin && url.pathname === "/share-target" && !url.search &&
    request.method === "POST" && request.mode === "navigate") {
    event.respondWith(receiveShare(request));
    return;
  }
  if (request.method !== "GET" || url.origin !== self.location.origin) return;
  const artifactPage =
    /^\/app\/artifacts\/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(
      url.pathname,
    );
  if (
    request.mode === "navigate" &&
    (navigationPaths.has(url.pathname) || artifactPage)
  ) {
    event.respondWith(
      (async () => {
        try {
          return await fetch(request);
        } catch {
          return (
            (await (await caches.open(cacheName)).match("/index.html")) ??
            new Response("moosuri is offline. Reconnect and try again.", {
              status: 503,
              headers: {
                "Content-Type": "text/plain; charset=utf-8",
                "Cache-Control": "no-store",
              },
            })
          );
        }
      })(),
    );
    return;
  }
  if (!url.search && publicAssets.has(url.pathname)) {
    event.respondWith(
      (async () =>
        (await (await caches.open(cacheName)).match(url.pathname)) ??
        fetch(request))(),
    );
  }
});

// Generic payload only. Authorize against the current cookie/account without
// refreshing session expiry or fetching/caching any private notification body.
type PushIds = { v: number; notification_id: string; binding_id: string };
const uuid = (v: unknown): v is string =>
  typeof v === "string" &&
  /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(v);
async function authorizePush(value: PushIds): Promise<PushBinding | null> {
  try {
    const binding = await readPushBinding();
    if (!binding?.account_id || binding.binding_id !== value.binding_id)
      return null;
    const response = await fetch("/api/v1/notifications/push/authorize", {
      method: "POST",
      credentials: "same-origin",
      cache: "no-store",
      redirect: "error",
      headers: {
        "Content-Type": "application/json",
        "X-Moosuri-Account": binding.account_id,
      },
      body: JSON.stringify({
        binding_id: value.binding_id,
        notification_id: value.notification_id,
      }),
    });
    if (response.status !== 204) return null;
    const current = await readPushBinding();
    return current?.generation === binding.generation &&
      current.binding_id === binding.binding_id &&
      current.account_id === binding.account_id
      ? current
      : null;
  } catch {
    return null;
  }
}
self.addEventListener("push", (event) => {
  event.waitUntil(
    (async () => {
      try {
        const text = event.data?.text();
        if (!text || text.length > 1024) return;
        const value = JSON.parse(text) as PushIds;
        if (
          value.v !== 1 ||
          !uuid(value.notification_id) ||
          !uuid(value.binding_id)
        )
          return;
        const binding = await authorizePush(value);
        if (!binding) return;
        const tag = `moosuri-push-${value.notification_id}`;
        await self.registration.showNotification("moosuri", {
          body: "새 소식이 있습니다. 앱에서 확인하세요.",
          tag,
          data: {
            v: 1,
            notification_id: value.notification_id,
            binding_id: value.binding_id,
            generation: binding.generation,
            account_id: binding.account_id,
          },
        });
        const current = await readPushBinding();
        if (
          current?.generation !== binding.generation ||
          current.binding_id !== binding.binding_id
        )
          for (const notice of await self.registration.getNotifications({
            tag,
          }))
            notice.close();
      } catch {
        /* Push is best effort; the inbox remains authoritative. */
      }
    })(),
  );
});
self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  event.waitUntil(
    (async () => {
      const value = event.notification.data as PushIds | undefined;
      if (
        !value ||
        value.v !== 1 ||
        !uuid(value.notification_id) ||
        !uuid(value.binding_id) ||
        !(await authorizePush(value))
      )
        return;
      const windows = await self.clients.matchAll({
        type: "window",
        includeUncontrolled: true,
      });
      const existing = windows.find(
        (client) => new URL(client.url).origin === self.location.origin,
      );
      if (existing) {
        existing.postMessage({ type: "moosuri-open-schedules" });
        await existing.focus();
      } else await self.clients.openWindow("/app/schedules");
    })(),
  );
});
self.addEventListener("message", (event) => {
  if (event.data?.type !== "moosuri-push-clear") return;
  const expected =
    typeof event.data.expected_account === "string"
      ? event.data.expected_account
      : null;
  event.waitUntil(
    (async () => {
      for (const notice of await self.registration.getNotifications())
        if (
          notice.tag.startsWith("moosuri-push-") &&
          (expected === null || notice.data?.account_id === expected)
        )
          notice.close();
    })(),
  );
});
self.addEventListener("pushsubscriptionchange", (event) => {
  event.waitUntil(
    (async () => {
      await clearPushBinding();
      for (const notice of await self.registration.getNotifications())
        if (notice.tag.startsWith("moosuri-push-")) notice.close();
      for (const client of await self.clients.matchAll({ type: "window" }))
        client.postMessage({ type: "moosuri-push-reconnect" });
    })(),
  );
});
