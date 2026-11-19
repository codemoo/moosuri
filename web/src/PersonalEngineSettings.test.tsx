import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import { PersonalEngineSettings } from "./PersonalEngineSettings";

afterEach(() => vi.unstubAllGlobals());

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
  expect(fetcher.mock.calls.map(([, request]) => request.method)).toEqual(["GET", "GET"]);
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
