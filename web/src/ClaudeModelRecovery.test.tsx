import { StrictMode } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vitest";
import { ClaudeExecutionSettings } from "./ClaudeExecutionSettings";
import { PersonalModelSettings } from "./ProviderSettings";

const response = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { "Content-Type": "application/json" } });
const binding = { id: "owned", revision: 3, label: "Synthetic Claude", status: "pending", cleanup_state: "not_requested" };
const list = (status = "pending") => ({ runtime_configured: true, text_only: true, bindings: [{ ...binding, status }] });
const choice = { source: "personal_claude_code", provider_id: "owned", provider_revision: 3, provider_name: binding.label, kind: "anthropic", model: "claude-synthetic-v1", effort: null, function_tools: false, verified_at: 1 };
const settings = (ready = false) => ({ selection: null, effective: null, choices: ready ? [choice] : [] });
const props = { accountId: "owner", onExpired: vi.fn(), onVerified: vi.fn() };
beforeEach(() => { vi.restoreAllMocks(); props.onExpired = vi.fn(); props.onVerified = vi.fn(); });

it("recovers an existing pending login on entry and reloads persisted choices without a probe loop", async () => {
  let ready = false;
  let syncs = 0;
  let releaseOld!: (value: Response) => void;
  let reads = 0;
  const fetch = vi.fn(async (url: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const path = String(url);
    if (path.endsWith("/settings/model")) {
      reads++;
      if (reads === 1) return new Promise(resolve => { releaseOld = resolve; });
      return response(settings(ready));
    }
    if (path.endsWith("/settings/claude")) return response(list(ready ? "active" : "pending"));
    if (path.endsWith("/owned/models")) {
      syncs++; expect(init?.method).toBe("POST");
      expect(JSON.parse(String(init?.body))).toEqual({ revision: 3 });
      expect(init?.headers).toMatchObject({ "X-Moosuri-Account": "owner" });
      ready = true; return response({ status: "ready", models: [choice.model] });
    }
    if (String(url).endsWith("/settings/compatible-providers")) return response({providers:[],default_selection:null});
    return response([]);
  });
  vi.stubGlobal("fetch", fetch);
  render(<PersonalModelSettings accountId="owner" onExpired={props.onExpired} />);
  expect(await screen.findByRole("option", { name: /claude-synthetic-v1/ })).toBeInTheDocument();
  await act(async () => releaseOld(response(settings(false))));
  expect(screen.getByRole("option", { name: /claude-synthetic-v1/ })).toBeInTheDocument();
  await userEvent.setup().click(screen.getByRole("button", { name: "새로고침" }));
  await waitFor(() => expect(reads).toBe(3));
  expect(syncs).toBe(1);
  expect(fetch.mock.calls.some(([url]) => String(url).endsWith("/probe"))).toBe(false);
});

it("main model refresh retries recovery after login-required and adds choices when ready", async () => {
  let ready = false; let loggedIn = false; let syncs = 0;
  vi.stubGlobal("fetch", vi.fn(async (url: RequestInfo | URL) => {
    const path = String(url);
    if (path.endsWith("/settings/model")) return response(settings(ready));
    if (path.endsWith("/settings/claude")) return response(list(ready ? "active" : "pending"));
    if (path.endsWith("/owned/models")) { syncs++; ready = loggedIn; return response({ status: ready ? "ready" : "login_required", models: ready ? [choice.model] : [] }); }
    if (String(url).endsWith("/settings/compatible-providers")) return response({providers:[],default_selection:null});
    return response([]);
  }));
  render(<PersonalModelSettings accountId="owner" onExpired={props.onExpired} />);
  await screen.findByText(/공식 로그인이 필요합니다/);
  expect(within(screen.getByLabelText("모델")).getAllByRole("option")).toHaveLength(1);
  loggedIn = true;
  await userEvent.setup().click(screen.getByRole("button", { name: "새로고침" }));
  expect(await screen.findByRole("option", { name: /claude-synthetic-v1/ })).toBeInTheDocument();
  expect(syncs).toBe(2);
});

