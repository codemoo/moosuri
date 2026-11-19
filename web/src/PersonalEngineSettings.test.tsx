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
  expect(await status.findByText("OpenAI 호환 API")).toBeInTheDocument();
  expect(status.getByText("synthetic-compatible · 모델 기본값")).toBeInTheDocument();
  expect(screen.getByRole("option", { name: "서버 기본값 사용" })).toHaveProperty("selected", true);
  expect(screen.getByRole("button", { name: "설치 설정 저장" })).toBeDisabled();
  expect(fetcher.mock.calls.map(([, request]) => request.method)).toEqual(["GET", "GET"]);
});
