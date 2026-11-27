import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import { PersonalClaudeSetup } from "./PersonalClaudeSetup";
import { PersonalEngineSetup } from "./PersonalEngineSetup";

afterEach(() => vi.unstubAllGlobals());
const current = "11111111-1111-4111-8111-111111111111";
type Call = { url: string; method: string; body?: Record<string, unknown> };
const pixels = { frame_token: "frame-1", png_base64: "iVBORw0KGgo=", width: 400, height: 800, url: "https://claude.ai" };

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
/** Answers status/frame for the initiating session while the stage is `awaiting_code`. */
function owner(id: () => string, stage: () => string) {
  return (call: Call) => {
    if (call.body?.op === "frame") {
      return stage() === "awaiting_code"
        ? Response.json({ ...op(id(), stage(), { browser_available: true }), frame: pixels })
        : Response.json(op(id(), stage()));
    }
    return Response.json(op(id(), stage(), stage() === "awaiting_code" ? { browser_available: true } : {}));
  };
}

it("needs explicit hosting acceptance, shows the login in the same page and finishes without any code entry", async () => {
  let id = ""; let stage = "preparing";
  const answer = owner(() => id, () => stage);
  const calls = serve(call => {
    if (call.method === "GET") return Response.json(idle);
    if (call.body?.op === "start") { id = String(call.body.request_id); stage = "awaiting_code"; return Response.json(op(id, "preparing")); }
    return answer(call);
  });
  const props = view();
  await begin();
  expect(await screen.findByAltText("Claude 로그인 화면")).toBeInTheDocument();
  expect(screen.queryByLabelText("로그인 코드")).not.toBeInTheDocument();
  expect(screen.queryByRole("link", { name: /Claude 로그인/ })).not.toBeInTheDocument();
  stage = "ready"; // the server captured the callback by itself
  await waitFor(() => expect(props.onSaved).toHaveBeenCalledOnce());
  const start = calls.find(c => c.body?.op === "start")!.body!;
  expect(start).toMatchObject({ email: "owner@example.test", model: "claude-sonnet-5-5", effort: "medium", hosting_terms_accepted: true });
  expect(start).not.toHaveProperty("replace_revision");
  expect(calls.some(c => c.body?.op === "code")).toBe(false);
  expect(screen.queryByAltText("Claude 로그인 화면")).not.toBeInTheDocument();
  expect(screen.getByRole("status")).toHaveTextContent("연결되었습니다");
});

it("shows another session of the same owner only the status: no screen, input or cancel", async () => {
  const resumed = "22222222-2222-4222-8222-222222222222";
  const calls = serve(() => Response.json(op(resumed, "awaiting_code")));
  view();
  expect(await screen.findByText(/로그인을 시작한 세션에서/)).toBeInTheDocument();
  await new Promise(resolve => setTimeout(resolve, 60));
  expect(screen.queryByAltText("Claude 로그인 화면")).not.toBeInTheDocument();
  expect(screen.queryByLabelText("선택한 칸에 입력")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "취소" })).not.toBeInTheDocument();
  expect(calls.some(c => c.body?.op === "frame" || c.body?.op === "input")).toBe(false);
});

it("inspects a lost start reply and never starts a second login", async () => {
  let id = "";
  const answer = owner(() => id, () => "awaiting_code");
  const calls = serve(call => {
    if (call.method === "GET") return Response.json(idle);
    if (call.body?.op === "start") { id = String(call.body.request_id); throw new TypeError("synthetic response lost"); }
    return answer(call);
  });
  view();
  await begin();
  expect(await screen.findByAltText("Claude 로그인 화면")).toBeInTheDocument();
  expect(calls.filter(c => c.body?.op === "start")).toHaveLength(1);
  expect(calls.find(c => c.body?.op === "status")!.body!.request_id).toBe(id);
});

