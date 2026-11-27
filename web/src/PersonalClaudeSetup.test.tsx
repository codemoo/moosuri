import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import { PersonalClaudeSetup } from "./PersonalClaudeSetup";
import { PersonalEngineSetup } from "./PersonalEngineSetup";

afterEach(() => vi.unstubAllGlobals());
const official = "https://claude.com/cai/oauth/authorize?code=true&state=synthetic";
const current = "11111111-1111-4111-8111-111111111111";
type Call = { url: string; method: string; body?: Record<string, unknown> };

/** Stubs fetch; `subscription` answers the subscription endpoint. */
function serve(subscription: (call: Call, calls: Call[]) => Response | Promise<Response>, other?: () => Response) {
  const calls: Call[] = [];
  vi.stubGlobal("fetch", vi.fn(async (url: string, init: RequestInit) => {
    const call: Call = { url: String(url), method: String(init.method ?? "GET"), body: init.body ? JSON.parse(String(init.body)) : undefined };
    if (!call.url.endsWith("/personal/admin/engine/subscription")) return other ? other() : Response.json({ supported: true, saved: false });
    calls.push(call);
    return subscription(call, calls);
  }));
  return calls;
}
const op = (id: string, state: string, extra: Record<string, unknown> = {}) => ({ supported: true, operation: { request_id: id, state, model: "claude-sonnet-5-5", effort: "medium", ...extra } });
const idle = { supported: true, operation: null };
function view(extra: Partial<Parameters<typeof PersonalClaudeSetup>[0]> = {}) {
  const props = { account: "synthetic-admin", onSaved: vi.fn(), expired: vi.fn(), unavailable: vi.fn(), pollMs: 10, ...extra };
  render(<PersonalClaudeSetup {...props} />);
  return props;
}
async function begin() {
  const user = userEvent.setup();
  await user.type(await screen.findByLabelText("Claude 계정 이메일"), "owner@example.test");
  const start = screen.getByRole("button", { name: "구독 로그인 시작" });
  expect(start).toBeDisabled();
  await user.click(screen.getByLabelText(/호스팅 조건을 확인했고/));
  await user.click(start);
  return user;
}

it("needs explicit hosting acceptance, links only the official page and sends the code once", async () => {
  let id = ""; let stage = "preparing";
  const calls = serve(call => {
    const body = call.body;
    if (call.method === "GET") return Response.json(idle);
    if (body?.op === "start") { id = String(body.request_id); return Response.json(op(id, "preparing")); }
    if (body?.op === "code") { stage = "ready"; return Response.json(op(id, "verifying")); }
    if (stage === "preparing") stage = "awaiting_code";
    return Response.json(stage === "awaiting_code" ? op(id, stage, { authorization_url: official }) : op(id, stage));
  });
  const props = view();
  const user = await begin();
  const link = await screen.findByRole("link", { name: "Claude 로그인 열기" });
  expect(link).toHaveAttribute("href", official);
  expect(link).toHaveAttribute("rel", expect.stringContaining("noreferrer"));
  expect(link).toHaveAttribute("referrerpolicy", "no-referrer");
  const input = screen.getByLabelText("로그인 코드");
  await user.type(input, "synthetic-code-1234");
  await user.click(screen.getByRole("button", { name: "코드 제출" }));
  await waitFor(() => expect(props.onSaved).toHaveBeenCalledOnce());
  const start = calls.find(c => c.body?.op === "start")!.body!;
  expect(start).toMatchObject({ email: "owner@example.test", model: "claude-sonnet-5-5", effort: "medium", hosting_terms_accepted: true });
  expect(start).not.toHaveProperty("replace_revision");
  expect(calls.filter(c => c.body?.op === "code")).toHaveLength(1);
  expect(screen.queryByRole("link", { name: "Claude 로그인 열기" })).not.toBeInTheDocument();
  expect(screen.getByRole("status")).toHaveTextContent("연결되었습니다");
  expect(JSON.stringify(localStorage) + JSON.stringify(sessionStorage)).not.toContain("synthetic-code-1234");
  expect(document.body.innerHTML).not.toContain("synthetic-code-1234");
});