it("automatically checks models after successful official login with no manual model ID", async () => {
  let loggedIn = false; let syncs = 0;
  vi.stubGlobal("fetch", vi.fn(async (url: RequestInfo | URL, init?: RequestInit) => {
    const path = String(url);
    if (path.endsWith("/settings/claude")) return response(list());
    if (path.endsWith("/owned/models")) { syncs++; return response({ status: loggedIn ? "ready" : "login_required", models: loggedIn ? [choice.model] : [] }); }
    if (path.endsWith("/login")) return response({ attempt_id: "synthetic", status: "awaiting_code", authorization_url: "https://claude.com/cai/oauth/authorize?state=synthetic", expires_in_seconds: 290 });
    if (init?.method === "POST") { loggedIn = true; return response({ attempt_id: "synthetic", status: "succeeded", authorization_url: null, expires_in_seconds: 0 }); }
    throw new Error("Unexpected synthetic route");
  }));
  render(<ClaudeExecutionSettings {...props} />);
  await screen.findByText(/공식 로그인이 필요합니다/);
  const user = userEvent.setup();
  await user.click(screen.getByRole("button", { name: "구독 로그인" }));
  await user.type(screen.getByLabelText("2. Claude에서 받은 로그인 코드"), "synthetic-code");
  await user.click(screen.getByRole("button", { name: "코드로 연결 완료" }));
  await screen.findByText(/Claude 모델을 준비했습니다/);
  expect(props.onVerified).toHaveBeenCalledOnce();
  expect(syncs).toBe(2);
  expect(screen.getByText("정확한 모델 직접 확인 (고급)").closest("details")).not.toHaveAttribute("open");
});

it.each(["unavailable", "login_required"])("shows useful %s guidance and deduplicates simultaneous retries", async status => {
  let syncs = 0; let release!: (value: Response) => void;
  vi.stubGlobal("fetch", vi.fn(async (url: RequestInfo | URL) => {
    if (String(url).endsWith("/models")) { syncs++; return syncs === 1 ? response({ status, models: [] }) : new Promise<Response>(resolve => { release = resolve; }); }
    return response(list());
  }));
  render(<ClaudeExecutionSettings {...props} />);
  await screen.findByText(status === "login_required" ? /공식 로그인이 필요합니다/ : /사용 가능한 모델을 확인하지 못했습니다/);
  const retry = screen.getByRole("button", { name: "모델 목록 다시 확인" });
  act(() => { fireEvent.click(retry); fireEvent.click(retry); });
  await waitFor(() => expect(syncs).toBe(2));
  await act(async () => release(response({ status: "ready", models: [choice.model] })));
  await screen.findByText(/Claude 모델을 준비했습니다/);
  expect(syncs).toBe(2); expect(props.onVerified).toHaveBeenCalledOnce();
});

it("fences a held recovery response and callbacks when the owner changes", async () => {
  let release!: (value: Response) => void;
  const fetch = vi.fn(async (url: RequestInfo | URL, init?: RequestInit) => {
    const owner = (init?.headers as Record<string, string>)["X-Moosuri-Account"];
    if (String(url).endsWith("/models")) return new Promise<Response>(resolve => { release = resolve; });
    return response(owner === "owner" ? list() : { runtime_configured: true, bindings: [], text_only: true });
  });
  vi.stubGlobal("fetch", fetch);
  const view = render(<ClaudeExecutionSettings {...props} />);
  await waitFor(() => expect(release).toBeDefined());
  view.rerender(<ClaudeExecutionSettings {...props} accountId="other" />);
  await act(async () => release(response({ status: "ready", models: [choice.model] })));
  expect(props.onVerified).not.toHaveBeenCalled();
  expect(screen.queryByText(/Claude 모델을 준비했습니다/)).not.toBeInTheDocument();
  expect(screen.queryByRole("group", { name: binding.label })).not.toBeInTheDocument();
});

