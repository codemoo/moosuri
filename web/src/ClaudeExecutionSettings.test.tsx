import { render, screen, waitFor, act, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, it, expect, vi } from "vitest";
import { ClaudeExecutionSettings } from "./ClaudeExecutionSettings";

const response = (body: unknown, status = 200) => new Response(JSON.stringify(body), {
  status, headers: { "Content-Type": "application/json" },
});
const unavailable = { runtime_configured: false, bindings: [], text_only: true };
const binding = { id: "owned", revision: 1, label: "내 Claude", status: "pending", cleanup_state: "not_requested" };
// POST returns an execution reference, not the list endpoint's binding summary.
const createdRef = { id: "owned", revision: 1, environment_id: "synthetic-env", profile_digest: "synthetic-profile", root_id: "synthetic-root" };
beforeEach(() => { vi.restoreAllMocks(); });

it("always offers connection setup and separates the unconfigured subscription from the shared API", async () => {
  const connectApi = vi.fn();
  const fetch = vi.fn(async () => response(unavailable));
  vi.stubGlobal("fetch", fetch);
  render(<ClaudeExecutionSettings accountId="a" onExpired={vi.fn()} onVerified={vi.fn()} onConnectApi={connectApi} />);
  const user = userEvent.setup();
  const connect = screen.getByRole("button", { name: "Claude 연결" });
  expect(connect).toBeEnabled();
  await user.click(connect);
  expect(await screen.findByText("이 서버의 구독 실행 환경이 아직 준비되지 않았습니다.")).toBeInTheDocument();
  expect(screen.getByRole("link", { name: "서버 설정 안내 열기" })).toHaveAttribute("href", expect.stringContaining("docs/claude-execution.md"));
  expect(screen.queryByRole("button", { name: "내 Claude 연결 추가" })).not.toBeInTheDocument();
  expect(screen.queryByLabelText(/정확한 모델/)).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Claude API 연결" }));
  expect(connectApi).toHaveBeenCalledOnce();
  expect(fetch).toHaveBeenCalledTimes(1);
});

it("guides members to their administrator without exposing an API credential form", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => response(unavailable)));
  render(<ClaudeExecutionSettings accountId="member" onExpired={vi.fn()} onVerified={vi.fn()} />);
  await userEvent.setup().click(screen.getByRole("button", { name: "Claude 연결" }));
  expect(await screen.findByText(/설치 관리자에게 Claude 구독 실행 환경/)).toBeInTheDocument();
  expect(screen.getByText(/공유 API 연결은 관리자가 추가합니다/)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "Claude API 연결" })).not.toBeInTheDocument();
});

it("ends loading on a failed status request and lets refresh reveal newly configured setup", async () => {
  let configured = false;
  vi.stubGlobal("fetch", vi.fn(async () => configured
    ? response({ ...unavailable, runtime_configured: true })
    : response({ code: "unavailable", message: "private-server-details" }, 503)));
  render(<ClaudeExecutionSettings accountId="a" onExpired={vi.fn()} onVerified={vi.fn()} />);
  const user = userEvent.setup();
  expect(await screen.findByRole("alert")).toHaveTextContent("Claude 연결 상태를 불러오지 못했습니다");
  expect(screen.queryByText("Claude 연결 상태를 확인하는 중…")).not.toBeInTheDocument();
  expect(screen.queryByText(/private-server-details/)).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Claude 연결" }));
  expect(screen.queryByText("이 서버의 구독 실행 환경이 아직 준비되지 않았습니다.")).not.toBeInTheDocument();
  configured = true;
  await user.click(screen.getByRole("button", { name: "Claude 연결 새로고침" }));
  expect(await screen.findByRole("button", { name: "내 Claude 연결 추가" })).toBeEnabled();
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});

