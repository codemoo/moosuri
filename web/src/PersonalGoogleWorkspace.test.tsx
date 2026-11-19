import { StrictMode } from "react";
import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import type { SessionView } from "../shared/api-types";
import { PersonalConnections } from "./PersonalConnections";

const key = "moosuri.personal.google-workspace.v1";
const gmailKey = "moosuri.personal.google-read.v1";
const base = "/api/v1/personal/google";
const flow = "a".repeat(64);
const handle = "b".repeat(64);
const session = { user: { id: "member-a" }, chat_draft_scope: "session-a" } as SessionView;
const docs = { service: "docs", connection_id: "docs-id", epoch: 3, mailbox: "docs@example.test" };
const sheets = { service: "sheets", connection_id: "sheets-id", epoch: 5, mailbox: "sheets@example.test" };
const gmail = { connection_id: "gmail-id", epoch: 2, mailbox: "gmail@example.test" };

function setup({ grants = [docs, sheets], workspaceStatus = 200, loseCompletion = false } = {}) {
  const calls: { path: string; method: string; body: unknown }[] = [];
  vi.stubGlobal("fetch", vi.fn(async (path: string, init: RequestInit) => {
    const method = init.method ?? "GET";
    const body = init.body ? JSON.parse(String(init.body)) : undefined;
    if (path === "/api/v1/personal/vault") return Response.json({ state: "setup_required" });
    calls.push({ path, method, body });
    expect(new Headers(init.headers).get("X-Moosuri-Account")).toBe(session.user.id);
    if (path === base) return Response.json({ connection: gmail });
    if (path === base + "/workspace") return Response.json({ connections: grants }, { status: workspaceStatus });
    if (path === base + "/docs/disconnect") {
      grants = grants.filter((grant) => grant.service !== "docs");
      return new Response(null, { status: 204 });
    }
    if (path === base + "/drive/start") return Response.json({ flow_id: flow, authorization_url: "https://accounts.google.com/o/oauth2/v2/auth?state=synthetic" });
    if (path === base + "/docs/complete") {
      grants = [docs, ...grants.filter((grant) => grant.service !== "docs")];
      if (loseCompletion) throw new TypeError("lost reply");
      return Response.json({ connection_id: docs.connection_id, epoch: docs.epoch, mailbox: docs.mailbox, flow_id: flow });
    }
    throw Error("Unexpected request");
  }));
  return calls;
}

function storePending(overrides = {}) {
  const pending = {
    account: session.user.id, session: session.chat_draft_scope, service: "docs", flow,
    expires: Date.now() + 60000, phase: "consent", ...overrides,
  };
  sessionStorage.setItem(key, JSON.stringify(pending));
  return pending;
}

function mount(installationAdmin = false) {
  const returned = vi.fn();
  const openGoogleSetup = installationAdmin ? vi.fn() : undefined;
  return { returned, openGoogleSetup, ...render(<StrictMode><PersonalConnections
    openGoogleSetup={openGoogleSetup} session={session} expired={vi.fn()} onBack={vi.fn()} returned={returned}
  /></StrictMode>) };
}

afterEach(() => {
  cleanup();
  sessionStorage.clear();
  history.replaceState(null, "", "/");
  vi.unstubAllGlobals();
});

it("searches the four branded connectors and lists only their actual connected accounts", async () => {
  const calls = setup();
  const user = userEvent.setup();
  mount();
  const search = screen.getByRole("searchbox", { name: "커넥터 검색" });
  for (const [alias, name] of [
    ["구글드라이브", "Google Drive"], ["구글 문서", "Google Docs"],
    ["구글 슬라이드", "Google Slides"], ["Google Spreadsheets", "Google Sheets"],
  ]) {
    await user.clear(search);
    await user.type(search, alias);
    const service = screen.getByRole("button", { name: new RegExp(name) });
    expect(service.querySelector("img")?.getAttribute("src")).toContain(`${name.split(" ")[1].toLowerCase()}.png`);
  }
  await user.clear(search);
  await user.click(screen.getByRole("button", { name: "연결됨" }));
  expect(await screen.findByRole("button", { name: /Google Docs docs@example.test/ })).toBeVisible();
  expect(screen.getByRole("button", { name: /Google Sheets sheets@example.test/ })).toBeVisible();
  expect(screen.queryByRole("button", { name: /Google (Drive|Slides)/ })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: /Google Docs/ }));
  const detail = within(screen.getByRole("region", { name: "Google Docs" }));
  expect(detail.getByRole("status")).toHaveTextContent("연결됨");
  await user.click(screen.getByRole("button", { name: "연결 해제" }));
  await detail.findByText("연결된 계정이 없습니다.");
  await user.click(screen.getByRole("button", { name: "뒤로" }));
  expect(screen.queryByRole("button", { name: /Google Docs/ })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Google Sheets sheets@example.test/ })).toBeVisible();
  expect(calls.filter(({ method }) => method === "POST")).toEqual([{
    path: base + "/docs/disconnect", method: "POST", body: { connection_id: docs.connection_id, epoch: docs.epoch },
  }]);
});

