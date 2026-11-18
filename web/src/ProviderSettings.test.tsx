import { openWorkspaceMenu } from "./test-workspace";
import { StrictMode } from "react";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import type {
  ModelChoice,
  ModelSelection,
  ModelSettings,
  ProviderSummary,
  ProvidersResponse,
  SessionView,
} from "../shared/api-types";

const admin: SessionView = {
  user: {
    id: "admin-a",
    name: "Admin Example",
    email: "admin@example.test",
    role: "admin",
    must_change_password: false,
  },
  password_change_required: false,
};
const member: SessionView = {
  user: {
    id: "member-b",
    name: "Member Example",
    email: "member@example.test",
    role: "member",
    must_change_password: false,
  },
  password_change_required: false,
};
const provider: ProviderSummary = {
  id: "provider-a",
  name: "공유 연결",
  revision: 1,
  enabled: true,
  has_credential: true,
  is_default: false,
  config: {
    kind: "open_ai",
    base_url: null,
    allowed_ips: [],
    router_providers: [],
    zero_data_retention: false,
  },
  choices: [],
};
const high: ModelChoice = {
  provider_id: provider.id,
  provider_revision: 1,
  provider_name: provider.name,
  kind: "open_ai",
  model: "synthetic-text",
  effort: "high",
  function_tools: false,
  verified_at: 100,
};
const low: ModelChoice = { ...high, effort: "low" };
const selection = (choice: ModelChoice): ModelSelection => ({
  provider_id: choice.provider_id,
  provider_revision: choice.provider_revision,
  model: choice.model,
  effort: choice.effort,
});
const key = (choice: ModelChoice) =>
  JSON.stringify([
    choice.provider_id,
    choice.provider_revision,
    choice.model,
    choice.effort,
  ]);