it("drops the screen and stops every request when the administrator loses authority", async () => {
  let id = ""; let revoked = false;
  const answer = owner(() => id, () => "awaiting_code");
  const calls = serve(call => {
    if (call.method === "GET") return Response.json(idle);
    if (call.body?.op === "start") { id = String(call.body.request_id); return Response.json(op(id, "awaiting_code", { browser_available: true })); }
    return revoked ? new Response(null, { status: 403 }) : answer(call);
  });
  const props = view();
  await begin();
  await screen.findByAltText("Claude 로그인 화면");
  revoked = true;
  await waitFor(() => expect(props.unavailable).toHaveBeenCalledOnce());
  expect(screen.queryByAltText("Claude 로그인 화면")).not.toBeInTheDocument();
  expect(screen.queryByLabelText("선택한 칸에 입력")).not.toBeInTheDocument();
  await new Promise(resolve => setTimeout(resolve, 40));
  const seen = calls.length;
  await new Promise(resolve => setTimeout(resolve, 80));
  expect(calls.length).toBe(seen);
});

it("resumes the initiating session's screen after reload without starting again", async () => {
  const resumed = "22222222-2222-4222-8222-222222222222";
  const calls = serve(call => call.body?.op === "frame"
    ? Response.json({ ...op(resumed, "awaiting_code", { browser_available: true }), frame: pixels })
    : Response.json(op(resumed, "awaiting_code", { browser_available: true })));
  view();
  expect(await screen.findByAltText("Claude 로그인 화면")).toBeInTheDocument();
  expect(calls.some(c => c.body?.op === "start")).toBe(false);
});

