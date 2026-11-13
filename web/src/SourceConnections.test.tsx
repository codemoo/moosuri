import { StrictMode } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SourceConnections } from "./SourceConnections";
import { SourceProvenance } from "./SourceProvenance";
import type { SourceConnection, SourceSyncRun } from "../shared/api-types";

const fixture = (extra: Partial<SourceConnection> = {}): SourceConnection => ({
  id: "connection-a",
  adapter_version: "1",
  collection_id: "collection-a",
  kind: "custom_http",
  name: "내 수집기",
  scope: "notes",
  remote_account: "external",
  mail_connection_id: null,
  config_revision: 3,
  credential_epoch: 2,
  state: "active",
  sync_enabled: true,
  poll_interval_seconds: 300,
  local_sequence: 5,
  last_heartbeat_at: 120,
  last_commit_at: 110,
  last_completed_at: 100,
  next_due_at: 0,
  error: null,
  credential_present: true,
  ...extra,
});
const run = (extra: Partial<SourceSyncRun> = {}): SourceSyncRun => ({
  id: "run-a",
  generation: 1,
  state: "completed",
  started_at: 100,
  heartbeat_at: 110,
  finished_at: 120,
  error: null,
  producer_report: true,
  ...extra,
});
const response = (value: unknown, status = 200) =>
  new Response(status === 204 ? null : JSON.stringify(value), {
    status,
    headers: { "Content-Type": "application/json" },
  });
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((yes) => {
    resolve = yes;
  });
  return { promise, resolve };
}
const recentStart = new Date(
  Date.now() - 86400000 - new Date().getTimezoneOffset() * 60000,
)
  .toISOString()
  .slice(0, 16);
type Call = {
  path: string;
  init?: RequestInit;
  body: Record<string, unknown> | undefined;
};
function server(initial = [fixture()], runs = [run()]) {
  let connections = initial;
  const calls: Call[] = [];
  const handler = vi.fn(
    async (url: string | URL | Request, init?: RequestInit) => {
      const path = String(url);
      const body = init?.body ? JSON.parse(String(init.body)) : undefined;
      calls.push({ path, init, body });
      if (
        path === "/api/v1/source-connections" &&
        (!init?.method || init.method === "GET")
      )
        return response({ connections, gmail_configured: true });
      if (path === "/api/v1/mail/connections")
        return response({
          configured: true,
          connections: [
            {
              id: "mail-a",
              email: "synthetic@example.test",
              epoch: 1,
              status: "active",
            },
          ],
        });
      if (path.endsWith("/runs")) return response({ runs });
      if (path === "/api/v1/source-connections" && init?.method === "POST") {
        const value = fixture({
          id: "added",
          kind: body.kind,
          name: body.name,
          scope: body.scope ?? body.channel_id ?? "labels",
        });
        connections = [...connections, value];
        return response(value);
      }
      if (path.endsWith("/pause")) {
        const value = {
          ...connections[0],
          state: body.paused ? "paused" : "active",
          config_revision: connections[0].config_revision + 1,
        };
        connections = [value];
        return response(value);
      }
      if (path.endsWith("/token"))
        return response({
          token: "synthetic-once-secret",
          expires_at: Math.floor(Date.now() / 1000) + 3600,
        });
      if (path.endsWith("/revoke-token")) {
        connections = connections.map((c) => ({
          ...c,
          credential_present: false,
        }));
        return response(null, 204);
      }
      if (path.endsWith("/disconnect")) {
        const value = {
          ...connections[0],
          state: body.mode === "delete" ? "deleted" : "disconnected",
          config_revision: connections[0].config_revision + 1,
        };
        connections = [value];
        return response(value);
      }
      if (
        path.endsWith("/gmail-connection") ||
        path.endsWith("/reset-checkpoint") ||
        path.endsWith("/slack-token") ||
        path.endsWith("/sync") ||
        init?.method === "PATCH"
      )
        return response(connections[0]);
      throw new Error(`Unexpected test route: ${path}`);
    },
  );
  vi.stubGlobal("fetch", handler);
  return { handler, calls };
}
const props = { accountId: "account-a", onExpired: vi.fn(), back: vi.fn() };
async function openDetails(name = "내 수집기") {
  await userEvent.click(
    await screen.findByRole("button", { name: new RegExp(name) }),
  );
  return screen.findByRole("region", { name: "수집 연결 상세" });
}
beforeEach(() => {
  window.history.replaceState(null, "", "/settings/sources");
  localStorage.clear();
  sessionStorage.clear();
  vi.useRealTimers();
});

