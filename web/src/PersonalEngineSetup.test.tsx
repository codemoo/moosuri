import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import { PersonalEngineSetup } from "./PersonalEngineSetup";

afterEach(() => vi.unstubAllGlobals());
const empty = { supported: true, saved: false };
const saved = { supported: true, saved: true };
async function fill() {
  const user = userEvent.setup();
  await user.type(await screen.findByLabelText("Anthropic API 키"), "synthetic-test-key");
  await user.click(screen.getByRole("button", { name: "연결 확인 후 저장" }));
}
it("sends the chosen model in an account-bound request and clears the credential", async () => {
  const writes: Record<string, unknown>[] = [];
  const onSaved = vi.fn();
  vi.stubGlobal("fetch", vi.fn(async (_url: string, init: RequestInit) => {
    expect((init.headers as Record<string, string>)["X-Moosuri-Account"]).toBe("synthetic-admin");
    if (init.method === "POST") { const body = JSON.parse(String(init.body)); writes.push(body); return Response.json({ ...saved, connection_revision: body.request_id }); }
    return Response.json(empty);
  }));
  render(<PersonalEngineSetup account="synthetic-admin" onSaved={onSaved} expired={vi.fn()} unavailable={vi.fn()} />);
  await fill();
  expect(await screen.findByRole("status")).toHaveTextContent("연결 설정이 등록");
  expect(writes).toHaveLength(1);
  expect(writes[0]).toMatchObject({ model: "claude-sonnet-5-5", effort: "medium", api_key: "synthetic-test-key" });
  expect(screen.queryByLabelText("Anthropic API 키")).not.toBeInTheDocument();
  expect(JSON.stringify(localStorage)).not.toContain("synthetic-test-key");
  expect(JSON.stringify(sessionStorage)).not.toContain("synthetic-test-key");
  expect(onSaved).toHaveBeenCalledOnce();
});
it("reconciles a lost save response without a second submission", async () => {
  let submitted = 0; let revision = "";
  vi.stubGlobal("fetch", vi.fn(async (_url: string, init: RequestInit) => {
    if (init.method === "POST") { submitted++; revision = JSON.parse(String(init.body)).request_id; throw new TypeError("synthetic response lost"); }
    return Response.json(submitted ? { ...saved, connection_revision: revision } : empty);
  }));
  render(<PersonalEngineSetup account="synthetic-admin" onSaved={vi.fn()} expired={vi.fn()} unavailable={vi.fn()} />);
  await fill();
  expect(await screen.findByRole("status")).toHaveTextContent("연결 설정이 등록");
  expect(submitted).toBe(1);
  expect(screen.queryByRole("alert")).not.toBeInTheDocument();
});
it("keeps a provider rejection unsaved and requires fresh secret entry", async () => {
  vi.stubGlobal("fetch", vi.fn(async (_url: string, init: RequestInit) => init.method === "POST"
    ? Response.json({ code: "model_not_in_provider_catalog" }, { status: 422 }) : Response.json(empty)));
  const onSaved = vi.fn();
  render(<PersonalEngineSetup account="synthetic-admin" onSaved={onSaved} expired={vi.fn()} unavailable={vi.fn()} />);
  await fill();
  expect(await screen.findByRole("alert")).toHaveTextContent("모델을 찾지 못했습니다");
  expect(screen.getByLabelText("Anthropic API 키")).toHaveValue("");
  expect(onSaved).not.toHaveBeenCalled();
});
const current = "11111111-1111-4111-8111-111111111111";
it("replaces a saved connection only by naming its current revision", async () => {
  const writes: Record<string, unknown>[] = [];
  let state: Record<string, unknown> = { supported: true, saved: true, connection_revision: current };
  vi.stubGlobal("fetch", vi.fn(async (_url: string, init: RequestInit) => {
    if (init.method === "POST") {
      const body = JSON.parse(String(init.body)); writes.push(body);
      state = { supported: true, saved: true, connection_revision: body.request_id };
      return Response.json(state);
    }
    return Response.json(state);
  }));
  const onSaved = vi.fn();
  render(<PersonalEngineSetup account="synthetic-admin" onSaved={onSaved} expired={vi.fn()} unavailable={vi.fn()} />);
  expect(screen.queryByLabelText("Anthropic API 키")).not.toBeInTheDocument();
  await userEvent.setup().click(await screen.findByRole("button", { name: "키·모델 교체" }));
  await fill();
  await waitFor(() => expect(onSaved).toHaveBeenCalledOnce());
  expect(writes).toHaveLength(1);
  expect(writes[0]).toMatchObject({ replace_revision: current, api_key: "synthetic-test-key" });
  expect(writes[0].request_id).not.toBe(current);
  expect(screen.queryByLabelText("Anthropic API 키")).not.toBeInTheDocument();
  expect(JSON.stringify(localStorage)).not.toContain("synthetic-test-key");
});
it("offers no replacement without a known revision", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => Response.json(saved)));
  render(<PersonalEngineSetup account="synthetic-admin" onSaved={vi.fn()} expired={vi.fn()} unavailable={vi.fn()} />);
  expect(await screen.findByRole("status")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "키·모델 교체" })).not.toBeInTheDocument();
});
it("reports a stale replacement without saving or re-submitting", async () => {
  let posts = 0;
  vi.stubGlobal("fetch", vi.fn(async (_url: string, init: RequestInit) => {
    if (init.method === "POST") { posts++; return Response.json({}, { status: 409 }); }
    return Response.json({ supported: true, saved: true, connection_revision: current });
  }));
  const onSaved = vi.fn();
  render(<PersonalEngineSetup account="synthetic-admin" onSaved={onSaved} expired={vi.fn()} unavailable={vi.fn()} />);
  await userEvent.setup().click(await screen.findByRole("button", { name: "키·모델 교체" }));
  await fill();
  expect(await screen.findByRole("alert")).toHaveTextContent("먼저 변경");
  expect(posts).toBe(1);
  expect(onSaved).not.toHaveBeenCalled();
});
it("offers an explicit disconnect for a connected subscription, then returns to fresh choices without replace", async () => {
  let removed = false;
  const subscriptionPosts: Record<string, unknown>[] = [];
  vi.stubGlobal("fetch", vi.fn(async (url: string, init: RequestInit) => {
    if (String(url).endsWith("/personal/admin/engine/subscription")) {
      if (init.method === "GET") return Response.json({ supported: true, operation: null });
      const body = JSON.parse(String(init.body)); subscriptionPosts.push(body);
      const state = body.op === "disconnect" ? (removed = true, "disconnected") : "preparing";
      return Response.json({ supported: true, operation: { request_id: body.request_id, state, kind: body.op === "disconnect" ? "disconnect" : undefined, model: "claude-sonnet-5-5", effort: "medium" } });
    }
    return Response.json(removed ? empty : { supported: true, saved: true, connection_revision: current });
  }));
  const onSaved = vi.fn();
  render(<PersonalEngineSetup account="synthetic-admin" onSaved={onSaved} expired={vi.fn()} unavailable={vi.fn()} subscriptionRevision={current} />);
  const user = userEvent.setup();
  // Connected: one clear disconnect, no key form and no implicit replacement.
  await user.click(await screen.findByRole("button", { name: "구독 연결 해제" }));
  await waitFor(() => expect(subscriptionPosts).toHaveLength(1));
  expect(subscriptionPosts[0]).toMatchObject({ op: "disconnect", replace_revision: current });
  expect(subscriptionPosts[0].request_id).not.toBe(current);
  await waitFor(() => expect(onSaved).toHaveBeenCalledOnce());
  // Back to the choices: API by default, own subscription is a fresh start (no replace).
  expect(await screen.findByLabelText("API 키 사용")).toBeChecked();
  expect(screen.getByRole("status")).toHaveTextContent("연결이 해제되었습니다");
  expect(screen.queryByRole("button", { name: "구독 연결 해제" })).not.toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "키·모델 교체" })).not.toBeInTheDocument();
  await user.click(screen.getByLabelText("내 Claude 구독 사용"));
  await user.type(await screen.findByLabelText("Claude 계정 이메일"), "owner@example.test");
  await user.click(screen.getByLabelText(/호스팅 조건을 확인했고/));
  await user.click(screen.getByRole("button", { name: "구독 로그인 시작" }));
  await waitFor(() => expect(subscriptionPosts.some(p => p.op === "start")).toBe(true));
  expect(subscriptionPosts.find(p => p.op === "start")).not.toHaveProperty("replace_revision");
});
it("shows no disconnect when the connected subscription revision cannot be confirmed", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => Response.json({ supported: true, saved: true, connection_revision: current })));
  render(<PersonalEngineSetup account="synthetic-admin" onSaved={vi.fn()} expired={vi.fn()} unavailable={vi.fn()} subscriptionRevision="22222222-2222-4222-8222-222222222222" />);
  expect(await screen.findByRole("alert")).toHaveTextContent("현재 연결 상태를 확인하지 못했습니다");
  expect(screen.queryByRole("button", { name: "구독 연결 해제" })).not.toBeInTheDocument();
});
it("returns to API and subscription choices after a reloaded disconnect finishes", async () => {
  let removed = false;
  vi.stubGlobal("fetch", vi.fn(async (url: string, init: RequestInit) => {
    if (String(url).endsWith("/personal/admin/engine/subscription")) {
      if (init.method === "POST") removed = true;
      return Response.json({ operation: { request_id: current, kind: "disconnect", state: removed ? "disconnected" : "cleanup_pending" } });
    }
    return Response.json({ ...empty, subscription_operation: !removed });
  }));
  const onSaved = vi.fn();
  render(<PersonalEngineSetup account="synthetic-admin" onSaved={onSaved} expired={vi.fn()} unavailable={vi.fn()} />);
  await userEvent.setup().click(await screen.findByRole("button", { name: "정리 다시 시도" }));
  await waitFor(() => expect(onSaved).toHaveBeenCalledOnce());
  expect(await screen.findByLabelText("API 키 사용")).toBeChecked();
  expect(screen.getByLabelText("Anthropic API 키")).toBeVisible();
});
it("keeps recovery reachable when corrupt config conflicts with a stale engine revision", async () => {
  vi.stubGlobal("fetch", vi.fn(async (url: string) => Response.json(
    String(url).endsWith("/personal/admin/engine/subscription")
      ? { operation: { request_id: current, kind: "disconnect", state: "cleanup_pending" } }
      : { supported: true, saved: true, connection_revision: null, subscription_operation: true }
  )));
  render(<PersonalEngineSetup account="synthetic-admin" subscriptionRevision={current} onSaved={vi.fn()} expired={vi.fn()} unavailable={vi.fn()} />);
  expect(await screen.findByRole("button", { name: "정리 다시 시도" })).toBeVisible();
  expect(screen.queryByRole("button", { name: "구독 로그인 시작" })).not.toBeInTheDocument();
  expect(screen.queryByLabelText("Anthropic API 키")).not.toBeInTheDocument();
});
it("stops immediately when the administrator session loses authority", async () => {
  const unavailable = vi.fn(); let calls = 0;
  vi.stubGlobal("fetch", vi.fn(async (_url: string, init: RequestInit) => { calls++; return init.method === "POST" ? new Response(null, { status: 403 }) : Response.json(empty); }));
  render(<PersonalEngineSetup account="synthetic-admin" onSaved={vi.fn()} expired={vi.fn()} unavailable={unavailable} />);
  await fill();
  await waitFor(() => expect(unavailable).toHaveBeenCalledOnce());
  expect(calls).toBe(2);
  expect(screen.getByLabelText("Anthropic API 키")).toHaveValue("");
});

it("does not attribute another admin's connection to a lost first-save response", async () => {
  let submitted = 0;
  const onSaved = vi.fn();
  vi.stubGlobal("fetch", vi.fn(async (_url: string, init: RequestInit) => {
    if (init.method === "POST") { submitted++; throw new TypeError("synthetic response lost"); }
    return Response.json(submitted ? { ...saved, connection_revision: current } : empty);
  }));
  render(<PersonalEngineSetup account="synthetic-admin" onSaved={onSaved} expired={vi.fn()} unavailable={vi.fn()} />);
  await fill();
  expect(await screen.findByRole("alert")).toHaveTextContent("연결을 확인하지 못했습니다");
  expect(screen.getByRole("status")).toHaveTextContent("연결 설정이 등록되어 있습니다");
  expect(onSaved).not.toHaveBeenCalled();
  expect(submitted).toBe(1);
});
