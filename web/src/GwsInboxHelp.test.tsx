import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import { GwsInboxHelp } from "./GwsInboxHelp";
import { beginGws, clearPendingGws, GWS_CONNECTED, pendingGws } from "./gws-connection";
const target = {threadId: "11111111-1111-4111-8111-111111111111", runId: "22222222-2222-4222-8222-222222222222"};
const resume = `/api/v1/threads/${target.threadId}/runs/${target.runId}/resume-gmail`;
type Call = {path: string; method: string; body: unknown; account: string};
function fixture(handler: (path: string) => unknown | Response | Promise<unknown>, overview = {configured: true, mailbox: "synthetic@example.test", confirmed: false}) {
  const calls: Call[] = [];
  vi.stubGlobal("fetch", vi.fn(async (path: string, init: RequestInit) => {
    calls.push({path, method: init.method ?? "GET", body: init.body ? JSON.parse(init.body as string) : undefined, account: (init.headers as Record<string,string>)["X-Moosuri-Account"]});
    const result = path === "/api/v1/mail/gws-inbox" ? overview : await handler(path);
    return result instanceof Response ? result : Response.json(result);
  }));
  return calls;
}
const mutations = (calls: Call[]) => calls.filter(call => call.method === "POST");
afterEach(() => { cleanup(); clearPendingGws("owner"); clearPendingGws("other-owner"); localStorage.clear(); vi.useRealTimers(); vi.unstubAllGlobals(); });
it("keeps status and browser focus passive until the user explicitly connects", async () => {
  const calls = fixture(() => { throw new Error("unexpected mutation"); }, {configured:true, mailbox:"synthetic@example.test", confirmed:true});
  render(<GwsInboxHelp accountId="owner" resumeTarget={target} />);
  await screen.findByText(/연결한 기록이 있습니다/);
  fireEvent.focus(window); fireEvent.online(window);
  expect(mutations(calls)).toEqual([]);
  expect(pendingGws("owner")).toBeNull();
  expect(screen.queryByRole("textbox")).toBeNull();
  expect(screen.queryByRole("button", {name:"내 Google 로그인 볼트 열기 ↗"})).toBeNull();
});
it("confirms the displayed mailbox once then resumes only the exact server-owned original request", async () => {
  const calls = fixture(path => path.endsWith("/check") ? {state:"needs_confirmation", mailbox:"synthetic@example.test", confirmation:"synthetic-confirmation"} : {state:"ready", mailbox:"synthetic@example.test"});
  const connected = vi.fn(); window.addEventListener(GWS_CONNECTED, connected);
  render(<GwsInboxHelp accountId="owner" resumeTarget={target} />);
  await screen.findByText(/기기 연결이 준비됐습니다/);
  await userEvent.click(screen.getByRole("button", {name:"Gmail 연결하고 요청 이어가기"}));
  await screen.findByText(/표시된 계정을 확인/);
  expect(mutations(calls).map(c => c.path)).toEqual(["/api/v1/mail/gws-inbox/check"]);
  const stored = localStorage.getItem("moosuri.gws-connect.v1.owner")!;
  expect(stored).toContain(target.runId); expect(stored).not.toContain("synthetic@example.test"); expect(stored).not.toContain("confirmation");
  await userEvent.click(screen.getByRole("button", {name:"이 Gmail 계정으로 연결"}));
  await screen.findByText(/원래 받은메일 요청을 이어갑니다/);
  expect(mutations(calls).map(c => [c.path,c.body])).toEqual([
    ["/api/v1/mail/gws-inbox/check", {}],
    ["/api/v1/mail/gws-inbox/confirm", {confirmation:"synthetic-confirmation"}],
    [resume, {}],
  ]);
  expect(calls.every(c => c.account === "owner")).toBe(true);
  expect(connected).toHaveBeenCalledTimes(1);
  expect((connected.mock.calls[0][0] as CustomEvent).detail).toEqual({accountId:"owner", ...target});
  expect(pendingGws("owner")).toBeNull();
  fireEvent.focus(window); expect(mutations(calls)).toHaveLength(3);
  window.removeEventListener(GWS_CONNECTED, connected);
});
it("opens Google login on the connected 연결한 기기 only after the login click and verifies on return", async () => {
  let ready = false;
  const calls = fixture(path => path.endsWith("/login") ? {state:"login_started"} : {state:ready ? "ready" : "login_required", mailbox:"synthetic@example.test"});
  render(<GwsInboxHelp accountId="owner" resumeTarget={target} />);
  await screen.findByText(/기기 연결이 준비됐습니다/);
  await userEvent.click(screen.getByRole("button", {name:"Gmail 연결하고 요청 이어가기"}));
  await screen.findByText(/Google 로그인이 필요/);
  expect(calls.some(c => c.path.endsWith("/login"))).toBe(false);
  await userEvent.click(screen.getByRole("button", {name:"연결한 기기에서 Google 로그인 열기"}));
  await screen.findByText(/연결한 기기에서 Google 로그인 화면을 열었습니다/);
  expect(calls.some(c => c.path === resume)).toBe(false);
  ready = true; fireEvent.focus(window);
  await screen.findByText(/원래 받은메일 요청을 이어갑니다/);
  expect(calls.filter(c => c.path.endsWith("/login"))).toHaveLength(1);
  expect(calls.filter(c => c.path === resume)).toHaveLength(1);
});
it("restores an explicit unexpired workflow in settings after remount without storing request text", async () => {
  let ready = false;
  const calls = fixture(() => ({state:ready ? "ready" : "offline"}));
  const first = render(<GwsInboxHelp accountId="owner" resumeTarget={target} />);
  await screen.findByText(/기기 연결이 준비됐습니다/);
  await userEvent.click(screen.getByRole("button", {name:"Gmail 연결하고 요청 이어가기"}));
  await screen.findByText(/연결한 기기에 닿지 않습니다/);
  first.unmount(); ready = true;
  render(<GwsInboxHelp accountId="owner" />);
  await screen.findByText(/원래 받은메일 요청을 이어갑니다/);
  expect(calls.filter(c => c.path === resume)).toHaveLength(1);
});
it("does not continue another account or another original card's pending workflow", async () => {
  beginGws("owner", target);
  const calls = fixture(() => ({state:"ready"}));
  const view = render(<GwsInboxHelp accountId="other-owner" />);
  await screen.findByText(/기기 연결이 준비됐습니다/);
  view.unmount();
  render(<GwsInboxHelp accountId="owner" resumeTarget={{...target,runId:"33333333-3333-4333-8333-333333333333"}} />);
  await screen.findByText(/기기 연결이 준비됐습니다/);
  fireEvent.focus(window);
  expect(mutations(calls)).toEqual([]);
});
it("expires a persisted workflow and allows cancellation without focus replay", async () => {
  vi.setSystemTime(new Date("2026-11-18T01:00:00Z")); beginGws("owner", target);
  vi.setSystemTime(new Date("2026-11-18T01:31:00Z"));
  const calls = fixture(() => ({state:"offline"}));
  render(<GwsInboxHelp accountId="owner" resumeTarget={target} />);
  await screen.findByText(/기기 연결이 준비됐습니다/);
  fireEvent.focus(window); expect(mutations(calls)).toEqual([]);
  await userEvent.click(screen.getByRole("button", {name:"Gmail 연결하고 요청 이어가기"}));
  await screen.findByText(/연결한 기기에 닿지 않습니다/);
  await userEvent.click(screen.getByRole("button", {name:"연결 취소"}));
  expect(pendingGws("owner")).toBeNull();
  fireEvent.focus(window); expect(mutations(calls)).toHaveLength(1);
});
it.each(["unconfigured", "wrong_account", "offline"])("reports %s without attempting any resume", async state => {
  const calls = fixture(() => ({state}));
  render(<GwsInboxHelp accountId="owner" resumeTarget={target} />);
  await screen.findByText(/기기 연결이 준비됐습니다/);
  await userEvent.click(screen.getByRole("button", {name:"Gmail 연결하고 요청 이어가기"}));
  const descriptions = {unconfigured:/설치자에게 이 계정의 기기 연결/,wrong_account:/Google 계정이 연결할 Gmail과 다릅니다/,offline:/연결한 기기에 닿지 않습니다/};
  await screen.findByText(descriptions[state as keyof typeof descriptions]);
  expect(mutations(calls).map(c => c.path)).toEqual(["/api/v1/mail/gws-inbox/check"]);
});
it("clears a stale server-rejected original and never replays other effects", async () => {
  const calls = fixture(path => path === resume ? Response.json({code:"conflict"},{status:409}) : {state:"ready"});
  render(<GwsInboxHelp accountId="owner" resumeTarget={target} />);
  await screen.findByText(/기기 연결이 준비됐습니다/);
  await userEvent.click(screen.getByRole("button", {name:"Gmail 연결하고 요청 이어가기"}));
  await screen.findByText(/원래 요청은 이어갈 수 없습니다/);
  expect(pendingGws("owner")).toBeNull();
  fireEvent.focus(window);
  expect(mutations(calls).map(c => c.path)).toEqual(["/api/v1/mail/gws-inbox/check", resume]);
});
it("retains an unknown resume receipt and recovers through the same original endpoint", async () => {
  let failed = false;
  const calls = fixture(path => { if(path === resume && !failed) {failed=true; throw new TypeError("synthetic dropped receipt");} return {state:"ready"}; });
  render(<GwsInboxHelp accountId="owner" resumeTarget={target} />);
  await screen.findByText(/기기 연결이 준비됐습니다/);
  await userEvent.click(screen.getByRole("button", {name:"Gmail 연결하고 요청 이어가기"}));
  await screen.findByRole("alert"); expect(pendingGws("owner")?.target).toEqual(target);
  fireEvent.focus(window);
  await screen.findByText(/원래 받은메일 요청을 이어갑니다/);
  expect(calls.filter(c => c.path === resume)).toHaveLength(2);
});
it("fences late verification after unmount and prevents double-click duplicates", async () => {
  let resolve: ((value: unknown) => void) | undefined;
  const calls = fixture(() => new Promise(r => {resolve=r;}));
  const view = render(<GwsInboxHelp accountId="owner" resumeTarget={target} />);
  await screen.findByText(/기기 연결이 준비됐습니다/);
  const button = screen.getByRole("button", {name:"Gmail 연결하고 요청 이어가기"});
  fireEvent.click(button); fireEvent.click(button); fireEvent.focus(window);
  await waitFor(() => expect(mutations(calls)).toHaveLength(1));
  view.unmount();
  await act(async () => { resolve?.({state:"ready"}); });
  expect(calls.some(c => c.path === resume)).toBe(false);
});

it("honors another tab clearing a completed workflow rather than reviving an in-memory marker", async () => {
  beginGws("owner", target);
  localStorage.removeItem("moosuri.gws-connect.v1.owner");
  const calls = fixture(() => ({state:"ready"}));
  render(<GwsInboxHelp accountId="owner" resumeTarget={target} />);
  await screen.findByText(/기기 연결이 준비됐습니다/);
  fireEvent.focus(window);
  expect(mutations(calls)).toEqual([]);
  expect(pendingGws("owner")).toBeNull();
});