it("cancels a live login with the same cancel request", async () => {
  const id = "88888888-8888-4888-8888-888888888888";
  let cancelled = false;
  const calls = serve(call => {
    if (call.body?.op === "cancel") { cancelled = true; return Response.json(op(id, "cancelled")); }
    if (call.body?.op === "frame") return cancelled ? Response.json(op(id, "cancelled")) : Response.json({ ...op(id, "awaiting_code", { browser_available: true }), frame: pixels });
    return Response.json(op(id, cancelled ? "cancelled" : "awaiting_code", cancelled ? {} : { browser_available: true }));
  });
  view();
  await screen.findByAltText("Claude 로그인 화면");
  await userEvent.setup().click(screen.getByRole("button", { name: "취소" }));
  expect(await screen.findByRole("button", { name: "다시 시도" })).toBeInTheDocument();
  expect(calls.find(c => c.body?.op === "cancel")!.body).toEqual({ op: "cancel", request_id: id });
  expect(screen.queryByAltText("Claude 로그인 화면")).not.toBeInTheDocument();
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

it("keeps cleanup_pending retryable with the same cancel request and shows no screen", async () => {
  const pendingId = "66666666-6666-4666-8666-666666666666";
  const calls = serve(call => call.method === "GET" || call.body?.op === "status"
    ? Response.json(op(pendingId, "cleanup_pending"))
    : Response.json(op(pendingId, "cleanup_pending")));
  view();
  const retry = await screen.findByRole("button", { name: "정리 다시 시도" });
  await userEvent.setup().click(retry);
  await waitFor(() => expect(calls.filter(c => c.body?.op === "cancel")).toHaveLength(1));
  expect(screen.queryByAltText("Claude 로그인 화면")).not.toBeInTheDocument();
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

const gone = (id: string, state: string) => op(id, state, { kind: "disconnect" });

it("disconnects with one explicit action, offers no cancel meanwhile and reports only its own completion", async () => {
  let id = ""; let release = false;
  const calls = serve(call => {
    if (call.method === "GET") return Response.json(idle);
    if (call.body?.op === "disconnect") { id = String(call.body.request_id); return Response.json(gone(id, "disconnecting")); }
    return Response.json(gone(id, release ? "disconnected" : "disconnecting"));
  });
  const onDisconnected = vi.fn();
  const props = view({ disconnectRevision: current, onDisconnected });
  const user = userEvent.setup();
  expect(await screen.findByText(/이 서버에 저장된 내 Claude 로그인이 삭제/)).toBeInTheDocument();
  expect(screen.queryByLabelText("Claude 계정 이메일")).not.toBeInTheDocument();
  await user.click(await screen.findByRole("button", { name: "구독 연결 해제" }));
  expect(await screen.findByText(/로그인을 삭제하고 있습니다/)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "취소" })).not.toBeInTheDocument();
  expect(onDisconnected).not.toHaveBeenCalled();
  release = true;
  await waitFor(() => expect(onDisconnected).toHaveBeenCalledOnce());
  const sent = calls.filter(c => c.body?.op === "disconnect");
  expect(sent).toHaveLength(1);
  expect(sent[0].body).toEqual({ op: "disconnect", request_id: id, replace_revision: current });
  expect(id).not.toBe(current);
  expect(props.onSaved).not.toHaveBeenCalled();
  expect(screen.getByRole("status")).toHaveTextContent("해제되었고");
});

it("inspects a lost disconnect reply by the same operation and never sends a second one", async () => {
  let id = "";
  const calls = serve(call => {
    if (call.method === "GET") return Response.json(idle);
    if (call.body?.op === "disconnect") { id = String(call.body.request_id); throw new TypeError("synthetic response lost"); }
    return Response.json(gone(id, "disconnecting"));
  });
  view({ disconnectRevision: current });
  await userEvent.setup().click(await screen.findByRole("button", { name: "구독 연결 해제" }));
  expect(await screen.findByText(/로그인을 삭제하고 있습니다/)).toBeInTheDocument();
  expect(calls.filter(c => c.body?.op === "disconnect")).toHaveLength(1);
  expect(calls.find(c => c.body?.op === "status")!.body!.request_id).toBe(id);
});

it("resumes another session's stuck disconnect through the cleanup request instead of starting a new one", async () => {
  const other = "77777777-7777-4777-8777-777777777777";
  let refused = false; let cleaned = false;
  const calls = serve(call => {
    if (call.method === "GET") return Response.json(refused ? gone(other, "cleanup_pending") : idle);
    if (call.body?.op === "disconnect") { refused = true; return new Response(null, { status: 409 }); }
    if (call.body?.op === "cancel") { cleaned = true; return Response.json(gone(other, "disconnecting")); }
    return Response.json(gone(other, cleaned ? "disconnected" : "cleanup_pending"));
  });
  const onDisconnected = vi.fn();
  view({ disconnectRevision: current, onDisconnected });
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "구독 연결 해제" }));
  await user.click(await screen.findByRole("button", { name: "정리 다시 시도" }));
  await waitFor(() => expect(onDisconnected).toHaveBeenCalledOnce());
  expect(calls.find(c => c.body?.op === "cancel")!.body).toEqual({ op: "cancel", request_id: other });
  expect(calls.filter(c => c.body?.op === "disconnect")).toHaveLength(1);
  expect(calls.some(c => c.body?.op === "start")).toBe(false);
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


it("replaces an external subscription without disconnecting and keeps cancellation despite stale polls", async () => {
  let id = "";
  const calls = serve(call => {
    if (call.method === "GET") return Response.json({ ...idle, replacement_revision: current });
    if (call.body?.op === "start") { id = String(call.body.request_id); return Response.json(op(id, "preparing")); }
    if (call.body?.op === "cancel") return Response.json(op(id, "cancelled"));
    return Response.json(op(id, "preparing"));
  });
  const onDisconnected = vi.fn();
  const props = view({ disconnectRevision: current, onDisconnected });
  const user = await begin();
  expect(calls.find(c => c.body?.op === "start")?.body?.replace_revision).toBe(current);
  await user.click(await screen.findByRole("button", { name: "취소" }));
  await waitFor(() => expect(calls.filter(c => c.body?.op === "cancel")).toHaveLength(1));
  expect(await screen.findByText(/설정이 취소되었습니다. 기존 연결은 그대로입니다/)).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "다시 시도" }));
  expect(await screen.findByLabelText("Claude 계정 이메일")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "구독 연결 해제" })).not.toBeInTheDocument();
  expect(calls.some(c => c.body?.op === "disconnect")).toBe(false);
  expect(onDisconnected).not.toHaveBeenCalled();
  expect(props.onSaved).not.toHaveBeenCalled();
});

