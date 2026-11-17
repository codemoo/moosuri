import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { TelegramSettings, type TelegramConfiguration } from "./TelegramSettings";

let state: TelegramConfiguration;
let requests: { path: string; method: string; body: any; account: string | null }[];
let rejectSave = false;
const syntheticToken = "synthetic-bot-token";
const pairCode = "synthetic-once-code";
const binding = { id: "binding-a", telegram_user_id: 123456, thread_id: "thread-a", notifications: false, timezone: "Asia/Seoul", last_delivery: "unknown" };
beforeEach(() => {
  state = { bot: { revision: 3, username: "synthetic_bot", enabled: true, status: "polling", last_poll_at: 1234 }, binding: null };
  requests = []; rejectSave = false;
  vi.stubGlobal("fetch", vi.fn(async (input: string, init: RequestInit = {}) => {
    const path = input.replace("/api/v1", ""), method = init.method ?? "GET";
    const body = init.body ? JSON.parse(init.body as string) : null;
    requests.push({ path, method, body, account: new Headers(init.headers).get("X-Moosuri-Account") });
    if (method === "GET") return Response.json(state);
    if (path === "/admin/telegram" && method === "PUT") {
      if (rejectSave) return Response.json({ code: "telegram_invalid_token", description: syntheticToken }, { status: 400 });
      state = { ...state, bot: { ...state.bot, revision: 4, status: "connected" } };
    }
    if (path.endsWith("/pair")) return Response.json({ url: `https://t.me/synthetic_bot?start=${pairCode}`, code: pairCode, expires_at: Math.floor(Date.now() / 1000) + 600 });
    if (path === "/settings/telegram" && method === "PUT") state = { ...state, binding: { ...binding, notifications: body.notifications } };
    if (method === "DELETE") state = { ...state, binding: null, bot: path.startsWith("/admin") ? { ...state.bot, enabled: false, status: "disabled", revision: 5 } : state.bot };
    return new Response(null, { status: 204 });
  }));
});
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

it("clears pasted tokens immediately on success and failure and saves the observed revision", async () => {
  const user = userEvent.setup();
  render(<TelegramSettings accountId="admin-a" admin onExpired={vi.fn()} />);
  const input = await screen.findByLabelText("BotFather 토큰");
  expect(input).toHaveAttribute("type", "password");
  expect(input).toHaveAttribute("autocomplete", "off");
  await user.type(input, syntheticToken);
  await user.click(screen.getByRole("button", { name: "봇 확인하고 저장" }));
  expect(input).toHaveValue("");
  await screen.findByText(/봇이 연결되었습니다/);
  expect(requests.find(r => r.method === "PUT")?.body).toEqual({ expected_revision: 3, token: syntheticToken });
  rejectSave = true;
  await user.type(input, syntheticToken);
  await user.click(screen.getByRole("button", { name: "봇 확인하고 저장" }));
  await screen.findByRole("alert");
  expect(input).toHaveValue("");
  expect(document.body.textContent).not.toContain(syntheticToken);
  expect(requests.filter(r => r.method === "PUT")[1].body.expected_revision).toBe(4);
  await user.click(screen.getByRole("button", { name: "봇 끄고 모든 연결 해제" }));
  await screen.findByText(/봇이 꺼져 있습니다/);
  expect(requests.find(r => r.method === "DELETE")?.body).toEqual({ expected_revision: 4 });
  expect(requests.every(r => r.account === "admin-a")).toBe(true);
});

it("pairs with opt-in alerts, polls only the pending code, and can revoke it", async () => {
  const user = userEvent.setup();
  render(<TelegramSettings accountId="account-a" onExpired={vi.fn()} />);
  await screen.findByRole("button", { name: "연결 링크 만들기" });
  const storage = vi.spyOn(Storage.prototype, "setItem");
  expect(screen.getByRole("checkbox")).not.toBeChecked();
  await user.click(screen.getByRole("checkbox"));
  vi.useFakeTimers();
  await act(async () => { screen.getByRole("button", { name: "연결 링크 만들기" }).click(); });
  expect(screen.getByLabelText("연결 코드")).toHaveValue(pairCode);
  expect(screen.getByLabelText("연결 링크")).toHaveValue(`https://t.me/synthetic_bot?start=${pairCode}`);
  expect(screen.getByText(/10분 동안 한 번만/)).toBeVisible();
  expect(requests.find(r => r.path.endsWith("/pair"))?.body).toEqual({ timezone: expect.any(String), notifications: true });
  state = { ...state, binding };
  await act(async () => { await vi.advanceTimersByTimeAsync(3000); });
  expect(screen.getByText("내 계정 연결됨")).toBeVisible();
  expect(screen.queryByLabelText("연결 코드")).toBeNull();
  const count = requests.length;
  await act(async () => { await vi.advanceTimersByTimeAsync(9000); });
  expect(requests).toHaveLength(count);
  vi.useRealTimers();
  await user.click(screen.getByRole("checkbox"));
  await waitFor(() => expect(screen.getByRole("checkbox")).toBeChecked());
  expect(requests.find(r => r.method === "PUT")?.body).toEqual({ binding_id: "binding-a", notifications: true });
  await user.click(screen.getByRole("button", { name: "내 연결·대기 코드 해제" }));
  await screen.findByRole("button", { name: "연결 링크 만들기" });
  expect(requests.some(r => r.method === "DELETE" && r.path === "/settings/telegram")).toBe(true);
  expect(storage).not.toHaveBeenCalled();
  storage.mockRestore();
});

it("does not show an old account's pending pairing response after switching accounts", async () => {
  const user = userEvent.setup();
  const original = vi.mocked(fetch).getMockImplementation()!;
  let complete!: (response: Response) => void;
  let signal: AbortSignal | null | undefined;
  vi.mocked(fetch).mockImplementation((input, init) => {
    if (String(input).endsWith("/pair")) {
      signal = init?.signal;
      return new Promise<Response>(resolve => { complete = resolve; });
    }
    return original(input, init);
  });
  const expired = vi.fn();
  const view = render(<TelegramSettings accountId="account-a" onExpired={expired} />);
  await user.click(await screen.findByRole("button", { name: "연결 링크 만들기" }));
  view.rerender(<TelegramSettings accountId="account-b" onExpired={expired} />);
  await screen.findByRole("button", { name: "연결 링크 만들기" });
  expect(signal?.aborted).toBe(true);
  await act(async () => { complete(Response.json({ url: `https://t.me/synthetic_bot?start=${pairCode}`, code: pairCode, expires_at: Math.floor(Date.now() / 1000) + 600 })); });
  expect(screen.queryByLabelText("연결 코드")).toBeNull();
  expect(document.body.textContent).not.toContain(pairCode);
  expect(expired).not.toHaveBeenCalled();
  expect(requests.at(-1)?.account).toBe("account-b");
});
