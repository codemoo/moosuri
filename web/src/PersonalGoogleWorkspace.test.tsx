import { StrictMode } from "react";
import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import type { SessionView } from "../shared/api-types";
import { PersonalConnections } from "./PersonalConnections";
import { connectInKiri } from "./kiri-google-connection";
import { ApiFailure } from "./api";

vi.mock("./kiri-google-connection", () => ({ connectInKiri: vi.fn() }));

const key = "moosuri.personal.google-workspace.v1";
const gmailKey = "moosuri.personal.google-read.v1";
const base = "/api/v1/personal/google";
const flow = "a".repeat(64);
const handle = "b".repeat(64);
const vaultOrigin = "https://vault.example.test";
const session = { user: { id: "member-a", email: "member@example.test" }, chat_draft_scope: "session-a" } as SessionView;
const docs = { service: "docs", connection_id: "docs-id", epoch: 3, mailbox: "docs@example.test" };
const sheets = { service: "sheets", connection_id: "sheets-id", epoch: 5, mailbox: "sheets@example.test" };
const gmail = { connection_id: "gmail-id", epoch: 2, mailbox: "gmail@example.test" };

function setup({ grants = [docs, sheets], workspaceStatus = 200, loseCompletion = false, configured = true } = {}) {
  const calls: { path: string; method: string; body: unknown }[] = [];
  vi.mocked(connectInKiri).mockResolvedValue({ job:"11111111-1111-4111-8111-111111111111", flow, handle, email: session.user.email });
  vi.stubGlobal("fetch", vi.fn(async (path: string, init: RequestInit) => {
    const method = init.method ?? "GET";
    const body = init.body ? JSON.parse(String(init.body)) : undefined;
    if (path === "/api/v1/personal/vault") return Response.json({ state: "ready", site_origin: vaultOrigin });
    calls.push({ path, method, body });
    expect(new Headers(init.headers).get("X-Moosuri-Account")).toBe(session.user.id);
    if (path === "/api/v1/personal/admin/google-setup") return Response.json({ configured, redirect_uri: "https://example.test/api/v1/mail/oauth/callback" });
    if (path === base) return Response.json({ connection: gmail });
    if (path === base + "/workspace") return Response.json({ connections: grants }, { status: workspaceStatus });
    if (path === base + "/docs/disconnect") {
      grants = grants.filter((grant) => grant.service !== "docs");
      return new Response(null, { status: 204 });
    }
    if (path === base + "/drive/start") return Response.json({ flow_id: flow, authorization_url: "https://accounts.google.com/o/oauth2/v2/auth?state=synthetic" });
    if (path === base + "/drive/complete") {
      const grant = { service: "drive", connection_id: "drive-id", epoch: 1, mailbox: session.user.email };
      grants = [grant, ...grants.filter(({ service }) => service !== "drive")];
      return Response.json({ ...grant, flow_id: flow });
    }
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
  return { returned, ...render(<StrictMode><PersonalConnections
    installationAdmin={installationAdmin} session={session} expired={vi.fn()} onBack={vi.fn()} returned={returned}
  /></StrictMode>) };
}

afterEach(() => {
  cleanup();
  sessionStorage.clear();
  history.replaceState(null, "", "/");
  vi.clearAllMocks();
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
  expect(await screen.findByRole("button", { name: /Google Docs docs@example.test 연결됨/ })).toBeVisible();
  expect(screen.getByRole("button", { name: /Google Sheets sheets@example.test/ })).toBeVisible();
  expect(screen.queryByRole("button", { name: /Google (Drive|Slides)/ })).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: /Google Docs/ }));
  const detail = within(screen.getByRole("region", { name: "Google Docs" }));
  expect(screen.getByRole("heading", { name: "Google Docs" })).toHaveFocus();
  expect(detail.getByRole("status")).toHaveTextContent("연결됨");
  expect(detail.getByText("읽기 전용")).toBeVisible();
  expect(detail.getByText("지정한 Docs 문서의 제목 조회")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "연결 해제" }));
  await detail.findByText("연결된 계정이 없습니다.");
  await user.click(screen.getByRole("button", { name: "뒤로" }));
  expect(screen.queryByRole("button", { name: /Google Docs/ })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: /Google Sheets sheets@example.test/ })).toBeVisible();
  expect(calls.filter(({ method }) => method === "POST")).toEqual([{
    path: base + "/docs/disconnect", method: "POST", body: { connection_id: docs.connection_id, epoch: docs.epoch },
  }]);
});

it("uses the vault-backed Kiri flow for only the selected service and confirms the returned account", async () => {
  const calls = setup();
  const user = userEvent.setup();
  mount();
  await user.click(await screen.findByRole("button", { name: /Google Drive/ }));
  await user.click(await screen.findByRole("button", { name: "Google 연결" }));
  await within(screen.getByRole("region", { name: "Google Drive" })).findByText(session.user.email);
  expect(connectInKiri).toHaveBeenCalledWith(expect.any(Function), session.user.id, session.user.email,
    "drive", vaultOrigin, undefined, expect.any(AbortSignal), expect.any(Function), expect.objectContaining({persisted:expect.any(Function)}));
  expect(calls.filter(({ method }) => method === "POST")).toEqual([{
    path: base + "/drive/complete", method: "POST", body: { handle },
  }]);
  expect(sessionStorage.getItem(key)).toBeNull();
  expect(sessionStorage.getItem(gmailKey)).toBeNull();
});

