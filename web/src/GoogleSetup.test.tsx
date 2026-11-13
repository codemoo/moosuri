import { StrictMode } from "react";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vitest";
import { GoogleSetup } from "./GoogleSetup";
import type { GoogleSetupStatus } from "../shared/api-types";

const redirect = "https://moosuri.example.test/api/v1/mail/oauth/callback";
const secret = "synthetic-secret-never-render";
const raw = JSON.stringify({ web: { client_id: "synthetic.apps.googleusercontent.com", client_secret: secret, redirect_uris: [redirect] } });
let status: GoogleSetupStatus;
let requests: { path: string; method: string; body: any; account: string | null }[];
let failure: { path: string; code: string; status?: number } | null;
function jsonFile(value = raw) {
  const file = new File([value], "client.json", { type: "application/json" });
  Object.defineProperty(file, "text", { configurable: true, value: () => Promise.resolve(value) });
  return file;
}
function show(accountId = "installer") {
  return render(<GoogleSetup accountId={accountId} onExpired={vi.fn()} />);
}
async function importAndValidate(user: ReturnType<typeof userEvent.setup>) {
  await user.upload(await screen.findByLabelText("Google 웹 클라이언트 JSON"), jsonFile());
  await user.click(screen.getByRole("button", { name: "파일과 콜백 확인" }));
  await screen.findByText("파일 형식과 콜백 일치 확인");
}
beforeEach(() => {
  status = { revision: 3, mode: "unconfigured", configured: false, client_id: null, redirect_uri: redirect, saved_redirect_uri: null, redirect_matches: false, personal_connections: [] };
  requests = []; failure = null;
  window.history.replaceState(null, "", "/admin/google");
  vi.stubGlobal("fetch", vi.fn(async (input: string, init: RequestInit = {}) => {
    const path = input.replace("/api/v1", "");
    const method = init.method ?? "GET";
    const body = init.body ? JSON.parse(init.body as string) : null;
    requests.push({ path, method, body, account: new Headers(init.headers).get("X-Moosuri-Account") });
    if (failure?.path === path && method !== "GET") return Response.json({ code: failure.code, provider_body: secret }, { status: failure.status ?? 409 });
    if (path === "/admin/google" && method === "GET") return Response.json(status);
    if (path === "/admin/google/validate") return Response.json({ client_id: "synthetic.apps.googleusercontent.com", redirect_uri: redirect, local_valid: true });
    if (path === "/admin/google" && method === "PUT") {
      status = { ...status, revision: 4, mode: "managed", configured: true, client_id: "synthetic.apps.googleusercontent.com", saved_redirect_uri: redirect, redirect_matches: true };
      return Response.json(status);
    }
    if (path === "/admin/google/check") return Response.json({ email: "mine@example.test", checked_at: 1234 });
    if (path === "/mail/oauth/complete") {
      const connection = { id: "mine", email: "mine@example.test", epoch: 1, status: "active" };
      status.personal_connections = [connection];
      return Response.json(connection);
    }
    if (path === "/admin/google/oauth/start") return Response.json({ code: "google_authorization_in_progress" }, { status: 409 });
    throw new Error("Unexpected synthetic fixture route");
  }));
});
it("separates local JSON validation, explicit save, and a real personal profile check", async () => {
  const user = userEvent.setup();
  const storage = vi.spyOn(Storage.prototype, "setItem");
  show();
  await importAndValidate(user);
  expect(requests.some(r => r.method === "PUT")).toBe(false);
  expect(screen.queryByText(/Google 프로필 확인 성공/)).toBeNull();
  expect(document.body.textContent).not.toContain(secret);
  expect(screen.queryByRole("textbox", { name: /JSON/ })).toBeNull();
  await user.click(screen.getByRole("button", { name: "이 설치에 저장" }));
  await screen.findByText(/설치의 Google 앱 설정을 저장했습니다/);
  expect(screen.getByLabelText("Google 웹 클라이언트 JSON")).toHaveValue("");
  expect(screen.getByRole("button", { name: "이 설치에 저장" })).toBeDisabled();
  expect(requests.find(r => r.method === "PUT")?.body).toEqual({ expected_revision: 3, client_json: raw });
  expect(requests.some(r => r.path === "/admin/google/check")).toBe(false);
  status.personal_connections = [{ id: "mine", email: "mine@example.test", epoch: 1, status: "active" }];
  await user.click(screen.getByRole("button", { name: "상태 새로고침" }));
  await user.click(await screen.findByRole("button", { name: "내 연결 프로필 확인" }));
  await screen.findByText("Google 프로필 확인 성공 · mine@example.test");
  expect(requests.find(r => r.path === "/admin/google/check")?.body).toEqual({ connection_id: "mine" });
  expect(requests.every(r => r.account === "installer")).toBe(true);
  expect(storage).not.toHaveBeenCalled();
  storage.mockRestore();
});
it("rejects installed-client, malformed and oversized files without requesting validation", async () => {
  const user = userEvent.setup(); show();
  const input = await screen.findByLabelText("Google 웹 클라이언트 JSON");
  for (const value of ['{"installed":{}}', "not json", "x".repeat(16385)]) {
    await user.upload(input, jsonFile(value));
    await screen.findByRole("alert");
    expect(screen.getByRole("button", { name: "파일과 콜백 확인" })).toBeDisabled();
  }
  expect(requests.filter(r => r.path.endsWith("/validate"))).toHaveLength(0);
});
it("wrong callback cannot be saved; selecting a new file invalidates previous validation", async () => {
  const user = userEvent.setup(); show();
  failure = { path: "/admin/google/validate", code: "google_redirect_uri_mismatch", status: 400 };
  await user.upload(await screen.findByLabelText("Google 웹 클라이언트 JSON"), jsonFile());
  await user.click(screen.getByRole("button", { name: "파일과 콜백 확인" }));
  await screen.findByText(/콜백 주소가 다릅니다/);
  expect(screen.getByRole("button", { name: "이 설치에 저장" })).toBeDisabled();
  failure = null;
  await user.click(screen.getByRole("button", { name: "파일과 콜백 확인" }));
  await screen.findByText("파일 형식과 콜백 일치 확인");
  await user.upload(screen.getByLabelText("Google 웹 클라이언트 JSON"), jsonFile(raw.replace(secret, "another-synthetic-secret")));
  await waitFor(() => expect(screen.getByRole("button", { name: "이 설치에 저장" })).toBeDisabled());
  expect(screen.queryByText("파일 형식과 콜백 일치 확인")).toBeNull();
  expect(document.body.textContent).not.toContain(secret);
});
it("stale revision clears imported credentials, reloads latest status, and requires a new file", async () => {
  const user = userEvent.setup(); show(); await importAndValidate(user);
  failure = { path: "/admin/google", code: "revision_conflict" };
  status.revision = 9;
  await user.click(screen.getByRole("button", { name: "이 설치에 저장" }));
  await screen.findByText(/설정이 변경되었습니다/);
  expect(screen.getByRole("button", { name: "이 설치에 저장" })).toBeDisabled();
  expect(screen.getByLabelText("Google 웹 클라이언트 JSON")).toHaveValue("");
  failure = null; await importAndValidate(user);
  await user.click(screen.getByRole("button", { name: "이 설치에 저장" }));
  await screen.findByText(/설치의 Google 앱 설정을 저장했습니다/);
  expect(requests.filter(r => r.method === "PUT").map(r => r.body.expected_revision)).toEqual([3, 9]);
});
it("explains pending OAuth without leaking provider errors and allows a fresh retry", async () => {
  status = { ...status, configured: true, redirect_matches: true, mode: "managed" };
  const user = userEvent.setup(); show();
  const button = await screen.findByRole("button", { name: "내 Google 계정 연결" });
  await user.click(button);
  await screen.findByText(/Google 로그인이 진행 중입니다/);
  expect(button).toBeEnabled();
  await user.click(button);
  await waitFor(() => expect(requests.filter(r => r.path === "/admin/google/oauth/start")).toHaveLength(2));
  expect(document.body.textContent).not.toContain(secret);
});
it("operator file mode retains callback and own check but has no import or save", async () => {
  status = { ...status, mode: "operator_file", configured: true, redirect_matches: true, personal_connections: [{ id: "mine", email: "mine@example.test", epoch: 1, status: "active" }] };
  const user = userEvent.setup(); show();
  await screen.findByText(/운영자의/);
  expect(screen.getByLabelText("이 설치의 콜백 주소")).toHaveValue(redirect);
  expect(screen.queryByLabelText("Google 웹 클라이언트 JSON")).toBeNull();
  expect(screen.queryByRole("button", { name: "이 설치에 저장" })).toBeNull();
  await user.click(screen.getByRole("button", { name: "내 연결 프로필 확인" }));
  await screen.findByText("Google 프로필 확인 성공 · mine@example.test");
});
it("origin mismatch explains repair and blocks authorization even with a saved app", async () => {
  status.configured = false; status.mode = "managed";
  status.client_id = "synthetic-client"; status.saved_redirect_uri = "https://old.example.test/api/v1/mail/oauth/callback";
  show();
  await screen.findByText(/서버 접속 주소와 저장된 콜백이 다릅니다/);
  expect(screen.getByRole("button", { name: "내 Google 계정 연결" })).toBeDisabled();
  expect(screen.getByText("설치 설정: 콜백 갱신 필요")).toBeVisible();
});
it("profile failure stays distinct from a successful local setup", async () => {
  status = { ...status, configured: true, redirect_matches: true, mode: "managed", personal_connections: [{ id: "mine", email: "mine@example.test", epoch: 1, status: "active" }] };
  failure = { path: "/admin/google/check", code: "google_profile_mismatch", status: 400 };
  const user = userEvent.setup(); show();
  await user.click(await screen.findByRole("button", { name: "내 연결 프로필 확인" }));
  await screen.findByText(/연결한 계정과 Google 프로필이 다릅니다/);
  expect(screen.queryByText(/Google 프로필 확인 성공/)).toBeNull();
  expect(document.body.textContent).not.toContain(secret);
  expect(screen.getByRole("button", { name: "내 연결 프로필 확인" })).toBeEnabled();
});
it("an account switch clears validation and ignores a late file read", async () => {
  let release: (raw: string) => void = () => {};
  const file = jsonFile();
  Object.defineProperty(file, "text", { configurable: true, value: () => new Promise<string>(resolve => { release = resolve; }) });
  const user = userEvent.setup(); const view = show();
  await user.upload(await screen.findByLabelText("Google 웹 클라이언트 JSON"), file);
  view.rerender(<GoogleSetup accountId="member" onExpired={vi.fn()} />);
  await act(async () => { release(raw); });
  await screen.findByLabelText("Google 웹 클라이언트 JSON");
  expect(screen.getByRole("button", { name: "파일과 콜백 확인" })).toBeDisabled();
  expect(screen.getByLabelText("Google 웹 클라이언트 JSON")).toHaveValue("");
});
it("a later file selection wins over a slow previous file read", async () => {
  let release: (raw: string) => void = () => {};
  const slow = jsonFile();
  Object.defineProperty(slow, "text", { configurable: true, value: () => new Promise<string>(resolve => { release = resolve; }) });
  const user = userEvent.setup(); show();
  const input = await screen.findByLabelText("Google 웹 클라이언트 JSON");
  await user.upload(input, slow);
  await user.upload(input, jsonFile("{}"));
  await screen.findByRole("alert");
  await act(async () => { release(raw); });
  expect(screen.getByRole("button", { name: "파일과 콜백 확인" })).toBeDisabled();
});
it("late status responses cannot cross account lifetimes even if transport ignores abort", async () => {
  const base = vi.mocked(fetch).getMockImplementation()!;
  let release: (value: Response) => void = () => {};
  const held = new Promise<Response>(resolve => { release = resolve; });
  vi.mocked(fetch).mockImplementation((input, init) => new Headers(init?.headers).get("X-Moosuri-Account") === "installer" ? held : base(input, init));
  const view = show();
  view.rerender(<GoogleSetup accountId="member" onExpired={vi.fn()} />);
  await screen.findByLabelText("Google 웹 클라이언트 JSON");
  await act(async () => { release(Response.json({ ...status, client_id: "previous-account-private-id" })); });
  expect(screen.queryByText(/previous-account-private-id/)).toBeNull();
});
it("callback strips its handle immediately and completes exactly once under StrictMode", async () => {
  status.configured = true; status.redirect_matches = true;
  window.history.replaceState(null, "", "/admin/google?oauth=synthetic-once&keep=1");
  render(<StrictMode><GoogleSetup accountId="installer" onExpired={vi.fn()} /></StrictMode>);
  expect(window.location.search).toBe("?keep=1");
  await screen.findByText("Google 프로필 확인 성공 · mine@example.test");
  expect(requests.filter(r => r.path === "/mail/oauth/complete")).toHaveLength(1);
  expect(requests.some(r => /sync|search|send/.test(r.path))).toBe(false);
});
it("failed callback clears query and busy state without automatic completion replay", async () => {
  status.configured = true; status.redirect_matches = true;
  failure = { path: "/mail/oauth/complete", code: "google_oauth_exchange_failed", status: 400 };
  window.history.replaceState(null, "", "/admin/google?oauth=synthetic-failed");
  const user = userEvent.setup(); const view = show();
  await screen.findByText(/Google 연결을 완료하지 못했습니다/);
  expect(window.location.search).toBe("");
  expect(screen.getByRole("button", { name: "내 Google 계정 연결" })).toBeEnabled();
  await user.click(screen.getByRole("button", { name: "상태 새로고침" }));
  view.unmount(); show();
  await screen.findByRole("heading", { name: "본인 Google 계정 연결" });
  expect(requests.filter(r => r.path === "/mail/oauth/complete")).toHaveLength(1);
  expect(screen.queryByText(/Google 프로필 확인 성공/)).toBeNull();
});
it("denied callback cleans up and offers a fresh account connection", async () => {
  status.configured = true; status.redirect_matches = true;
  window.history.replaceState(null, "", "/admin/google?oauth_error=denied");
  show();
  await screen.findByText(/Google 연결을 허락하지 않았습니다/);
  expect(window.location.search).toBe("");
  expect(requests.some(r => r.path === "/mail/oauth/complete")).toBe(false);
  expect(screen.getByRole("button", { name: "내 Google 계정 연결" })).toBeEnabled();
});
