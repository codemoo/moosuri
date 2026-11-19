import { StrictMode } from "react";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { PersonalConnections } from "./PersonalConnections";
import { googleReturn } from "./PersonalGoogle";
import type { SessionView } from "../shared/api-types";
const key = "moosuri.personal.google-read.v1",
  flow = "a".repeat(64),
  handle = "b".repeat(64);
const session = {
  user: { id: "member-a" },
  chat_draft_scope: "session-a",
} as SessionView;
const read = { thread_id: "original-thread", request_id: "read-request" };
const grant = {
  connection_id: "e8cb4767-2688-43ab-9ece-76ad780360aa",
  epoch: 3,
  mailbox: "member@example.test",
};
function setup({
  manual = false,
  binding = session.chat_draft_scope,
  lost = "",
}: { manual?: boolean; binding?: string; lost?: string } = {}) {
  const calls: { path: string; body: any }[] = [];
  sessionStorage.setItem(
    key,
    JSON.stringify({
      flow,
      account: session.user.id,
      session: binding,
      read: manual ? undefined : read,
      draft: "Original mail request",
      phase: "consent",
      expires: Date.now() + 60000,
    }),
  );
  history.replaceState(null, "", "/#google_handle=" + handle + "&flow=" + flow);
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: string, init: RequestInit) => {
      const body = init?.body ? JSON.parse(String(init.body)) : null;
      if (input.endsWith("/vault")) return Response.json({ state: "setup_required" });
      calls.push({ path: input, body });
      if (input.endsWith("/complete")) {
        if (lost === "complete") throw new TypeError("lost reply");
        return Response.json({
          ...grant,
          flow_id: flow,
          ...(manual ? {} : { continuation: read }),
        });
      }
      if (input.endsWith("/inbox")) {
        if (lost === "read") throw new TypeError("lost read reply");
        return Response.json({
          ...read,
          state: "completed",
          created_at: 1,
          source: { ...grant, app_revision: "app" },
          result: {
            mailbox: grant.mailbox,
            messages: [],
            has_more: false,
            scope: "one_inbox_page_metadata_only",
          },
        });
      }
      return Response.json({ connection: null });
    }),
  );
  return calls;
}
function mount(returned = vi.fn()) {
  return {
    returned,
    ...render(
      <StrictMode>
        <PersonalConnections
          onBack={() => {}}
          session={session}
          expired={() => {}}
          returned={returned}
        />
      </StrictMode>,
    ),
  };
}
afterEach(() => {
  cleanup();
  sessionStorage.clear();
  history.replaceState(null, "", "/");
  vi.unstubAllGlobals();
});
it("returns the durable read to the original thread using only server continuation", async () => {
  const calls = setup();
  const { returned } = mount();
  await waitFor(() =>
    expect(returned).toHaveBeenCalledWith(read, false, "Original mail request"),
  );
  expect(calls.map((c) => c.path)).toEqual([
    "/api/v1/personal/google/complete",
    "/api/v1/personal/google/inbox",
  ]);
  expect(calls[1].body).toEqual({
    connection_id: grant.connection_id,
    epoch: 3,
    read,
  });
  expect(location.hash).toBe("#connections/gmail");
  expect(sessionStorage.getItem(key)).toBeNull();
});
it("manual connection never resumes a read", async () => {
  const calls = setup({ manual: true });
  mount();
  await screen.findByText(grant.mailbox);
  expect(location.hash).toBe("#connections/gmail");
  expect(calls.map((c) => c.path)).toEqual([
    "/api/v1/personal/google/complete",
  ]);
  expect(sessionStorage.getItem(key)).toBeNull();
});
it("a different session cannot exchange the returned handle", async () => {
  const calls = setup({ binding: "old-session" });
  mount();
  await screen.findByText(/연결을 시작한 계정/);
  expect(calls.every((c) => c.path.endsWith("/google"))).toBe(true);
});
it.each(["complete", "read"])(
  "lost %s reply returns to stored history on reload without a second read",
  async (lost) => {
    const calls = setup({ lost });
    const first = mount();
    await waitFor(() =>
      expect(first.returned).toHaveBeenCalledWith(
        read,
        true,
        "Original mail request",
      ),
    );
    first.unmount();
    const second = mount();
    await waitFor(() =>
      expect(second.returned).toHaveBeenCalledWith(
        read,
        true,
        "Original mail request",
      ),
    );
    expect(calls.filter((c) => c.path.endsWith("/complete"))).toHaveLength(1);
    expect(calls.filter((c) => c.path.endsWith("/inbox"))).toHaveLength(
      lost === "read" ? 1 : 0,
    );
    expect(location.hash).toBe("#connections/gmail");
  },
);
it("denial clears pending read without a provider read", async () => {
  const calls = setup();
  history.replaceState(null, "", "/#google_denied=" + flow);
  mount();
  await screen.findByText(/취소됐습니다/);
  expect(calls.map((c) => c.path)).toEqual(["/api/v1/personal/google"]);
  expect(sessionStorage.getItem(key)).toBeNull();
});

it("clears an interrupted manual callback on the next status visit", async () => {
  const calls = setup({ manual: true });
  history.replaceState(null, "", "/");
  const pending = JSON.parse(sessionStorage.getItem(key)!);
  sessionStorage.setItem(key, JSON.stringify({ ...pending, phase: "completing", expires: 1 }));
  expect(googleReturn()).toBe(false);
  sessionStorage.setItem(key, JSON.stringify({ ...pending, phase: "completing" }));
  expect(googleReturn()).toBe(true);
  mount();
  await waitFor(() => expect(sessionStorage.getItem(key)).toBeNull());
  expect(googleReturn()).toBe(false);
  expect(calls.some(call => /\/(inbox|complete)$/.test(call.path))).toBe(false);
});
