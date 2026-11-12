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
      within(screen.getByLabelText("기본 모델과 추론 강도")).getAllByRole(
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
    const defaultSelect = screen.getByLabelText("기본 모델과 추론 강도");
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
    await user.selectOptions(defaultSelect, key(choice));
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
    expect(screen.getByLabelText("기본 모델과 추론 강도")).toHaveDisplayValue(
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

describe("personal model preferences", () => {
  it("blocks members from shared connection editing while providing their own settings", async () => {
    const user = userEvent.setup();
    const { calls } = server(undefined, member);
    open("/admin/providers");
    await screen.findByRole("heading", {
      name: "This page is for administrators.",
    });
    expect(
      screen.queryByRole("button", { name: /연결 추가/ }),
    ).not.toBeInTheDocument();
    expect(calls.some((call) => call.path.startsWith("/admin/providers"))).toBe(
      false,
    );
    await user.click(screen.getByRole("button", { name: "내 모델" }));
    await screen.findByRole("heading", { name: "내 모델", level: 2 });
    expect(screen.getByText(/내 계정에만 적용됩니다/)).toBeInTheDocument();
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
      member,
      undefined,
      { selection: null, effective: high, choices: [high, low] },
    );
    open("/settings/model");
    await user.selectOptions(
      await screen.findByLabelText("모델과 추론 강도"),
      key(low),
    );
    await user.click(screen.getByRole("button", { name: "선택 저장" }));
    await screen.findByText("내 모델을 저장했습니다.");
    expect(calls.find((call) => call.method === "PUT")?.body).toEqual({
      selection: selection(low),
    });
    expect(screen.getByText(`${provider.name} · low`)).toBeInTheDocument();
    await user.selectOptions(screen.getByLabelText("모델과 추론 강도"), "");
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
            member.user.id,
        ),
    ).toBe(true);
  });

  it("does not upgrade a saved revision or silently fall back when the selected connection changed", async () => {
    const newer = { ...high, provider_revision: 2 };
    server(undefined, member, undefined, {
      selection: selection(high),
      effective: null,
      choices: [newer],
    });
    open("/settings/model");
    expect(await screen.findByLabelText("모델과 추론 강도")).toHaveDisplayValue(
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
      member,
      undefined,
      { selection: selection(high), effective: high, choices: [high, low] },
    );
    open("/settings/model");
    await user.selectOptions(
      await screen.findByLabelText("모델과 추론 강도"),
      key(low),
    );
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
  await user.click(screen.getAllByRole("button", { name: "Sign out" })[0]);
  await user.type(await screen.findByLabelText("Email"), member.user.email);
  await user.type(screen.getByLabelText("Password"), "synthetic-password");
  await user.click(screen.getByRole("button", { name: "Sign in" }));
  await screen.findByRole("heading", { name: "Hello, Member." });
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
