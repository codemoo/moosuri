import { StrictMode } from "react";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import PersonalGoogle from "./PersonalGoogle";
import type { SessionView } from "../shared/api-types";

const key = "moosuri.personal.google-read.v1",
  flow = "a".repeat(64),
  handle = "b".repeat(64);
const session = {
  user: { id: "member-a" },
  chat_draft_scope: "session-a",
} as SessionView;
const grant = {
  connection_id: "e8cb4767-2688-43ab-9ece-76ad780360aa",
  epoch: 3,
  mailbox: "member@example.test",
};
function setup(url: string, binding = session.chat_draft_scope, lost = false) {
  const calls: { path: string; body: unknown }[] = [];
  sessionStorage.setItem(
    key,
    JSON.stringify({
      flow,
      account: session.user.id,
      session: binding,
      read: true,
      expires: Date.now() + 60000,
    }),
  );
  history.replaceState(null, "", "/#" + url);
  vi.stubGlobal(
    "fetch",
    vi.fn(async (input: string, init: RequestInit) => {
      calls.push({
        path: input,
        body: init?.body ? JSON.parse(String(init.body)) : null,
      });
      if (input.endsWith("/complete")) {
        if (lost) throw new TypeError("lost response");
        return Response.json({ ...grant, flow_id: flow });
      }
      if (input.endsWith("/inbox"))
        return Response.json({
          mailbox: grant.mailbox,
          messages: [],
          has_more: false,
        });
      return Response.json({ connection: null });
    }),
  );
  return calls;
}
function mount() {
  return render(
    <StrictMode>
      <PersonalGoogle session={session} expired={() => {}} />
    </StrictMode>,
  );
}
afterEach(() => {
  cleanup();
  sessionStorage.clear();
  history.replaceState(null, "", "/");
  vi.unstubAllGlobals();
});
it("completes once in the originating tab and reads only the returned grant", async () => {
  const calls = setup("google_handle=" + handle + "&flow=" + flow);
  mount();
  await screen.findByText("받은 메일이 없습니다.");
  expect(calls.map((c) => c.path)).toEqual([
    "/api/v1/personal/google/complete",
    "/api/v1/personal/google/inbox",
  ]);
  expect(calls[1].body).toEqual({
    connection_id: grant.connection_id,
    epoch: 3,
  });
  expect(location.hash).toBe("");
  expect(sessionStorage.getItem(key)).toBeNull();
});
it("a different session cannot exchange a returned handle", async () => {
  const calls = setup(
    "google_handle=" + handle + "&flow=" + flow,
    "retired-session",
  );
  mount();
  await screen.findByText(/연결을 시작한 계정/);
  expect(calls.every((c) => c.path.endsWith("/google"))).toBe(true);
});
it("denial clears the pending read and never calls the provider", async () => {
  const calls = setup("google_denied=" + flow);
  mount();
  await screen.findByText(/취소됐습니다/);
  expect(calls.map((c) => c.path)).toEqual(["/api/v1/personal/google"]);
  expect(sessionStorage.getItem(key)).toBeNull();
});
it("a lost completion reply cannot silently replay an exchange or read", async () => {
  const calls = setup(
    "google_handle=" + handle + "&flow=" + flow,
    session.chat_draft_scope,
    true,
  );
  const first = mount();
  await screen.findByText(/다시 확인해 주세요/);
  first.unmount();
  mount();
  await waitFor(() => expect(calls).toHaveLength(2));
  expect(calls.map((c) => c.path)).toEqual([
    "/api/v1/personal/google/complete",
    "/api/v1/personal/google",
  ]);
});
