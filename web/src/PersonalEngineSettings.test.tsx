import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import { PersonalEngineSettings } from "./PersonalEngineSettings";

afterEach(() => vi.unstubAllGlobals());

it("keeps API credential recovery available after the engine has been configured", async () => {
  const model = { model: "claude-synthetic", effort: "medium" };
  const revision = "00000000-0000-4000-8000-000000000001";
  const fetcher = vi.fn(async (url: string) => Response.json(url.endsWith("/setup")
    ? { supported: true, saved: true, connection_revision: revision }
    : { configured: true, enabled: true, revision: 1, connection_revision: revision,
        kind: "anthropic", connection_mode: "api", choices: [model], host_default: model,
        default_choice: null, default_stale: false }));
  vi.stubGlobal("fetch", fetcher);
  render(<PersonalEngineSettings account="actor" expired={vi.fn()} unavailable={vi.fn()} />);
  await userEvent.setup().click(await screen.findByRole("button", { name: "키·모델 교체" }));
  expect(screen.getByLabelText("Anthropic API 키")).toHaveValue("");
  expect(screen.getByRole("button", { name: "연결 확인 후 저장" })).toBeInTheDocument();
  // Opening recovery does not send a provider request or replace the connection.
  expect(fetcher.mock.calls).toHaveLength(2);
});

it("offers server setup guidance until an engine is configured, then shows the saved server default", async () => {
  const defaultModel = { model: "synthetic-compatible", effort: null };
  const unconfigured = {
    configured: false, enabled: false, revision: 0, connection_revision: null, kind: null,
    host_default: null, choices: [], default_choice: null, default_stale: false,
  };
  const configured = {
    ...unconfigured, configured: true, enabled: true, revision: 1, connection_revision: "synthetic-revision",
    host_default: defaultModel, choices: [defaultModel], kind: "compatible",
  };
  const fetcher = vi.fn(async (_url: string, _request: RequestInit) => Response.json(configured))
    .mockImplementationOnce(async () => Response.json(unconfigured));
  vi.stubGlobal("fetch", fetcher);
  render(<PersonalEngineSettings account="actor" expired={vi.fn()} unavailable={vi.fn()} />);
  expect(await screen.findByRole("link", { name: "설치 서버에서 엔진 연결 설정하기" })).toBeInTheDocument();
  expect(screen.queryByRole("form", { name: "설치 모델 설정" })).not.toBeInTheDocument();

  await userEvent.setup().click(screen.getByRole("button", { name: "새로고침" }));
  const status = within(await screen.findByRole("region", { name: "엔진 상태" }));
  expect(await status.findByText(/OpenAI 호환 API · 서버 기본값/)).toBeInTheDocument();
  expect(status.getByText("synthetic-compatible · 모델 기본값")).toBeInTheDocument();
  expect(screen.getByRole("option", { name: "서버 기본값 사용" })).toHaveProperty("selected", true);
  expect(screen.getByRole("button", { name: "설치 설정 저장" })).toBeDisabled();
  expect(fetcher.mock.calls.filter(([url]) => !url.endsWith("/setup")).map(([, request]) => request.method)).toEqual(["GET", "GET"]);
});

it("previews the chosen model without claiming it is active and reloads after a revision conflict", async () => {
  const old = { model: "synthetic-old", effort: null };
  const next = { model: "synthetic-new", effort: "medium" };
  const engine = {
    configured: true, enabled: true, revision: 4, connection_revision: "current",
    host_default: old, choices: [old, next], kind: "compatible", default_choice: null, default_stale: false,
  };
  let reads = 0;
  const writes: unknown[] = [];
  vi.stubGlobal("fetch", vi.fn(async (_url: string, request: RequestInit) => {
    if (request.method === "POST") {
      const body = JSON.parse(String(request.body));
      writes.push(body);
      if (writes.length === 1) return new Response(null, { status: 409 });
      return Response.json({ ...engine, revision: 6, default_choice: body.default_choice, enabled: body.enabled });
    }
    reads++;
    return Response.json(reads === 1 ? engine : { ...engine, revision: 5, connection_revision: "new-connection" });
  }));
  render(<PersonalEngineSettings account="actor" expired={vi.fn()} unavailable={vi.fn()} />);
  const user = userEvent.setup();
  const model = await screen.findByRole("combobox", { name: "모델" });
  await user.selectOptions(model, next.model);
  expect(screen.getByText(/조합을 모든 회원의 새 요청에 적용합니다/)).toHaveTextContent("synthetic-new · 보통");
  expect(within(screen.getByRole("region", { name: "엔진 상태" })).getByText(`${old.model} · 모델 기본값`)).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "설치 설정 저장" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("최신 설정");
  const reloaded = await screen.findByRole("combobox", { name: "모델" });
  expect(reloaded).toHaveValue("");
  await user.click(screen.getByRole("checkbox", { name: "새 모델 요청 허용" }));
  expect(screen.getByText(/모든 회원의 새 모델 요청이 일시 중지됩니다/)).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "설치 설정 저장" }));
  expect(await within(screen.getByRole("region", { name: "엔진 상태" })).findByText("일시 중지")).toBeInTheDocument();
  expect(writes).toEqual([
    { revision: 4, connection_revision: "current", enabled: true, default_choice: next },
    { revision: 5, connection_revision: "new-connection", enabled: false, default_choice: null },
  ]);
});