it("never turns a foreign sign-in address into a link", async () => {
  let id = "";
  serve(call => {
    if (call.method === "GET") return Response.json(idle);
    if (call.body?.op === "start") { id = String(call.body.request_id); return Response.json(op(id, "preparing")); }
    return Response.json(op(id, "awaiting_code", { authorization_url: "https://claude.com.evil.test/cai/oauth/authorize?x=1" }));
  });
  view();
  await begin();
  expect(await screen.findByText(/로그인을 시작한 세션에서/)).toBeInTheDocument();
  expect(screen.queryByLabelText("로그인 코드")).not.toBeInTheDocument();
  expect(screen.queryByRole("link", { name: "Claude 로그인 열기" })).not.toBeInTheDocument();
});

it("inspects a lost start reply and never starts a second login", async () => {
  let id = "";
  const calls = serve(call => {
    if (call.method === "GET") return Response.json(idle);
    if (call.body?.op === "start") { id = String(call.body.request_id); throw new TypeError("synthetic response lost"); }
    return Response.json(op(id, "awaiting_code", { authorization_url: official }));
  });
  view();
  await begin();
  expect(await screen.findByRole("link", { name: "Claude 로그인 열기" })).toBeInTheDocument();
  expect(calls.filter(c => c.body?.op === "start")).toHaveLength(1);
  expect(calls.find(c => c.body?.op === "status")!.body!.request_id).toBe(id);
});

it("does not resubmit a code whose reply was lost and asks again only if it was not taken", async () => {
  let id = "";
  const calls = serve(call => {
    if (call.method === "GET") return Response.json(idle);
    if (call.body?.op === "start") { id = String(call.body.request_id); return Response.json(op(id, "awaiting_code", { authorization_url: official })); }
    if (call.body?.op === "code") throw new TypeError("synthetic response lost");
    return Response.json(op(id, "awaiting_code", { authorization_url: official }));
  });
  view();
  const user = await begin();
  const input = await screen.findByLabelText("로그인 코드");
  await user.type(input, "synthetic-code-1234");
  await user.click(screen.getByRole("button", { name: "코드 제출" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("코드 전달을 확인하지 못했습니다");
  expect(calls.filter(c => c.body?.op === "code")).toHaveLength(1);
  expect(screen.getByLabelText("로그인 코드")).toHaveValue("");
});

it("drops the link and stops polling when the administrator loses authority", async () => {
  let id = "";
  const calls = serve(call => {
    if (call.method === "GET") return Response.json(idle);
    if (call.body?.op === "start") { id = String(call.body.request_id); return Response.json(op(id, "awaiting_code", { authorization_url: official })); }
    return new Response(null, { status: 403 });
  });
  const props = view();
  await begin();
  await waitFor(() => expect(props.unavailable).toHaveBeenCalledOnce());
  expect(screen.queryByRole("link", { name: "Claude 로그인 열기" })).not.toBeInTheDocument();
  expect(screen.queryByLabelText("로그인 코드")).not.toBeInTheDocument();
  const seen = calls.length;
  await new Promise(resolve => setTimeout(resolve, 80));
  expect(calls.length).toBe(seen);
});

it("resumes an unfinished operation after reload and offers an explicit restart when interrupted", async () => {
  const resumed = "22222222-2222-4222-8222-222222222222";
  const calls = serve(call => call.method === "GET"
    ? Response.json(op(resumed, "awaiting_code", { authorization_url: official }))
    : Response.json(op(resumed, "awaiting_code", { authorization_url: official })));
  view();
  expect(await screen.findByRole("link", { name: "Claude 로그인 열기" })).toBeInTheDocument();
  expect(calls.some(c => c.body?.op === "start")).toBe(false);
});
it("recovers an interrupted operation only through the authenticated cleanup request, then allows a fresh start", async () => {
  const orphan = "33333333-3333-4333-8333-333333333333";
  let cleaned = false;
  const onActive = vi.fn();
  const calls = serve(call => {
    if (call.method === "GET") return Response.json(cleaned ? idle : op(orphan, "interrupted"));
    if (call.body?.op === "cancel") { cleaned = true; return Response.json(op(orphan, "cleanup_pending")); }
    if (call.body?.op === "status") return Response.json(op(orphan, cleaned ? "cancelled" : "interrupted"));
    return Response.json(op(String(call.body?.request_id), "preparing"));
  });
  const props = view({ onActive });
  expect(await screen.findByRole("status")).toHaveTextContent("중단되었습니다");
  expect(onActive).toHaveBeenLastCalledWith(true);
  // No restart control exists until the cleanup finished.
  expect(screen.queryByRole("button", { name: "다시 시도" })).not.toBeInTheDocument();
  expect(screen.queryByLabelText("Claude 계정 이메일")).not.toBeInTheDocument();
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "중단된 설정 정리" }));
  expect(calls.find(c => c.body?.op === "cancel")!.body).toEqual({ op: "cancel", request_id: orphan });
  await user.click(await screen.findByRole("button", { name: "다시 시도" }));
  expect(await screen.findByLabelText("Claude 계정 이메일")).toBeInTheDocument();
  expect(calls.some(c => c.body?.op === "start")).toBe(false);
  expect(props.onSaved).not.toHaveBeenCalled();
});