it("keeps active bindings available without automatic model requests", async () => {
  const fetch = vi.fn(async (_url: RequestInfo | URL) => response(list("active")));
  vi.stubGlobal("fetch", fetch);
  const view = render(<ClaudeExecutionSettings {...props} />);
  await screen.findByText("텍스트 확인됨");
  view.rerender(<ClaudeExecutionSettings {...props} refreshKey={1} />);
  await waitFor(() => expect(fetch).toHaveBeenCalledTimes(2));
  expect(fetch.mock.calls.every(call => String(call[0]).endsWith("/settings/claude"))).toBe(true);
  expect(props.onVerified).not.toHaveBeenCalled();
});

it.each([401, 409, 429, 503])("handles a %s model-sync failure without exposing provider details or retrying automatically", async status => {
  let syncs = 0;
  vi.stubGlobal("fetch", vi.fn(async (url: RequestInfo | URL) => {
    if (String(url).endsWith("/models")) { syncs++; return response({ code: "unavailable", message: "synthetic-private-provider-details" }, status); }
    return response(list());
  }));
  render(<ClaudeExecutionSettings {...props} />);
  if (status === 401) await waitFor(() => expect(props.onExpired).toHaveBeenCalledOnce());
  else {
    expect(await screen.findByRole("alert")).toHaveTextContent(status === 409 ? "연결 정보가 바뀌었습니다" : status === 429 ? "다른 연결 확인이 진행 중입니다" : "Claude 모델을 불러오지 못했습니다");
    await waitFor(() => expect(screen.getByRole("button", { name: "모델 목록 다시 확인" })).toBeEnabled());
  }
  expect(syncs).toBe(1); expect(props.onVerified).not.toHaveBeenCalled();
  expect(screen.queryByText(/synthetic-private-provider-details/)).not.toBeInTheDocument();
});

it("recovers pending bindings only once through the app's StrictMode effect replay", async () => {
  let ready = false; let syncs = 0;
  vi.stubGlobal("fetch", vi.fn(async (url: RequestInfo | URL) => {
    if (String(url).endsWith("/models")) { syncs++; ready = true; return response({ status: "ready", models: [choice.model] }); }
    return response(list(ready ? "active" : "pending"));
  }));
  render(<StrictMode><ClaudeExecutionSettings {...props} /></StrictMode>);
  await screen.findByText(/Claude 모델을 준비했습니다/);
  expect(syncs).toBe(1); expect(props.onVerified).toHaveBeenCalledOnce();
});

it("force-refreshes an active binding's missing models, reloads partial success and offers a fresh connection for another account", async () => {
  const second = { ...choice, model: "claude-synthetic-sonnet" };
  let added = false;
  const modelCalls: unknown[] = [];
  vi.stubGlobal("fetch", vi.fn(async (url: RequestInfo | URL, init?: RequestInit) => {
    const path = String(url);
    if (path.endsWith("/settings/model")) return response({ ...settings(), choices: added ? [choice, second] : [choice] });
    if (path.endsWith("/settings/claude")) return response(list("active"));
    if (path.endsWith("/owned/models")) {
      modelCalls.push(JSON.parse(String(init?.body)));
      added = true;
      // One additional tier succeeded; another remains unavailable.
      return response({ status: "ready", models: [choice.model, second.model] });
    }
    if (String(url).endsWith("/settings/compatible-providers")) return response({providers:[],default_selection:null});
    return response([]);
  }));
  render(<PersonalModelSettings accountId="owner" onExpired={props.onExpired} />);
  await screen.findByRole("option", { name: /claude-synthetic-v1/ });
  expect(modelCalls).toHaveLength(0);
  const owned = screen.getByRole("group", { name: binding.label });
  expect(within(owned).queryByRole("button", { name: /구독 로그인/ })).not.toBeInTheDocument();
  expect(within(owned).queryByText("터미널로 연결하기 (고급)")).not.toBeInTheDocument();
  expect(within(owned).getByText(/다른 Claude 계정을 사용하려면/)).toBeInTheDocument();
  await userEvent.setup().click(within(owned).getByRole("button", { name: "모델 목록 다시 확인" }));
  expect(await screen.findByRole("option", { name: /claude-synthetic-sonnet/ })).toBeInTheDocument();
  expect(modelCalls).toEqual([{ revision: 3, force: true }]);
  await userEvent.setup().click(screen.getByRole("button", { name: "새로고침" }));
  await waitFor(() => expect(screen.getByRole("button", { name: "새로고침" })).toBeEnabled());
  expect(modelCalls).toHaveLength(1);
});