it("creates an owner binding, verifies an exact model, and shows cleanup pending", async () => {
  const verified = vi.fn();
  const calls: unknown[] = [];
  let created = false;
  let retired = false;
  vi.stubGlobal("fetch", vi.fn(async (url: RequestInfo | URL, init?: RequestInit) => {
    const path = String(url);
    if (init?.body) calls.push(JSON.parse(String(init.body)));
    if (path.endsWith("/settings/claude") && init?.method === "POST") {
      created = true; return response(createdRef);
    }
    if (path.endsWith("/owned/probe")) return response({ model: "claude-fixture-v1" });
    if (path.endsWith("/owned") && init?.method === "DELETE") {
      retired = true; return response({ local_disconnected: true });
    }
    return response({ runtime_configured: true, text_only: true, bindings: created
      ? [{ ...binding, status: retired ? "disconnected" : "pending", cleanup_state: retired ? "pending" : "not_requested" }] : [] });
  }));
  render(<ClaudeExecutionSettings accountId="a" onExpired={vi.fn()} onVerified={verified} />);
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "Claude 연결" }));
  await user.click(await screen.findByRole("button", { name: "내 Claude 연결 추가" }));
  const owned = await screen.findByRole("group", { name: binding.label });
  expect(within(owned).getByText(/moosuri subscriptions claude-login/)).toHaveTextContent("--binding owned --revision 1");
  await user.type(await screen.findByLabelText("Claude 모델 내 Claude"), "claude-fixture-v1");
  await user.click(screen.getByRole("button", { name: "Claude 텍스트 연결 확인" }));
  await waitFor(() => expect(verified).toHaveBeenCalledOnce());
  expect(calls).toContainEqual({ revision: 1, model: "claude-fixture-v1" });
  await user.click(screen.getByRole("button", { name: "Claude 연결 해제" }));
  expect(await screen.findByText(/개인 로그인 파일 정리 대기/)).toBeInTheDocument();
});

it("keeps owned disconnection available even after the operator removes the runtime", async () => {
  let removed = false;
  const fetch = vi.fn(async (_url: RequestInfo | URL, init?: RequestInit) => {
    if (init?.method === "DELETE") { removed = true; return response({ local_disconnected: true }); }
    return response({ ...unavailable, bindings: [{ ...binding, status: removed ? "disconnected" : "active", cleanup_state: removed ? "pending" : "not_requested" }] });
  });
  vi.stubGlobal("fetch", fetch);
  render(<ClaudeExecutionSettings accountId="a" onExpired={vi.fn()} onVerified={vi.fn()} />);
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "Claude 연결 해제" }));
  expect(await screen.findByText(/개인 로그인 파일 정리 대기/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Claude 연결 해제" })).toBeDisabled();
  expect(fetch.mock.calls.find(([, init]) => init?.method === "DELETE")?.[1]).toMatchObject({
    headers: expect.objectContaining({ "X-Moosuri-Account": "a" }), body: JSON.stringify({ revision: 1 }),
  });
});

it("preserves a created receipt if the list refresh fails instead of suggesting another create", async () => {
  let created = false;
  vi.stubGlobal("fetch", vi.fn(async (_url: RequestInfo | URL, init?: RequestInit) => {
    if (init?.method === "POST") { created = true; return response(createdRef); }
    return created ? response({ code: "unavailable" }, 503) : response({ ...unavailable, runtime_configured: true });
  }));
  render(<ClaudeExecutionSettings accountId="a" onExpired={vi.fn()} onVerified={vi.fn()} />);
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "Claude 연결" }));
  await user.click(await screen.findByRole("button", { name: "내 Claude 연결 추가" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("요청은 처리했지만 최신 상태를 불러오지 못했습니다");
  expect(screen.getByRole("group", { name: "내 Claude" })).toHaveTextContent("공식 로그인과 모델 확인 대기");
  expect(screen.getByLabelText("Claude 모델 내 Claude")).toBeEnabled();
});

it("ignores held creation and resets the guide when switching accounts", async () => {
  let release!: (value: Response) => void;
  const verified = vi.fn();
  vi.stubGlobal("fetch", vi.fn((_url: RequestInfo | URL, init?: RequestInit) => {
    if (init?.method === "POST") return new Promise<Response>(resolve => { release = resolve; });
    return Promise.resolve(response({ ...unavailable, runtime_configured: true }));
  }));
  const view = render(<ClaudeExecutionSettings accountId="a" onExpired={vi.fn()} onVerified={verified} />);
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "Claude 연결" }));
  await user.click(await screen.findByRole("button", { name: "내 Claude 연결 추가" }));
  view.rerender(<ClaudeExecutionSettings accountId="b" onExpired={vi.fn()} onVerified={verified} />);
  await act(async () => { release(response(createdRef)); });
  expect(screen.queryByRole("group", { name: "내 Claude" })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Claude 연결" })).toHaveAttribute("aria-expanded", "false");
  expect(verified).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "Claude 연결" }));
  expect(await screen.findByRole("button", { name: "내 Claude 연결 추가" })).toBeEnabled();
});

it("expires a rejected session without calling it missing runtime configuration", async () => {
  const expired = vi.fn();
  vi.stubGlobal("fetch", vi.fn(async () => response({ code: "unauthorized" }, 401)));
  render(<ClaudeExecutionSettings accountId="a" onExpired={expired} onVerified={vi.fn()} />);
  await waitFor(() => expect(expired).toHaveBeenCalledOnce());
  expect(screen.queryByText("이 서버의 구독 실행 환경이 아직 준비되지 않았습니다.")).not.toBeInTheDocument();
});