it("starts consent only for the selected service and binds its flow to the current account and session", async () => {
  const calls = setup();
  const user = userEvent.setup();
  const assign = vi.fn();
  vi.stubGlobal("location", { hash: "", assign });
  mount();
  await user.click(await screen.findByRole("button", { name: /Google Drive/ }));
  await user.click(await screen.findByRole("button", { name: "Google 연결" }));
  await waitFor(() => expect(JSON.parse(sessionStorage.getItem(key) ?? "null")).toMatchObject({
    account: session.user.id, session: session.chat_draft_scope, service: "drive", flow, phase: "consent",
  }));
  expect(calls.filter(({ method }) => method === "POST")).toEqual([{ path: base + "/drive/start", method: "POST", body: {} }]);
  expect(assign).toHaveBeenCalledOnce();
  expect(assign).toHaveBeenCalledWith("https://accounts.google.com/o/oauth2/v2/auth?state=synthetic");
  expect(sessionStorage.getItem(gmailKey)).toBeNull();
});

it.each([false, true])("finishes a Docs callback once and reconciles status when the reply is lost (%s)", async (loseCompletion) => {
  const calls = setup({ grants: [sheets], loseCompletion });
  const pending = storePending();
  sessionStorage.setItem(gmailKey, JSON.stringify({
    ...pending, read: { thread_id: "unrelated-thread", request_id: "unrelated-request" }, phase: "consent",
  }));
  history.replaceState(null, "", `/#google_service=docs&google_handle=${handle}&flow=${flow}`);
  const { returned, unmount } = mount();
  const detail = within(screen.getByRole("region", { name: "Google Docs" }));
  await detail.findByText(docs.mailbox);
  if (loseCompletion) expect(screen.getByText(/연결 결과를 확인하지 못했습니다/)).toBeVisible();
  expect(location.hash).toBe("");
  expect(sessionStorage.getItem(key)).toBeNull();
  expect(sessionStorage.getItem(gmailKey)).not.toBeNull();
  expect(returned).not.toHaveBeenCalled();
  unmount();
  mount();
  await screen.findByRole("button", { name: /Google Docs docs@example.test/ });
  expect(calls.filter(({ method }) => method === "POST")).toEqual([{ path: base + "/docs/complete", method: "POST", body: { handle } }]);
});

it.each([{ service: "sheets" }, { session: "different-session" }, { flow: "c".repeat(64) }])(
  "rejects a callback that does not match the pending service, session, or flow (%j)", async (overrides) => {
    const calls = setup();
    storePending(overrides);
    history.replaceState(null, "", `/#google_service=docs&google_handle=${handle}&flow=${flow}`);
    mount();
    await screen.findByText(/연결을 시작한 계정/);
    expect(location.hash).toBe("");
    expect(sessionStorage.getItem(key)).not.toBeNull();
    expect(calls.every(({ method }) => method === "GET")).toBe(true);
  },
);

it.each([`/#google_service=docs&google_denied=${flow}`, "/"])("checks status on denial or interrupted completion (%s)", async (url) => {
  const calls = setup({ grants: [sheets] });
  storePending({ phase: url === "/" ? "completing" : "consent" });
  history.replaceState(null, "", url);
  mount();
  expect(screen.getByRole("heading", { name: "Google Docs" })).toBeVisible();
  await screen.findByText(url === "/" ? /연결 결과를 확인하지 못했습니다/ : /연결이 취소됐습니다/);
  await screen.findByRole("button", { name: "Google 연결" });
  expect(location.hash).toBe("");
  expect(sessionStorage.getItem(key)).toBeNull();
  expect(calls.every(({ method }) => method === "GET")).toBe(true);
});

it.each([404, 503])("shows member-safe Workspace status without changing Gmail availability (%s)", async (workspaceStatus) => {
  const calls = setup({ workspaceStatus });
  const user = userEvent.setup();
  mount();
  await user.click(await screen.findByRole("button", { name: /Google Slides/ }));
  const detail = within(screen.getByRole("region", { name: "Google Slides" }));
  await detail.findByText(workspaceStatus === 404 ? /이 서버에서 Google 로그인을 아직 사용할 수 없습니다/ : /연결 상태를 확인할 수 없습니다/);
  expect(screen.queryByRole("button", { name: "Google 연결" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Google 설치 설정 열기" })).not.toBeInTheDocument();
  expect(await screen.findByRole("button", { name: "다시 확인" })).toBeEnabled();
  await user.click(screen.getByRole("button", { name: "뒤로" }));
  await user.click(await screen.findByRole("button", { name: /Gmail gmail@example.test/ }));
  expect(await screen.findByRole("button", { name: "연결 해제" })).toBeEnabled();
  expect(calls.every(({ method }) => method === "GET")).toBe(true);
});

it("links the installation administrator to Google web sign-in setup when unavailable", async () => {
  setup({ workspaceStatus: 404 });
  const { openGoogleSetup } = mount(true);
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: /Google Slides/ }));
  await user.click(await screen.findByRole("button", { name: "Google 설치 설정 열기" }));
  expect(openGoogleSetup).toHaveBeenCalledOnce();
});