const response = (body: unknown, status = 200) =>
  new Response(status === 204 ? null : JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
type Call = {
  path: string;
  method: string;
  body: Record<string, unknown> | undefined;
  init: RequestInit;
};
type Handler = (call: Call) => Promise<Response> | Response | undefined;
function server(
  handler?: Handler,
  session: SessionView | null = admin,
  providers: ProvidersResponse = { providers: [], default_selection: null },
  settings: ModelSettings = { selection: null, effective: null, choices: [] },
) {
  const state = { session, providers, settings };
  const calls: Call[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn((url: string, init: RequestInit = {}) => {
      const path = url.replace("/api/v1", "");
      const call: Call = {
        path,
        method: init.method ?? "GET",
        body: init.body ? JSON.parse(String(init.body)) : undefined,
        init,
      };
      calls.push(call);
      const handled = handler?.(call);
      if (handled) return Promise.resolve(handled);
      if (path === "/system")
        return Promise.resolve(
          response({
            initialized: true,
            version: "0.1.0",
            stage: "provider-settings",
          }),
        );
      if (path === "/auth/session")
        return Promise.resolve(
          state.session
            ? response(state.session)
            : response({ code: "unauthorized" }, 401),
        );
      if (path === "/auth/logout") {
        state.session = null;
        return Promise.resolve(response(null, 204));
      }
      if (path === "/auth/login") {
        state.session = member;
        return Promise.resolve(response(member));
      }
      if (path === "/threads")
        return Promise.resolve(response({ threads: [] }));
      if (path === "/admin/providers" && call.method === "GET")
        return Promise.resolve(response(state.providers));
      if (path === "/settings/model" && call.method === "GET")
        return Promise.resolve(response(state.settings));
      if (path === "/settings/claude" && call.method === "GET")
        return Promise.resolve(response({ runtime_configured: false, bindings: [], text_only: true }));
      if (path === "/settings/compatible-providers") return Promise.resolve(response({providers:[],default_selection:null}));
      if (path === "/settings/chatgpt" && call.method === "GET")
        return Promise.resolve(response([]));
      return Promise.resolve(response({ code: "unexpected_test_route" }, 404));
    }),
  );
  return { state, calls };
}
function open(path: string) {
  window.history.replaceState(null, "", path);
  render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}
beforeEach(() => {
  vi.stubGlobal("BroadcastChannel", undefined);
});

describe("shared model administration", () => {
  it("opens the real Anthropic editor from My model and saves through the shared admin endpoint", async () => {
    const user = userEvent.setup();
    const claude: ProviderSummary = { ...provider, name: "Claude API", config: { ...provider.config, kind: "anthropic" } };
    const { calls } = server(call => call.path === "/admin/providers" && call.method === "POST"
      ? response(claude, 201) : undefined);
    open("/settings/model");
    await user.click(await screen.findByRole("button", { name: "Claude 연결" }));
    await user.click(screen.getByRole("button", { name: "Claude API 연결" }));
    const editor = await screen.findByRole("region", { name: "새 모델 연결" });
    expect(within(editor).getByLabelText("연결 방식")).toHaveValue("anthropic");
    expect(within(editor).getByLabelText("연결 이름")).toHaveValue("Claude API");
    expect(within(editor).getByLabelText("API 키")).toHaveFocus();
    await user.type(within(editor).getByLabelText("API 키"), "synthetic-anthropic-key");
    await user.click(within(editor).getByRole("button", { name: "연결 저장" }));
    expect(await screen.findByText("연결을 저장했습니다. 사용할 모델을 확인해 주세요.")).toBeInTheDocument();
    expect(calls.find(call => call.path === "/admin/providers" && call.method === "POST")?.body).toMatchObject({
      config: { kind: "anthropic" }, credential: { action: "replace", value: "synthetic-anthropic-key" },
    });
    expect(screen.queryByDisplayValue("synthetic-anthropic-key")).not.toBeInTheDocument();
  });

  it("does not expose shared credential creation to a member through the Claude entry or a direct link", async () => {
    const { calls } = server(undefined, member);
    open("/settings/model");
    await screen.findByRole("heading", {name: "This page is for administrators."});
    expect(screen.queryByRole("button", { name: "Claude 연결" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Claude API 연결" })).not.toBeInTheDocument();
    await act(async () => {
      window.history.pushState(null, "", "/admin/providers?connect=anthropic");
      window.dispatchEvent(new PopStateEvent("popstate"));
    });
    expect(await screen.findByText("This page is for administrators.")).toBeInTheDocument();
    expect(screen.queryByLabelText("API 키")).not.toBeInTheDocument();
    expect(calls.filter(call => call.path === "/admin/providers")).toEqual([]);
  });

  it("creates a compatible connection without a key and probes without an effort override", async () => {
    const user = userEvent.setup();
    const local: ProviderSummary = {
      ...provider,
      name: "로컬 연결",
      has_credential: false,
      config: {
        ...provider.config,
        kind: "compatible",
        base_url: "http://127.0.0.1:1234/v1",
        allowed_ips: ["127.0.0.1"],
      },
    };
    const choice: ModelChoice = {
      ...high,
      provider_name: local.name,
      kind: "compatible",
      effort: null,
    };
    const { calls } = server((call) =>
      call.path === "/admin/providers" && call.method === "POST"
        ? response(local, 201)
        : call.path.endsWith("/probe")
          ? response(choice)
          : undefined,
    );
    open("/admin/providers");
    await user.click(await screen.findByRole("button", { name: /연결 추가/ }));
    await user.type(screen.getByLabelText("연결 이름"), local.name);
    await user.selectOptions(screen.getByLabelText("연결 방식"), "compatible");
    await user.type(
      screen.getByLabelText("서버 API 주소"),
      local.config.base_url!,
    );
    await user.type(screen.getByLabelText("허용 IP"), "127.0.0.1");
    await user.click(screen.getByRole("button", { name: "연결 저장" }));
    await user.click(await screen.findByText("모델 확인하기"));
    expect(screen.queryByLabelText("추론 강도 (선택)")).not.toBeInTheDocument();
    await user.type(screen.getByLabelText("모델 ID"), choice.model);
    await user.click(screen.getByRole("button", { name: "연결·모델 확인" }));
    await screen.findByText(/모델과 추론 강도를 확인했습니다/);
    expect(
      calls.find(
        (call) => call.path === "/admin/providers" && call.method === "POST",
      )?.body,
    ).toMatchObject({ config: local.config, credential: { action: "clear" } });
    expect(calls.find((call) => call.path.endsWith("/probe"))?.body).toEqual({
      revision: 1,
      model: choice.model,
      effort: null,
    });
  });

  it("clears the key even when saving fails and does not print a returned provider message", async () => {
    const user = userEvent.setup();
    server((call) =>
      call.path === "/admin/providers" && call.method === "POST"
        ? response(
            {
              code: "model_protocol_failed",
              message: "synthetic-key-do-not-display",
            },
            422,
          )
        : undefined,
    );
    open("/admin/providers");
    await user.click(await screen.findByRole("button", { name: /연결 추가/ }));
    await user.type(screen.getByLabelText("연결 이름"), "합성 연결");
    await user.type(
      screen.getByLabelText("API 키"),
      "synthetic-key-do-not-display",
    );
    await user.click(screen.getByRole("button", { name: "연결 저장" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "응답 형식을 확인하지 못했습니다",
    );
    expect(screen.getByLabelText("API 키")).toHaveValue("");
    expect(document.body).not.toHaveTextContent("synthetic-key-do-not-display");
    expect(screen.queryByText(/연결을 저장했습니다/)).not.toBeInTheDocument();
  });

  it("does not offer a model after an identity verification failure or expose raw provider errors", async () => {
    const user = userEvent.setup();
    server(
      (call) =>
        call.path.endsWith("/probe")
          ? response(
              {
                code: "model_identity_unverified",
                message: "synthetic-private-provider-body",
              },
              422,
            )
          : undefined,
      admin,
      { providers: [provider], default_selection: null },
    );
    open("/admin/providers");
    await user.click(await screen.findByText("모델 확인하기"));
    await user.type(screen.getByLabelText("모델 ID"), high.model);
    await user.click(screen.getByRole("button", { name: "연결·모델 확인" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "모델 목록의 정확한 버전 ID",
    );
    expect(
      within(screen.getByLabelText("기본 모델")).getAllByRole(
        "option",
      ),
    ).toHaveLength(1);
    expect(
      screen.queryByText(/synthetic-private-provider-body/),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByText(/모델과 추론 강도를 확인했습니다/),
    ).not.toBeInTheDocument();
  });

  it("creates an explicit Router connection, discovers IDs, probes one exact pair, and then permits default selection", async () => {
    const user = userEvent.setup();
    const probe = deferred<Response>();
    const created: ProviderSummary = {
      ...provider,
      name: "내 Router",
      config: {
        ...provider.config,
        kind: "open_router",
        router_providers: ["approved-provider"],
        zero_data_retention: true,
      },
    };
    const choice: ModelChoice = {
      ...high,
      provider_name: created.name,
      kind: "open_router",
    };
    const { calls } = server((call) => {
      if (call.path === "/admin/providers" && call.method === "POST")
        return response(created, 201);
      if (call.path.endsWith("/models"))
        return response({
          models: [{ id: choice.model, name: "Synthetic listed model" }],
          truncated: true,
        });
      if (call.path.endsWith("/probe")) return probe.promise;
      if (call.path === "/admin/providers/default")
        return response({
          providers: [{ ...created, choices: [choice], is_default: true }],
          default_selection: selection(choice),
        });
    });
    open("/admin/providers");
    await user.click(await screen.findByRole("button", { name: /연결 추가/ }));
    await user.type(screen.getByLabelText("연결 이름"), created.name);
    await user.selectOptions(screen.getByLabelText("연결 방식"), "open_router");
    await user.type(
      screen.getByLabelText("사용할 제공업체"),
      "approved-provider",
    );
    await user.click(
      screen.getByLabelText("데이터를 보존하지 않는 경로만 사용"),
    );
    await user.type(
      screen.getByLabelText("API 키"),
      "synthetic-credential-never-echo",
    );
    await user.click(screen.getByRole("button", { name: "연결 저장" }));
    await screen.findByRole("heading", { name: created.name });
    expect(
      screen.queryByDisplayValue("synthetic-credential-never-echo"),
    ).not.toBeInTheDocument();
    expect(document.body).not.toHaveTextContent(
      "synthetic-credential-never-echo",
    );
    expect(
      calls.find(
        (call) => call.path === "/admin/providers" && call.method === "POST",
      )?.body,
    ).toEqual({
      name: created.name,
      config: created.config,
      enabled: true,
      credential: {
        action: "replace",
        value: "synthetic-credential-never-echo",
      },
    });
    await user.click(screen.getByText("모델 확인하기"));
    await user.click(
      screen.getByRole("button", { name: "모델 목록 불러오기" }),
    );
    await screen.findByText(/일부 모델만 표시됩니다/);
    const defaultSelect = screen.getByLabelText("기본 모델");
    expect(within(defaultSelect).getAllByRole("option")).toHaveLength(1);
    await user.type(screen.getByLabelText("모델 ID"), choice.model);
    await user.type(screen.getByLabelText("추론 강도 (선택)"), choice.effort!);
    await user.click(screen.getByRole("button", { name: "연결·모델 확인" }));
    expect(screen.getByRole("button", { name: /연결 추가/ })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "연결·모델 확인" }));
    expect(calls.filter((call) => call.path.endsWith("/probe"))).toHaveLength(
      1,
    );
    expect(within(defaultSelect).getAllByRole("option")).toHaveLength(1);
    await act(async () => {
      probe.resolve(response(choice));
    });
    await screen.findByText(/모델과 추론 강도를 확인했습니다/);
    await user.selectOptions(defaultSelect, key({...choice, effort: null}));
    await user.click(screen.getByRole("button", { name: "기본 모델 저장" }));
    await screen.findByText("설치 기본 모델을 저장했습니다.");
    expect(calls.find((call) => call.path.endsWith("/probe"))?.body).toEqual({
      revision: 1,
      model: choice.model,
      effort: "high",
    });
    expect(
      calls.find((call) => call.path === "/admin/providers/default")?.body,
    ).toEqual({ selection: selection(choice) });
    expect(
      calls
        .filter((call) => call.path.startsWith("/admin/providers"))
        .every(
          (call) =>
            new Headers(call.init.headers).get("X-Moosuri-Account") ===
            admin.user.id,
        ),
    ).toBe(true);
    expect(localStorage.length).toBe(0);
    expect(sessionStorage.length).toBe(0);
  });

  it("requires replacing or explicitly clearing a stored key when configuration changes, and invalidates old verified choices", async () => {
    const user = userEvent.setup();
    const existing = { ...provider, choices: [high] };
    const updated = {
      ...provider,
      revision: 2,
      enabled: false,
      has_credential: false,
      config: { ...provider.config, kind: "anthropic" as const },
      choices: [],
    };
    const { calls } = server(
      (call) =>
        call.path === `/admin/providers/${provider.id}` && call.method === "PUT"
          ? response(updated)
          : undefined,
      admin,
      { providers: [existing], default_selection: selection(high) },
    );
    open("/admin/providers");
    await user.click(await screen.findByRole("button", { name: "수정" }));
    await user.selectOptions(screen.getByLabelText("연결 방식"), "anthropic");
    await user.click(screen.getByRole("button", { name: "연결 저장" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "새 API 키를 입력하거나",
    );
    expect(calls.some((call) => call.method === "PUT")).toBe(false);
    await user.click(screen.getByLabelText("저장된 키 제거"));
    await user.click(screen.getByLabelText("이 연결 사용"));
    await user.click(screen.getByRole("button", { name: "연결 저장" }));
    await screen.findByText(
      "연결을 저장했습니다. 사용할 모델을 확인해 주세요.",
    );
    const mutation = calls.find((call) => call.method === "PUT");
    expect(mutation?.body).toMatchObject({
      expected_revision: 1,
      enabled: false,
      credential: { action: "clear" },
      config: updated.config,
    });
    expect(
      within(
        screen.getByRole("list", { name: `${provider.name} 확인된 모델` }),
      ).queryByText(high.model),
    ).not.toBeInTheDocument();
    expect(screen.getByLabelText("기본 모델")).toHaveDisplayValue(
      "현재 기본 모델 · 사용할 수 없음",
    );
  });

  it("keeps an existing credential on a name-only edit and never renders its value", async () => {
    const user = userEvent.setup();
    const { calls } = server(
      (call) =>
        call.method === "PUT"
          ? response({ ...provider, name: "이름 변경", revision: 2 })
          : undefined,
      admin,
      { providers: [provider], default_selection: null },
    );
    open("/admin/providers");
    await user.click(await screen.findByRole("button", { name: "수정" }));
    expect(screen.getByLabelText(/^API 키/)).toHaveValue("");
    await user.clear(screen.getByLabelText("연결 이름"));
    await user.type(screen.getByLabelText("연결 이름"), "이름 변경");
    await user.click(screen.getByRole("button", { name: "연결 저장" }));
    await screen.findByRole("heading", { name: "이름 변경" });
    expect(
      calls.find((call) => call.method === "PUT")?.body?.credential,
    ).toEqual({ action: "keep" });
  });
});

describe("model assignments", () => {
  it("keeps a member's assistant available without exposing the assigned model or configuration", async () => {
    server(undefined, member, undefined, { selection: null, effective: high, choices: [] });
    open("/app");
    await screen.findByRole("heading", {name: "Member님, 무엇을 도와드릴까요?"});
    expect(screen.queryByText(high.model, {exact: false})).not.toBeInTheDocument();
    expect(openWorkspaceMenu().queryByRole("button", {name: "내 모델"})).not.toBeInTheDocument();
    expect(openWorkspaceMenu().queryByRole("button", {name: "Manage installation"})).not.toBeInTheDocument();
  });

  it("lets an administrator select and reset the member's exact model and effort", async () => {
    const user = userEvent.setup();
    const memberPath = `/admin/accounts/${member.user.id}/model`;
    const { calls } = server(call => {
      if (call.path === "/admin/users") return response({ users: [admin.user, member.user] });
      if (call.path !== memberPath) return undefined;
      return response({
        selection: call.body?.selection ?? null,
        effective: call.body?.selection ? low : high,
        choices: [high, low],
      });
    });
    open("/admin");
    const picker = await screen.findByText("사용할 모델 · 추론 강도");
    expect(calls.some(call => call.path === memberPath)).toBe(false);
    await user.click(picker);
    await user.selectOptions(await screen.findByLabelText("모델"), key({...low, effort: null}));
    await user.selectOptions(screen.getByLabelText("추론 강도"), key(low));
    await user.click(screen.getByRole("button", {name: "회원 모델 저장"}));
    await screen.findByText(`현재 적용 · ${provider.name} · ${low.model} · low`);
    await user.selectOptions(screen.getByLabelText("모델"), "");
    await user.click(screen.getByRole("button", {name: "회원 모델 저장"}));
    await screen.findByText(`현재 적용 · ${provider.name} · ${high.model} · high`);
    const writes = calls.filter(call => call.path === memberPath && call.method === "PUT");
    expect(writes.map(call => call.body)).toEqual([{selection: selection(low)}, {selection: null}]);
    expect(writes.every(call => new Headers(call.init.headers).get("X-Moosuri-Account") === admin.user.id)).toBe(true);
  });

  it("saves an exact verified provider/revision/model/effort pair and can return to the installation default", async () => {
    const user = userEvent.setup();
    const { calls } = server(
      (call) =>
        call.path === "/settings/model" && call.method === "PUT"
          ? response({
              selection: call.body?.selection,
              effective: call.body?.selection ? low : high,
              choices: [high, low],
            })
          : undefined,
      admin,
      undefined,
      { selection: null, effective: high, choices: [high, low] },
    );
    open("/settings/model");
    await user.selectOptions(await screen.findByLabelText("모델"), key({...low, effort: null}));
    await user.selectOptions(screen.getByRole("combobox", {name:"추론 강도"}), key(low));
    await user.click(screen.getByRole("button", { name: "선택 저장" }));
    await screen.findByText("내 모델을 저장했습니다.");
    expect(calls.find((call) => call.method === "PUT")?.body).toEqual({
      selection: selection(low),
    });
    expect(screen.getByText(`${provider.name} · low`)).toBeInTheDocument();
    await user.selectOptions(screen.getByLabelText("모델"), "");
    await user.click(screen.getByRole("button", { name: "선택 저장" }));
    await waitFor(() =>
      expect(calls.filter((call) => call.method === "PUT")).toHaveLength(2),
    );
    expect(calls.filter((call) => call.method === "PUT").at(-1)?.body).toEqual({
      selection: null,
    });
    expect(
      calls
        .filter((call) => call.path === "/settings/model")
        .every(
          (call) =>
            new Headers(call.init.headers).get("X-Moosuri-Account") ===
            admin.user.id,
        ),
    ).toBe(true);
  });

  it("does not upgrade a saved revision or silently fall back when the selected connection changed", async () => {
    const newer = { ...high, provider_revision: 2 };
    server(undefined, admin, undefined, {
      selection: selection(high),
      effective: null,
      choices: [newer],
    });
    open("/settings/model");
    expect(await screen.findByLabelText("모델")).toHaveDisplayValue(
      "현재 선택 · 사용할 수 없음",
    );
    expect(
      screen.getByText(/다른 모델로 자동 전환하지 않습니다/),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "선택 저장" })).toBeDisabled();
  });

  it("retains the applied model during a held or failed save and does not send duplicate mutations", async () => {
    const user = userEvent.setup();
    const late = deferred<Response>();
    const { calls } = server(
      (call) =>
        call.path === "/settings/model" && call.method === "PUT"
          ? late.promise
          : undefined,
      admin,
      undefined,
      { selection: selection(high), effective: high, choices: [high, low] },
    );
    open("/settings/model");
    await user.selectOptions(await screen.findByLabelText("모델"), key({...low, effort: null}));
    await user.selectOptions(screen.getByRole("combobox", {name:"추론 강도"}), key(low));
    await user.click(screen.getByRole("button", { name: "선택 저장" }));
    expect(screen.getByRole("button", { name: "저장 중…" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "저장 중…" }));
    expect(calls.filter((call) => call.method === "PUT")).toHaveLength(1);
    expect(screen.getByText(`${provider.name} · high`)).toBeInTheDocument();
    await act(async () => {
      late.resolve(response({ code: "provider_unavailable" }, 409));
    });
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "모델을 사용할 수 없습니다",
    );
    expect(screen.getByText(`${provider.name} · high`)).toBeInTheDocument();
    expect(
      screen.queryByText("내 모델을 저장했습니다."),
    ).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Password")).not.toBeInTheDocument();
  });
});

it("discards a late administrator save and clears its key before another account signs in", async () => {
  const user = userEvent.setup();
  const late = deferred<Response>();
  const { calls } = server((call) =>
    call.path === "/admin/providers" && call.method === "POST"
      ? late.promise
      : undefined,
  );
  open("/admin/providers");
  await user.click(await screen.findByRole("button", { name: /연결 추가/ }));
  await user.type(screen.getByLabelText("연결 이름"), "Admin A private draft");
  await user.type(screen.getByLabelText("API 키"), "synthetic-late-credential");
  await user.click(screen.getByRole("button", { name: "연결 저장" }));
  expect(screen.getByLabelText("API 키")).toHaveValue("");
  const pending = calls.find(
    (call) => call.path === "/admin/providers" && call.method === "POST",
  )!;
  await user.click(openWorkspaceMenu().getByRole("button", { name: "Sign out" }));
  await user.type(await screen.findByLabelText("Email"), member.user.email);
  await user.type(screen.getByLabelText("Password"), "synthetic-password");
  await user.click(screen.getByRole("button", { name: "Sign in" }));
  await screen.findByRole("heading", { name: "Member님, 무엇을 도와드릴까요?" });
  expect(pending.init.signal?.aborted).toBe(true);
  await act(async () => {
    late.resolve(response({ ...provider, name: "Admin A private draft" }, 201));
  });
  expect(screen.queryByText("Admin A private draft")).not.toBeInTheDocument();
  expect(
    screen.queryByDisplayValue("synthetic-late-credential"),
  ).not.toBeInTheDocument();
  expect(
    screen.queryByRole("button", { name: /연결 추가/ }),
  ).not.toBeInTheDocument();
});


it("requires an actual tool-capability response before enabling a verified text model",async()=>{
  const user=userEvent.setup();let confirmed=false;
  const test=server(call=>call.path===`/admin/providers/${provider.id}/tool-probe`
    ? response({...high,function_tools:confirmed}) : undefined,
    admin,{providers:[{...provider,choices:[high]}],default_selection:null});
  open("/admin/providers");
  await user.click(await screen.findByRole("button",{name:"도구 연결 확인"}));
  expect(await screen.findByText(/도구 지원을 확인하지 못했습니다/)).toBeInTheDocument();
  expect(screen.queryByText(/도구 사용 가능/)).not.toBeInTheDocument();
  confirmed=true;
  await user.click(screen.getByRole("button",{name:"도구 연결 확인"}));
  expect(await screen.findByText(/도구 사용 가능/)).toBeInTheDocument();
  expect(screen.queryByRole("button",{name:"도구 연결 확인"})).not.toBeInTheDocument();
  const calls=test.calls.filter(call=>call.path.endsWith("/tool-probe"));
  expect(calls).toHaveLength(2);
  expect(calls[0].body).toEqual({revision:1,model:high.model,effort:"high"});
});

describe("personal ChatGPT settings", () => {
  it("probes and selects the personal source, then keeps a disconnected preference unavailable", async () => {
    const user = userEvent.setup();
    const personal: ModelChoice = {
      ...high,
      source: "personal_chat_gpt",
      provider_id: "personal-a",
      provider_name: "나의 구독",
      effort: null,
    };
    let connection = {id: "personal-a", revision: 1, label: "나의 구독", status: "active", remote_revoke: "not_requested"};
    const settings: ModelSettings = {selection: null, effective: null, choices: [personal]};
    const fixture = server((call) => {
      if (call.path === "/settings/chatgpt") return response([connection]);
      if (call.path === "/settings/chatgpt/personal-a/models") return response({models: [{id: personal.model, name: "Plan model"}], truncated: false});
      if (call.path === "/settings/chatgpt/personal-a/probe") return response(personal);
      if (call.path === "/settings/model" && call.method === "PUT") {
        settings.selection = call.body!.selection as ModelSelection;
        settings.effective = personal;
        return response(settings);
      }
      if (call.path === "/settings/chatgpt/personal-a" && call.method === "DELETE") {
        connection = {...connection, status: "disconnected", remote_revoke: "unconfirmed"};
        settings.effective = null;
        settings.choices = [];
        return response({remote_revocation_confirmed: false});
      }
      return undefined;
    }, admin, {providers: [], default_selection: null}, settings);
    open("/settings/model");
    const group = await screen.findByRole("group", {name: "나의 구독"});
    await user.click(within(group).getByRole("button", {name: "내 모델 목록"}));
    await user.type(within(group).getByLabelText("정확한 모델"), personal.model);
    await user.click(within(group).getByRole("button", {name: "텍스트 연결 확인"}));
    await screen.findByText("텍스트 연결을 확인했습니다. 사용할 모델을 선택하세요.");
    expect(fixture.calls.find((call) => call.path.endsWith("personal-a/probe"))?.body).toEqual({revision: 1, model: personal.model, effort: null});
    const targetKey = JSON.stringify(["personal_chat_gpt", personal.provider_id, 1, personal.model, null]);
    await user.selectOptions(screen.getByRole("combobox", {name: "모델"}), targetKey);
    await user.click(screen.getByRole("button", {name: "선택 저장"}));
    await waitFor(() => expect(settings.selection?.source).toBe("personal_chat_gpt"));
    await user.click(within(group).getByRole("button", {name: "ChatGPT 연결 해제"}));
    await screen.findByText(/원격 해제는 확인되지 않았습니다/);
    await screen.findByRole("option", {name: "현재 선택 · 사용할 수 없음"});
    expect(settings.selection?.provider_id).toBe("personal-a");
    expect(settings.effective).toBeNull();
  });
});

it("guides local-helper server sign-in without requesting private launch material in the browser", async () => {
  const user = userEvent.setup();
  const {calls} = server((call) => call.path === "/settings/chatgpt" ? response([
    {id:"synthetic-returning-binding",revision:7,label:"My VM connection",status:"active",remote_revoke:"not_requested"},
  ]) : undefined, admin);
  open("/settings/model");
  await user.click(await screen.findByRole("button", {name:"ChatGPT 구독 연결"}));
  expect(screen.getByRole("region",{name:"ChatGPT 로그인 안내"})).toBeInTheDocument();
  expect(screen.getByText("새 ChatGPT 연결")).toBeInTheDocument();
  expect(screen.getByRole("button", {name: "명령 복사"})).toBeInTheDocument();
  await user.click(screen.getByRole("button",{name:"이 연결로 다시 로그인"}));
  expect(screen.getByText("My VM connection 다시 연결")).toBeInTheDocument();
  expect(screen.getByText(/--binding synthetic-returning-binding --revision 7/)).toBeInTheDocument();
  expect(calls.some(call=>/handoffs|local-login/.test(call.path))).toBe(false);
  const content=screen.getByRole("region",{name:"ChatGPT 로그인 안내"}).textContent ?? "";
  for (const secret of ["transfer_key","authorization_url","refresh_token","access_token","session="]) expect(content).not.toContain(secret);
});


describe("personal foreground tool verification", () => {
  it.each([
    ["personal_chat_gpt", "chatgpt"],
    ["personal_claude_code", "claude"],
    ["personal_compatible", "compatible-providers"],
  ] as const)("checks the effective %s tuple on demand and keeps the selected model", async (source, route) => {
    const user = userEvent.setup();
    const choice: ModelChoice = {...high, source, provider_id:"current-plan", function_tools:false};
    const settings: ModelSettings = {selection:{...selection(choice), source:choice.source}, effective:choice, choices:[choice]};
    const endpoint = `/settings/${route}/current-plan/tool-probe`;
    const {calls} = server(call => {
      if (call.path !== endpoint || call.method !== "POST") return undefined;
      settings.effective = {...choice, function_tools:true};
      settings.choices = [settings.effective];
      return response(settings.effective);
    }, admin, undefined, settings);
    open("/settings/model");
    const probe = await screen.findByRole("button", {name:"현재 모델 도구 확인"});
    expect(await screen.findByText("도구 확인 필요")).toBeInTheDocument();
    expect(calls.some(call => call.path === endpoint)).toBe(false);
    await user.click(probe);
    expect(await screen.findByText(/이 모델의 도구 호출을 확인했습니다/)).toBeInTheDocument();
    expect(calls.filter(call => call.path === endpoint).map(call => call.body)).toEqual([
      {revision:choice.provider_revision, model:choice.model, effort:choice.effort},
    ]);
    expect(calls.some(call => call.method === "PUT")).toBe(false);
    expect(screen.queryByRole("button", {name:"현재 모델 도구 확인"})).not.toBeInTheDocument();
  });

  it.each(["selection_changed", "proof_missing"])("does not announce tools on the current model when refreshed settings report %s", async changed => {
    const user = userEvent.setup();
    const choice: ModelChoice = {...high, source:"personal_claude_code", provider_id:"current-plan", function_tools:false};
    const other: ModelChoice = {...choice, model:"synthetic-other", effort:null};
    const settings: ModelSettings = {selection:{...selection(choice), source:choice.source}, effective:choice, choices:[choice,other]};
    server(call => {
      if (!call.path.endsWith("current-plan/tool-probe")) return undefined;
      if (changed === "selection_changed") {
        settings.selection = {...selection(other), source:other.source};
        settings.effective = other;
      }
      return response({...choice,function_tools:true});
    }, admin, undefined, settings);
    open("/settings/model");
    await user.click(await screen.findByRole("button", {name:"현재 모델 도구 확인"}));
    await screen.findByText(/모델 설정이 변경되었거나 도구 확인을 적용하지 못했습니다/);
    expect(screen.queryByText(/이 모델의 도구 호출을 확인했습니다/)).not.toBeInTheDocument();
    expect(screen.getByText("도구 확인 필요")).toBeInTheDocument();
    if (changed === "selection_changed") expect(screen.getByText(other.model, {selector:"strong"})).toBeInTheDocument();
  });

  it("prefers a tool-verified effort when starting a new model selection", async () => {
    const user = userEvent.setup();
    const noProof: ModelChoice = {...high, effort:null, function_tools:false};
    const proved: ModelChoice = {...high, function_tools:true};
    server(undefined, admin, undefined, {selection:null, effective:null, choices:[noProof, proved]});
    open("/settings/model");
    await user.selectOptions(await screen.findByLabelText("모델"), key(noProof));
    expect(screen.getByLabelText("추론 강도")).toHaveValue(key(proved));
    expect(screen.getByRole("option", {name:"모델 기본값 · 도구 확인 필요"})).toBeInTheDocument();
  });

  it("verifies only a saved exact personal tuple and reports capability on refreshed settings", async () => {
    const user=userEvent.setup();
    const choice: ModelChoice={...high,source:"personal_chat_gpt",provider_id:"personal-tools",provider_name:"개인 도구 연결",function_tools:false};
    const settings: ModelSettings={selection:null,effective:null,choices:[choice]};
    const fixture=server(call=>{
      if(call.path==="/settings/chatgpt")return response([{id:"personal-tools",revision:1,label:"개인 도구 연결",status:"active",remote_revoke:"not_requested"}]);
      if(call.path==="/settings/chatgpt/personal-tools/tool-probe") {settings.choices=[{...choice,function_tools:true}];return response(settings.choices[0]);}
      return undefined;
    },admin,{providers:[],default_selection:null},settings);
    open("/settings/model");
    const group=await screen.findByRole("group",{name:"개인 도구 연결"});
    await user.click(await within(group).findByRole("button",{name:`도구 연결 확인 · ${choice.model} · ${choice.effort}`}));
    await within(group).findByText(`${choice.model} · ${choice.effort} · 도구 사용 가능`);
    expect(fixture.calls.find(c=>c.path.endsWith("personal-tools/tool-probe"))?.body).toEqual({revision:1,model:choice.model,effort:choice.effort});
    expect(fixture.calls.some(c=>c.path.includes("/admin/providers/"))).toBe(false);
  });
  it("failed tool verification leaves exact text capability available without success", async () => {
    const user=userEvent.setup();
    const choice:ModelChoice={...high,source:"personal_chat_gpt",provider_id:"personal-tools",provider_name:"개인 도구 연결",effort:null,function_tools:false};
    server(call=>call.path==="/settings/chatgpt"?response([{id:"personal-tools",revision:1,label:"개인 도구 연결",status:"active",remote_revoke:"not_requested"}]):call.path.endsWith("personal-tools/tool-probe")?response({code:"model_tool_probe_incomplete"},502):undefined,admin,{providers:[],default_selection:null},{selection:null,effective:null,choices:[choice]});
    open("/settings/model");const group=await screen.findByRole("group",{name:"개인 도구 연결"});
    await user.click(await within(group).findByRole("button",{name:`도구 연결 확인 · ${choice.model} · 기본 강도`}));
    await screen.findByText(/개인 ChatGPT 요청을 완료하지 못했습니다/);
    expect(within(group).getByText(`${choice.model} · 기본 강도 · 텍스트 확인됨`)).toBeInTheDocument();
    expect(within(group).queryByText(/도구 사용 가능/)).not.toBeInTheDocument();
  });
});


it("explains personal tools, delegated reads and the remaining external-work limits", async () => {
  server(call => call.path === "/settings/chatgpt" ? response([]) : undefined,admin,{providers:[],default_selection:null},{selection:null,effective:null,choices:[]});
  open("/settings/model");
  const tools = await screen.findByText(/자료·메일·일정 조회, 계산, 기억과 기록 관리/);
  expect(tools).toHaveTextContent("맡긴 업무는 로그아웃한 뒤에도 실행됩니다.");
  expect(tools).toHaveTextContent("메일 발송은 최종 내용을 보고 매번 허락해야 합니다.");
  expect(screen.getByText(/현재 요청으로 기본 캘린더의 단일 시간 일정/)).toHaveTextContent("예약 업무의 메일·일정 변경 및 브라우저·볼트 사용은 아직 지원하지 않습니다.");
});

describe("personal image verification", () => {
  it.each([["personal_chat_gpt","chatgpt"],["personal_claude_code","claude"]] as const)("checks exact effective %s image tuple only on request", async (source,route) => {
    const user=userEvent.setup();
    const choice:ModelChoice={...high,source,provider_id:"vision-owner"};
    const settings:ModelSettings={selection:{...selection(choice),source},effective:choice,choices:[choice]};
    const endpoint=`/settings/${route}/vision-owner/vision-probe`;
    const {calls}=server(call=>call.path===endpoint?response({vision:true}):undefined,admin,undefined,settings);
    open("/settings/model");
    const button=await screen.findByRole("button",{name:"현재 모델 이미지 확인"});
    expect(calls.some(call=>call.path===endpoint)).toBe(false);
    await user.click(button);
    expect(await screen.findByText(/이 모델과 추론 강도의 이미지 연결을 확인했습니다/)).toBeInTheDocument();
    expect(calls.filter(call=>call.path===endpoint).map(call=>call.body)).toEqual([{revision:1,model:choice.model,effort:"high"}]);
    expect(calls.some(call=>call.method==="PUT")).toBe(false);
  });
  it("keeps a failed image proof unavailable and permits retry",async()=>{
    const choice:ModelChoice={...high,source:"personal_chat_gpt",provider_id:"vision-owner"};
    server(call=>call.path.endsWith("/vision-probe")?response({code:"vision_probe_failed"},502):undefined,admin,undefined,{selection:null,effective:choice,choices:[choice]});
    open("/settings/model");
    await userEvent.click(await screen.findByRole("button",{name:"현재 모델 이미지 확인"}));
    expect(await screen.findByRole("alert")).toHaveTextContent("이미지 픽셀을 정확히 읽는지 확인하지 못했습니다");
    expect(screen.getByRole("button",{name:"현재 모델 이미지 확인"})).toBeEnabled();
    expect(screen.queryByText(/이 모델과 추론 강도의 이미지 연결을 확인했습니다/)).not.toBeInTheDocument();
  });
});