it("retains successful pending login while a parent refresh is held and recovers once without force", async () => {
  let reads = 0;
  let releaseRefresh!: (value: Response) => void;
  let releaseLogin!: (value: Response) => void;
  let loggedIn = false;
  const modelCalls: unknown[] = [];
  const waiting = { attempt_id: "synthetic-queued", status: "verifying", authorization_url: null, expires_in_seconds: 290 };
  vi.stubGlobal("fetch", vi.fn(async (url: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const path = String(url);
    if (path.endsWith("/settings/claude")) {
      reads++;
      if (reads === 2) return new Promise(resolve => { releaseRefresh = resolve; });
      return response(list(loggedIn ? "active" : "pending"));
    }
    if (path.endsWith("/owned/models")) {
      modelCalls.push(JSON.parse(String(init?.body)));
      return response({ status: loggedIn ? "ready" : "login_required", models: loggedIn ? [choice.model] : [] });
    }
    if (path.endsWith("/login")) return response(waiting);
    return new Promise(resolve => { releaseLogin = resolve; });
  }));
  const view = render(<ClaudeExecutionSettings {...props} />);
  await screen.findByText(/공식 로그인이 필요합니다/);
  await userEvent.setup().click(screen.getByRole("button", { name: "구독 로그인" }));
  await waitFor(() => expect(releaseLogin).toBeDefined());
  view.rerender(<ClaudeExecutionSettings {...props} refreshKey={1} />);
  await waitFor(() => expect(releaseRefresh).toBeDefined());
  await act(async () => {
    loggedIn = true;
    releaseLogin(response({ ...waiting, status: "succeeded" }));
  });
  await screen.findByText(/구독 로그인을 완료했습니다/);
  // The held refresh sees this binding active, so the queued login must still
  // trigger its own recovery after the unrelated list request completes.
  await act(async () => releaseRefresh(response(list("active"))));
  await screen.findByText(/Claude 모델을 준비했습니다/);
  expect(modelCalls).toEqual([{ revision: 3 }, { revision: 3 }]);
  expect(props.onVerified).toHaveBeenCalledOnce();
  expect(screen.queryByRole("button", { name: /구독 로그인/ })).not.toBeInTheDocument();
});

it("backfills effort for an already verified login and saves a per-model choice", async () => {
  let discovered = false;
  let saved: unknown;
  vi.stubGlobal("fetch", vi.fn(async (url: RequestInfo | URL, init?: RequestInit) => {
    const path = String(url);
    if (path.endsWith("/settings/model")) {
      if (init?.method === "PUT") saved = JSON.parse(String(init.body)).selection;
      return response({ selection: saved ?? null, effective: saved ? {...choice, effort:"high"} : null,
        choices: discovered ? [choice, {...choice, effort:"high"}] : [choice] });
    }
    if (path.endsWith("/settings/claude")) return response({...list("active"), bindings:[{...binding,status:"active",effort_metadata_pending:!discovered}]});
    if (path.endsWith("/owned/models")) { discovered = true; return response({status:"ready", models:[choice.model]}); }
    if (String(url).endsWith("/settings/compatible-providers")) return response({providers:[],default_selection:null});
    return response([]);
  }));
  render(<PersonalModelSettings accountId="owner" onExpired={props.onExpired}/>);
  const user = userEvent.setup();
  await screen.findByText(/Claude 모델을 준비했습니다/);
  await user.selectOptions(screen.getByLabelText("모델"), await screen.findByRole("option", {name:/claude-synthetic-v1/}));
  await user.selectOptions(screen.getByLabelText("추론 강도"), screen.getByRole("option", {name:"high"}));
  await user.click(screen.getByRole("button", {name:"선택 저장"}));
  await screen.findByText("내 모델을 저장했습니다.");
  expect(saved).toMatchObject({source:"personal_claude_code", model:choice.model, effort:"high"});
});
