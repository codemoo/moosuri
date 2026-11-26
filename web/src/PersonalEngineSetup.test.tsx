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