it("distinguishes the retained-receipt capacity limit from a live Kiri conflict", async () => {
  setup();
  vi.mocked(connectInKiri).mockRejectedValueOnce(new ApiFailure(429, "too_many_requests"));
  const user = userEvent.setup();
  mount();
  await user.click(await screen.findByRole("button", { name: /Google Drive/ }));
  await user.click(await screen.findByRole("button", { name: "Google 연결" }));
  expect(await screen.findByText("최근 Google 연결 시도가 많습니다. 잠시 후 다시 시도해 주세요.")).toBeVisible();
  expect(screen.queryByText("이 계정에 진행 중인 Kiri 연결이 있습니다.")).not.toBeInTheDocument();
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
  expect(location.hash).toBe("#connections/docs");
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
    expect(location.hash).toBe("#connections/docs");
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
  expect(location.hash).toBe("#connections/docs");
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
  expect(screen.queryByRole("button", { name: "Google Workspace 설정" })).not.toBeInTheDocument();
  expect(await screen.findByRole("button", { name: "다시 확인" })).toBeEnabled();
  await user.click(screen.getByRole("button", { name: "뒤로" }));
  await user.click(await screen.findByRole("button", { name: /Gmail gmail@example.test/ }));
  expect(await screen.findByRole("button", { name: "연결 해제" })).toBeEnabled();
  expect(calls.every(({ method }) => method === "GET")).toBe(true);
  expect(calls.some(({ path }) => path.includes("/admin/"))).toBe(false);
});

it("links the installation administrator to Google web sign-in setup when unavailable", async () => {
  setup({ workspaceStatus: 404 });
  mount(true);
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: /Google Slides/ }));
  await user.click(await screen.findByRole("button", { name: "Google Workspace 설정" }));
  expect(await screen.findByText("Google 앱 설정이 등록되어 있습니다.")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "Google Slides 연결로 돌아가기" }));
  expect(await screen.findByRole("heading", { name: "Google Slides" })).toBeVisible();
});

it("returns an unconfigured admin to the selected Drive service without claiming configuration or starting consent", async () => {
  const calls = setup({ workspaceStatus: 404, configured: false });
  mount(true);
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: /Google Drive/ }));
  await user.click(await screen.findByRole("button", { name: "Google Workspace 설정" }));
  await user.click(await screen.findByRole("button", { name: "Google Drive 연결로 이동" }));
  expect(await screen.findByRole("heading", { name: "Google Drive" })).toBeVisible();
  expect(await screen.findByRole("button", { name: "Google 연결" })).toBeEnabled();
  await user.click(screen.getByRole("button", { name: "뒤로" }));
  expect(await screen.findByRole("button", { name: /Google Drive .*설치 설정 필요/ })).toBeVisible();
  expect(connectInKiri).not.toHaveBeenCalled();
  expect(calls.every(({ method }) => method === "GET")).toBe(true);
});

it("opens the service chooser from root settings without default Gmail consent or read", async () => {
  const calls = setup({ workspaceStatus: 404, configured: false });
  mount(true);
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "Google Workspace 설정" }));
  await user.click(await screen.findByRole("button", { name: "서비스 선택하고 계속" }));
  expect(await screen.findByRole("button", { name: /Gmail/ })).toBeVisible();
  expect(screen.getByRole("button", { name: /Google Drive/ })).toBeVisible();
  expect(screen.queryByRole("region", { name: "Gmail" })).not.toBeInTheDocument();
  expect(connectInKiri).not.toHaveBeenCalled();
  expect(sessionStorage.getItem(gmailKey)).toBeNull();
  expect(calls.every(({ method }) => method === "GET")).toBe(true);
});

it("keeps an unregistered installation distinct from each member connection and restores focus from details", async () => {
  const calls = setup({ workspaceStatus: 404, configured: false });
  mount(true);
  const user = userEvent.setup();
  const installation = within(screen.getByRole("region", { name: "Google Workspace 설치 설정" }));
  expect(await installation.findByText("설치 설정 필요")).toBeVisible();
  expect(await screen.findByRole("button", { name: /Gmail gmail@example.test 연결됨/ })).toBeVisible();
  const slides = await screen.findByRole("button", { name: /Google Slides .*설치 설정 필요/ });
  slides.focus();
  await user.keyboard("{Enter}");
  expect(screen.getByRole("heading", { name: "Google Slides" })).toHaveFocus();
  const detail = within(screen.getByRole("region", { name: "Google Slides" }));
  expect(detail.getByRole("button", { name: "Google Workspace 설정" })).toBeEnabled();
  expect(detail.getByRole("button", { name: "Google 연결" })).toBeEnabled();
  await user.click(screen.getByRole("button", { name: "뒤로" }));
  expect(screen.getByRole("button", { name: /Google Slides .*설치 설정 필요/ })).toHaveFocus();
  expect(calls.every(({ method }) => method === "GET")).toBe(true);
});