it("shows an unfinished operation of this owner when a new start is refused", async () => {
  const orphan = "55555555-5555-4555-8555-555555555555";
  let seen = false;
  serve(call => {
    if (call.method === "GET") return Response.json(seen ? op(orphan, "interrupted") : idle);
    if (call.body?.op === "start") { seen = true; return new Response(null, { status: 409 }); }
    return Response.json(op(orphan, "interrupted"));
  });
  view();
  await begin();
  expect(await screen.findByRole("button", { name: "중단된 설정 정리" })).toBeInTheDocument();
  expect(screen.getByRole("status")).toHaveTextContent("중단되었습니다");
});

it("keeps cleanup_pending retryable with the same cancel request and leaves no code behind", async () => {
  const pendingId = "66666666-6666-4666-8666-666666666666";
  const calls = serve(call => call.method === "GET" || call.body?.op === "status"
    ? Response.json(op(pendingId, "cleanup_pending"))
    : Response.json(op(pendingId, "cleanup_pending")));
  view();
  const retry = await screen.findByRole("button", { name: "정리 다시 시도" });
  await userEvent.setup().click(retry);
  await waitFor(() => expect(calls.filter(c => c.body?.op === "cancel")).toHaveLength(1));
  expect(screen.queryByLabelText("로그인 코드")).not.toBeInTheDocument();
  expect(screen.queryByRole("link", { name: "Claude 로그인 열기" })).not.toBeInTheDocument();
});

it("never accepts another request's ready state", async () => {
  let id = "";
  serve(call => {
    if (call.method === "GET") return Response.json(idle);
    if (call.body?.op === "start") { id = String(call.body.request_id); return Response.json(op(id, "preparing")); }
    return Response.json(op("44444444-4444-4444-8444-444444444444", "ready"));
  });
  const props = view();
  await begin();
  await new Promise(resolve => setTimeout(resolve, 100));
  expect(props.onSaved).not.toHaveBeenCalled();
  expect(screen.getByRole("status")).toHaveTextContent("준비하고 있습니다");
});

it("keeps API as the default choice and replaces a saved connection only by its revision", async () => {
  let id = "";
  const calls = serve(call => {
    if (call.method === "GET") return Response.json(idle);
    id = String(call.body?.request_id);
    return Response.json(op(id, "preparing"));
  }, () => Response.json({ supported: true, saved: true, connection_revision: current }));
  const onSaved = vi.fn();
  render(<PersonalEngineSetup account="synthetic-admin" onSaved={onSaved} expired={vi.fn()} unavailable={vi.fn()} />);
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "키·모델 교체" }));
  expect(screen.getByLabelText("Anthropic API 키")).toBeInTheDocument();
  expect(screen.getByLabelText("API 키 사용")).toBeChecked();
  await user.click(screen.getByLabelText("내 Claude 구독 사용"));
  expect(screen.queryByLabelText("Anthropic API 키")).not.toBeInTheDocument();
  await user.type(await screen.findByLabelText("Claude 계정 이메일"), "owner@example.test");
  await user.click(screen.getByLabelText(/호스팅 조건을 확인했고/));
  await user.click(screen.getByRole("button", { name: "구독 로그인 시작" }));
  await waitFor(() => expect(calls.some(c => c.body?.op === "start")).toBe(true));
  expect(calls.find(c => c.body?.op === "start")!.body).toMatchObject({ replace_revision: current, hosting_terms_accepted: true });
  expect(onSaved).not.toHaveBeenCalled();
});