it("propagates a verified legacy replacement through the existing subscription parent", async () => {
  const calls = serve(call => {
    if (call.method === "GET") return Response.json({ ...idle, replacement_revision: current });
    const id = String(call.body?.request_id);
    return Response.json(op(id, "ready", { connection_revision: id }));
  }, () => Response.json({ supported: true, saved: true, connection_revision: current }));
  const onSaved = vi.fn();
  render(<PersonalEngineSetup account="synthetic-admin" subscriptionRevision={current} onSaved={onSaved} expired={vi.fn()} unavailable={vi.fn()} />);
  await begin();
  await waitFor(() => expect(onSaved).toHaveBeenCalledOnce());
  expect(calls.filter(c => c.body?.op === "start")).toHaveLength(1);
  expect(calls.some(c => c.body?.op === "disconnect")).toBe(false);
});

it("keeps connection actions unavailable after a failed check and permits explicit recovery", async () => {
  let available = false;
  const calls = serve(() => available ? Response.json({ ...idle, replacement_revision: current }) : new Response(null, { status: 503 }));
  view({ disconnectRevision: current });
  const retry = await screen.findByRole("button", { name: "연결 상태 다시 확인" });
  expect(screen.queryByRole("button", { name: "구독 연결 해제" })).not.toBeInTheDocument();
  expect(screen.queryByLabelText("Claude 계정 이메일")).not.toBeInTheDocument();
  available = true;
  await userEvent.setup().click(retry);
  expect(await screen.findByLabelText("Claude 계정 이메일")).toBeInTheDocument();
  expect(calls.every(c => c.method === "GET")).toBe(true);
});

it("refuses a replacement hint for a different displayed connection", async () => {
  const calls = serve(() => Response.json({ ...idle, replacement_revision: "99999999-9999-4999-8999-999999999999" }));
  view({ disconnectRevision: current });
  expect(await screen.findByRole("alert")).toHaveTextContent("먼저 변경되었습니다");
  expect(screen.queryByLabelText("Claude 계정 이메일")).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "구독 연결 해제" })).not.toBeInTheDocument();
  expect(calls.every(c => c.method === "GET")).toBe(true);
});

it("rechecks a changed displayed revision and refreshes the parent on a mismatch", async () => {
  const next = "99999999-9999-4999-8999-999999999999";
  const calls = serve(() => Response.json({ ...idle, connection_revision: current }));
  const onRefresh = vi.fn();
  const props = { account: "synthetic-admin", onSaved: vi.fn(), expired: vi.fn(), unavailable: vi.fn(), onRefresh };
  const mounted = render(<PersonalClaudeSetup {...props} disconnectRevision={current}/>);
  expect(await screen.findByRole("button", { name: "구독 연결 해제" })).toBeInTheDocument();
  mounted.rerender(<PersonalClaudeSetup {...props} disconnectRevision={next}/>);
  expect(screen.queryByRole("button", { name: "구독 연결 해제" })).not.toBeInTheDocument();
  await userEvent.setup().click(await screen.findByRole("button", { name: "연결 상태 다시 확인" }));
  expect(onRefresh).toHaveBeenCalledOnce();
  expect(calls.every(c => c.method === "GET")).toBe(true);
});

it("explains a missing managed record without claiming another setup is running", async () => {
  const calls = serve(call => call.method === "GET" ? Response.json(idle) : new Response(null, { status: 412 }));
  view({ disconnectRevision: current });
  await userEvent.setup().click(await screen.findByRole("button", { name: "구독 연결 해제" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("관리 기록을 확인하지 못해 해제하지 않았습니다");
  expect(calls.filter(c => c.body?.op === "disconnect")).toHaveLength(1);
  expect(screen.queryByText(/다른 설정이 진행 중/)).not.toBeInTheDocument();
});

it("shows an unstarted disconnect failure as preserved credentials and permits a fresh request", async () => {
  const calls = serve(call => call.method === "GET" ? Response.json(idle)
    : Response.json(gone(String(call.body?.request_id), "failed")));
  view({ disconnectRevision: current });
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "구독 연결 해제" }));
  expect(await screen.findByRole("status")).toHaveTextContent("연결과 로그인은 삭제하지 않았습니다");
  await user.click(screen.getByRole("button", { name: "다시 시도" }));
  await user.click(await screen.findByRole("button", { name: "구독 연결 해제" }));
  const ids = calls.filter(c => c.body?.op === "disconnect").map(c => c.body?.request_id);
  expect(ids).toHaveLength(2); expect(ids[0]).not.toBe(ids[1]);
});