it.each([
  ["anthropic", "claude_code", "Claude 구독 사용량 열기", "https://claude.ai/settings/usage"],
  ["open_ai", "chatgpt_plan", "ChatGPT 사용량 열기", "https://chatgpt.com/codex/settings/usage"],
  ["open_ai", "api", "OpenAI API 사용량 열기", "https://platform.openai.com/usage"],
  ["open_router", "api", "OpenRouter 사용량 열기", "https://openrouter.ai/activity"],
  ["compatible", "api", null, null],
])("directs %s / %s admins to the matching usage source and labels unreported limits", async (kind, connection_mode, name, url) => {
  const model = { model: "synthetic", effort: null };
  const get = vi.fn(async () => Response.json({
    configured: true, enabled: true, revision: 1, connection_revision: "fixture", kind, connection_mode,
    choices: [model], host_default: model, default_choice: null, default_stale: false,
  }));
  vi.stubGlobal("fetch", get);
  render(<PersonalEngineSettings account="actor" expired={vi.fn()} unavailable={vi.fn()} />);
  const section = within(await screen.findByRole("region", { name: "공급자 잔여 한도" }));
  expect(section.getByText(/잔여량 확인 불가 · 초기화 시각 확인 불가/)).toBeInTheDocument();
  if (name) expect(section.getByRole("link", { name })).toHaveAttribute("href", url);
  else expect(section.getByText(/운영자에게 한도를 확인/)).toBeInTheDocument();
  await userEvent.setup().click(section.getByText("무수리 회원별 사용 집계"));
  expect(section.getByRole("link", { name: "서버에서 회원별 집계 확인하기" })).toHaveAttribute("href", expect.stringContaining("admin.md#회원별-사용-집계"));
  expect(get).toHaveBeenCalledOnce();
});

it("requests current OpenRouter capacity explicitly and distinguishes zero, missing and scheduled resets", async () => {
  const user = userEvent.setup();
  const model = {model: "synthetic", effort: null};
  const metrics = [
    {id: "cap", label: "Key cap", remaining: 0, limit: 3, used: null, unit: "usd", reset_interval: "weekly", reset_at: null},
    {id: "requests", label: "Requests", remaining: null, limit: null, used: 2, unit: "requests", reset_interval: null, reset_at: 1796083200},
  ];
  const probes: Array<Record<string, unknown>> = [];
  let failed = false;
  vi.stubGlobal("fetch", vi.fn(async (url: string, init?: RequestInit) => {
    if (url.endsWith("/engine")) return Response.json({configured:true,enabled:true,revision:1,connection_revision:"current",
      connection_mode:"api",kind:"open_router",host_default:model,default_choice:null,choices:[model],default_stale:false});
    if (!url.endsWith("/engine/quota")) throw new Error("unexpected path");
    const data = JSON.parse(String(init?.body));
    probes.push(data);
    if (failed) return Response.json({probe_id:"second",report:{provider_id:"current",provider_revision:1,status:"unavailable",metrics:[],observed_at:null}});
    return Response.json({probe_id:"first",report:data.probe_id ? {provider_id:"current",provider_revision:1,status:"known",metrics,observed_at:1796060000} : null});
  }));
  render(<PersonalEngineSettings account="actor" expired={vi.fn()} unavailable={vi.fn()} />);
  const area = within(await screen.findByRole("region", {name:"공급자 잔여 한도"}));
  expect(probes).toHaveLength(0);
  await user.click(area.getByRole("button", {name:"남은 사용량 확인"}));
  const cap = (await area.findByText("Key cap")).parentElement;
  expect(cap).toHaveTextContent(/남은 양.*0\.00/);
  expect(cap).toHaveTextContent("매주 (시각 미제공)");
  expect(area.getByText("Requests").parentElement).toHaveTextContent(/사용 2회 · 잔여량 미제공 · 리셋/);
  expect(area.getByText(/기준$/)).toBeInTheDocument();
  expect(probes).toEqual([{revision:1,connection_revision:"current",probe_id:null},{revision:1,connection_revision:"current",probe_id:"first"}]);
  failed = true;
  await user.click(area.getByRole("button", {name:"사용량 다시 확인"}));
  expect(await area.findByText("제공사에서 한도 정보를 받지 못했습니다.")).toBeInTheDocument();
  expect(area.queryByText("Key cap")).not.toBeInTheDocument();
  expect(area.queryByText(/기준$/)).not.toBeInTheDocument();
});