describe("source connections", () => {
  it("reconnects Gmail with an existing personal connection and unchanged scope", async () => {
    const { calls } = server([
      fixture({
        kind: "gmail",
        state: "needs_reconnect",
        credential_present: false,
        mail_connection_id: "old-mail",
      }),
    ]);
    render(<SourceConnections {...props} />);
    await openDetails();
    await screen.findByRole("option", { name: "synthetic@example.test" });
    await userEvent.click(screen.getByRole("button", { name: "Gmail 재연결" }));
    expect(
      calls.find((call) => call.path.endsWith("/gmail-connection"))?.body,
    ).toEqual({ expected_revision: 3, mail_connection_id: "mail-a" });
    expect(
      screen.getByText(/같은 Google 계정의 새 연결만 사용할 수 있습니다/),
    ).toBeInTheDocument();
  });
  it("offers an explicit checkpoint restart while keeping existing source data", async () => {
    const { calls } = server([
      fixture({ kind: "gmail", error: "checkpoint_invalid" }),
    ]);
    render(<SourceConnections {...props} />);
    await openDetails();
    expect(
      screen.getByText(/기존 자료를 유지하고 허용한 범위를 다시 확인/),
    ).toBeInTheDocument();
    await userEvent.click(
      screen.getByRole("button", { name: "동기화 다시 시작" }),
    );
    expect(
      calls.find((call) => call.path.endsWith("/reset-checkpoint"))?.body,
    ).toEqual({ expected_revision: 3 });
    expect(calls.some((call) => call.path.endsWith("/disconnect"))).toBe(false);
  });
  it("explains invalid batches without suggesting silent skipping", async () => {
    server([fixture({ kind: "slack", error: "invalid_batch" })]);
    render(<SourceConnections {...props} />);
    await openDetails();
    expect(
      screen.getByText(
        /더 좁은 범위로 새 연결을 만드세요. 자료를 자동으로 건너뛰지 않습니다/,
      ),
    ).toBeInTheDocument();
  });
  it("lets a retained disconnected collection be deleted later", async () => {
    const { calls } = server([fixture({ state: "disconnected" })]);
    render(<SourceConnections {...props} />);
    await openDetails();
    await userEvent.click(
      screen.getByRole("button", { name: "보관된 자료 삭제" }),
    );
    expect(
      screen.getByRole("radio", { name: "자료와 기억 보관" }),
    ).toBeDisabled();
    await userEvent.click(
      screen.getByRole("button", { name: "자료 삭제 후 해제" }),
    );
    expect(
      calls.find((call) => call.path.endsWith("/disconnect"))?.body,
    ).toEqual({ expected_revision: 3, mode: "delete" });
  });
  it("creates a generic custom connection with only permitted identity fields", async () => {
    const { calls } = server([]);
    render(<SourceConnections {...props} />);
    await userEvent.click(
      screen.getByRole("button", { name: "커스텀 수집 연결 추가" }),
    );
    await userEvent.type(screen.getByLabelText("연결 이름"), "개인 카톡 자료");
    await userEvent.type(
      screen.getByLabelText("고정 수집 범위"),
      "my-personal-scope",
    );
    await userEvent.click(screen.getByRole("button", { name: "연결 만들기" }));
    await screen.findByRole("region", { name: "수집 연결 상세" });
    expect(
      calls.find(
        (c) =>
          c.path === "/api/v1/source-connections" && c.init?.method === "POST",
      )?.body,
    ).toEqual({
      kind: "custom_http",
      name: "개인 카톡 자료",
      scope: "my-personal-scope",
    });
    expect(
      screen.queryByRole("button", { name: /카톡 추가/ }),
    ).not.toBeInTheDocument();
  });
  it("requires explicit Gmail scope consent and sends existing account with bounded starting time", async () => {
    const { calls } = server([]);
    render(<SourceConnections {...props} />);
    await userEvent.click(screen.getByRole("button", { name: "Gmail 추가" }));
    await screen.findByRole("option", { name: "synthetic@example.test" });
    await userEvent.type(screen.getByLabelText("연결 이름"), "연구 메일");
    await userEvent.type(
      screen.getByLabelText("Gmail 라벨 ID (쉼표로 구분)"),
      "INBOX, Label_2",
    );
    fireEvent.change(screen.getByLabelText("수집 시작 시각"), {
      target: { value: recentStart },
    });
    await userEvent.click(screen.getByRole("button", { name: "연결 만들기" }));
    expect(calls.some((c) => c.init?.method === "POST")).toBe(false);
    await userEvent.click(
      screen.getByRole("checkbox", { name: /동기화하는 데 동의/ }),
    );
    await userEvent.click(screen.getByRole("button", { name: "연결 만들기" }));
    await screen.findByRole("region", { name: "수집 연결 상세" });
    expect(calls.find((c) => c.init?.method === "POST")?.body).toEqual({
      kind: "gmail",
      name: "연구 메일",
      mail_connection_id: "mail-a",
      label_ids: ["INBOX", "Label_2"],
      after_unix: Math.floor(new Date(recentStart).getTime() / 1000),
      poll_interval_seconds: 300,
    });
  });
  it("redacts Slack credentials and clears them immediately on submission, including failure", async () => {
    const { handler, calls } = server([]);
    render(<SourceConnections {...props} />);
    await userEvent.click(screen.getByRole("button", { name: "Slack 추가" }));
    await userEvent.type(screen.getByLabelText("연결 이름"), "팀 자료");
    await userEvent.type(screen.getByLabelText("Slack 채널 ID"), "C_SYNTHETIC");
    fireEvent.change(screen.getByLabelText("수집 시작 시각"), {
      target: { value: recentStart },
    });
    const credential = screen.getByLabelText("Slack 수집 토큰");
    expect(credential).toHaveAttribute("type", "password");
    await userEvent.type(credential, "synthetic-secret");
    await userEvent.click(
      screen.getByRole("checkbox", { name: /동기화하는 데 동의/ }),
    );
    const pending = deferred<Response>();
    handler.mockImplementationOnce(async (url, init) => {
      calls.push({
        path: String(url),
        init,
        body: JSON.parse(String(init?.body)),
      });
      return pending.promise;
    });
    await userEvent.click(screen.getByRole("button", { name: "연결 만들기" }));
    expect(credential).toHaveValue("");
    expect(calls.at(-1)?.body).toMatchObject({
      kind: "slack",
      channel_id: "C_SYNTHETIC",
      token: "synthetic-secret",
      reconcile_lookback_seconds: 86400,
    });
    expect(calls.at(-1)?.body).not.toHaveProperty("remote_account");
    pending.resolve(response({ code: "invalid_credentials" }, 400));
    await screen.findByRole("alert");
    expect(credential).toHaveValue("");
    expect(localStorage.length).toBe(0);
    expect(sessionStorage.length).toBe(0);
  });
  it("keeps local commit, heartbeat, and producer completion report distinct", async () => {
    server();
    render(<SourceConnections {...props} />);
    const detail = await openDetails();
    expect(within(detail).getByText("마지막 접속")).toBeInTheDocument();
    expect(within(detail).getByText("마지막 자료 저장")).toBeInTheDocument();
    expect(
      within(detail).getByText("마지막 완료 보고 (수집기)"),
    ).toBeInTheDocument();
    await screen.findByText("수집기 보고: 범위 동기화 완료");
    expect(
      screen.getByText("전체 범위를 무수리가 검증한 결과가 아닙니다."),
    ).toBeInTheDocument();
    expect(
      screen.queryByText("마지막 범위 동기화 완료"),
    ).not.toBeInTheDocument();
  });
  it("uses revision fencing for pause/resume and disables sync while paused or reconnect required", async () => {
    const { calls } = server(
      [fixture({ kind: "slack", state: "paused" })],
      [
        run({ state: "partial", producer_report: false }),
        run({ id: "run-b", state: "unknown", producer_report: false }),
      ],
    );
    render(<SourceConnections {...props} />);
    await openDetails();
    expect(screen.getByRole("button", { name: "지금 동기화" })).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "수집 재개" }));
    await screen.findByRole("button", { name: "수집 일시정지" });
    expect(calls.find((c) => c.path.endsWith("/pause"))?.body).toEqual({
      expected_revision: 3,
      paused: false,
    });
    expect(screen.getByRole("button", { name: "지금 동기화" })).toBeEnabled();
    await userEvent.click(
      screen.getByRole("button", { name: "수집 일시정지" }),
    );
    await screen.findByRole("button", { name: "수집 재개" });
    expect(calls.filter((c) => c.path.endsWith("/pause")).at(-1)?.body).toEqual(
      { expected_revision: 4, paused: true },
    );
    await screen.findByText("일부만 확인됨");
    expect(screen.getByText("결과 확인 필요")).toBeInTheDocument();
  });
  it("offers Slack reconnection without treating missing credentials as ready", async () => {
    const { calls } = server([
      fixture({
        kind: "slack",
        state: "needs_reconnect",
        credential_present: false,
      }),
    ]);
    render(<SourceConnections {...props} />);
    await openDetails();
    expect(screen.getByRole("button", { name: "지금 동기화" })).toBeDisabled();
    await userEvent.type(
      screen.getByLabelText("새 Slack 수집 토큰"),
      "synthetic-reconnect",
    );
    await userEvent.click(screen.getByRole("button", { name: "Slack 재연결" }));
    expect(screen.getByLabelText("새 Slack 수집 토큰")).toHaveValue("");
    expect(calls.find((c) => c.path.endsWith("/slack-token"))?.body).toEqual({
      token: "synthetic-reconnect",
    });
  });
  it.each(["keep", "delete"] as const)(
    "disconnects with explicit %s policy",
    async (mode) => {
      const { calls } = server();
      render(<SourceConnections {...props} />);
      await openDetails();
      await userEvent.click(screen.getByRole("button", { name: "연결 해제" }));
      if (mode === "delete")
        await userEvent.click(
          screen.getByRole("radio", { name: "원문과 연관 기억 삭제" }),
        );
      await userEvent.click(
        screen.getByRole("button", {
          name: mode === "keep" ? "자료 보관 후 해제" : "자료 삭제 후 해제",
        }),
      );
      await waitFor(() =>
        expect(
          screen.queryByRole("region", { name: "연결 해제 확인" }),
        ).not.toBeInTheDocument(),
      );
      expect(calls.find((c) => c.path.endsWith("/disconnect"))?.body).toEqual({
        expected_revision: 3,
        mode,
      });
      expect(
        screen.queryByRole("button", { name: "토큰 교체" }),
      ).not.toBeInTheDocument();
    },
  );
  it("reveals token once, copies only on request, and removes it on close/rotate/revoke", async () => {
    const user = userEvent.setup();
    const copy = vi.spyOn(navigator.clipboard, "writeText");
    const { calls } = server();
    render(<SourceConnections {...props} />);
    await openDetails();
    await user.click(screen.getByRole("button", { name: "토큰 교체" }));
    await screen.findByLabelText("수집 토큰");
    expect(copy).not.toHaveBeenCalled();
    expect(calls.find((c) => c.path.endsWith("/token"))?.body).toEqual({
      ttl_seconds: 86400,
    });
    await user.click(screen.getByRole("button", { name: "토큰 복사" }));
    expect(copy).toHaveBeenCalledWith("synthetic-once-secret");
    await user.click(screen.getByRole("button", { name: "토큰 표시 닫기" }));
    expect(screen.queryByLabelText("수집 토큰")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "토큰 교체" }));
    await screen.findByLabelText("수집 토큰");
    await user.click(screen.getByRole("button", { name: "토큰 철회" }));
    expect(screen.queryByLabelText("수집 토큰")).not.toBeInTheDocument();
    await screen.findByRole("button", { name: "토큰 발급" });
    expect(localStorage.length).toBe(0);
    expect(sessionStorage.length).toBe(0);
  });
  it("removes an expired one-time token from the page", async () => {
    const { handler } = server();
    render(<SourceConnections {...props} />);
    await openDetails();
    handler.mockImplementationOnce(async () =>
      response({
        token: "short-lived-synthetic",
        expires_at: Math.floor(Date.now() / 1000) + 2,
      }),
    );
    vi.useFakeTimers();
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "토큰 교체" }));
    });
    expect(screen.getByLabelText("수집 토큰")).toHaveValue(
      "short-lived-synthetic",
    );
    await act(async () => {
      vi.advanceTimersByTime(2100);
    });
    expect(screen.queryByLabelText("수집 토큰")).not.toBeInTheDocument();
    vi.useRealTimers();
  });
  it("discards stale account responses and clears credentials/forms on account switch", async () => {
    const { handler } = server();
    const { rerender } = render(
      <StrictMode>
        <SourceConnections {...props} />
      </StrictMode>,
    );
    await openDetails();
    await userEvent.click(screen.getByRole("button", { name: "토큰 교체" }));
    await screen.findByLabelText("수집 토큰");
    await userEvent.click(screen.getByRole("button", { name: "Slack 추가" }));
    await userEvent.type(
      screen.getByLabelText("Slack 수집 토큰"),
      "synthetic-unsent",
    );
    const old = deferred<Response>();
    handler.mockImplementationOnce(async () => old.promise);
    await userEvent.click(
      screen.getByRole("button", { name: "상태 새로고침" }),
    );
    rerender(
      <StrictMode>
        <SourceConnections {...props} accountId="account-b" />
      </StrictMode>,
    );
    expect(screen.queryByLabelText("수집 토큰")).not.toBeInTheDocument();
    expect(screen.queryByLabelText("Slack 수집 토큰")).not.toBeInTheDocument();
    await screen.findByRole("button", { name: /내 수집기/ });
    await act(async () => {
      old.resolve(
        response({
          connections: [fixture({ name: "old-account-private" })],
          gmail_configured: true,
        }),
      );
    });
    expect(screen.queryByText(/old-account-private/)).not.toBeInTheDocument();
    expect(
      handler.mock.calls.some(
        ([, init]) =>
          new Headers(init?.headers).get("X-Moosuri-Account") === "account-b",
      ),
    ).toBe(true);
  });
  it("fences delayed token issuance across account switches", async () => {
    const { handler } = server();
    const { rerender } = render(<SourceConnections {...props} />);
    await openDetails();
    const old = deferred<Response>();
    handler.mockImplementationOnce(async () => old.promise);
    await userEvent.click(screen.getByRole("button", { name: "토큰 교체" }));
    rerender(<SourceConnections {...props} accountId="account-b" />);
    await act(async () => {
      old.resolve(
        response({
          token: "old-account-secret",
          expires_at: Math.floor(Date.now() / 1000) + 100,
        }),
      );
    });
    expect(
      screen.queryByDisplayValue("old-account-secret"),
    ).not.toBeInTheDocument();
  });
  it("ignores stale run history when another connection is selected", async () => {
    const { handler } = server(
      [fixture(), fixture({ id: "connection-b", name: "다른 수집기" })],
      [],
    );
    render(<SourceConnections {...props} />);
    const old = deferred<Response>();
    handler.mockImplementationOnce(async () => old.promise);
    await openDetails();
    await userEvent.click(screen.getByRole("button", { name: /다른 수집기/ }));
    await screen.findByText("아직 실행 이력이 없습니다.");
    await act(async () => {
      old.resolve(response({ runs: [run({ state: "failed" })] }));
    });
    expect(screen.queryByText("수집기 보고: 실패")).not.toBeInTheDocument();
    expect(
      screen.getByRole("heading", { level: 3, name: "다른 수집기" }),
    ).toBeInTheDocument();
  });
  it("does not overwrite a completed pause with an older refresh response", async () => {
    const { handler } = server();
    render(<SourceConnections {...props} />);
    await openDetails();
    const old = deferred<Response>();
    handler.mockImplementationOnce(async () => old.promise);
    await userEvent.click(
      screen.getByRole("button", { name: "상태 새로고침" }),
    );
    await userEvent.click(
      screen.getByRole("button", { name: "수집 일시정지" }),
    );
    await screen.findByRole("button", { name: "수집 재개" });
    await act(async () => {
      old.resolve(
        response({ connections: [fixture()], gmail_configured: true }),
      );
    });
    expect(
      screen.getByRole("button", { name: "수집 재개" }),
    ).toBeInTheDocument();
  });
  it("labels bounded Slack polling without claiming full history completion", async () => {
    server(
      [fixture({ kind: "slack", last_completed_at: null })],
      [run({ state: "polled", producer_report: false })],
    );
    render(<SourceConnections {...props} />);
    await openDetails();
    await screen.findByText("설정한 기간 조회 완료");
    expect(
      screen.getByText(/재확인 기간 밖의 답글과 원본 삭제는 감지하지 못하며/),
    ).toBeInTheDocument();
    expect(screen.queryByText("범위 동기화 완료")).not.toBeInTheDocument();
  });
});

it("drills down from source provenance to the safe receipt without checkpoints", async () => {
  const fetch = vi.fn(
    async (_url: string | URL | Request, _init?: RequestInit) =>
      response({
        request_id: "request/synthetic",
        run_id: "run-a",
        sequence: 5,
        committed_at: 120,
        accepted: 1,
        unchanged: 0,
        source_ids: ["source-a"],
      }),
  );
  vi.stubGlobal("fetch", fetch);
  render(
    <SourceProvenance
      accountId="account-a"
      onExpired={props.onExpired}
      provenance={{
        connection_id: "connection-a",
        connection_name: "연구 메일",
        kind: "gmail",
        scope: "INBOX",
        external_id: "object-a",
        external_version: "v2",
        thread_id: null,
        parent_id: null,
        request_id: "request/synthetic",
        run_id: "run-a",
        source_revision: 2,
        received_at: 120,
      }}
    />,
  );
  expect(
    screen.getByRole("link", { name: "수집 연결과 실행 이력 보기" }),
  ).toHaveAttribute("href", "/settings/sources?connection=connection-a");
  await userEvent.click(screen.getByText("원본과 저장 기록"));
  await userEvent.click(
    screen.getByRole("button", { name: "수집 저장 기록 확인" }),
  );
  await screen.findByText(/^자료 저장 확인:/);
  expect(String(fetch.mock.calls[0]?.[0])).toBe(
    "/api/v1/source-connections/connection-a/receipts/request%2Fsynthetic",
  );
  expect(screen.getByText("object-a")).toBeInTheDocument();
  expect(
    screen.getByText(/원본 내용이나 전체 범위의 완료를 증명하지 않습니다/),
  ).toBeInTheDocument();
  expect(screen.queryByText(/checkpoint/)).not.toBeInTheDocument();
});
