import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import { PersonalGoogleSetup } from "./PersonalGoogleSetup";

const endpoint = "/api/v1/personal/admin/google-setup";
const account = "installer";
const redirect_uri = "https://kiri.example.test/api/v1/mail/oauth/callback";
const secret = "synthetic-test-secret";
const client = JSON.stringify({ web: { client_id: "demo", client_secret: secret, redirect_uris: [redirect_uri] } });
const draftKey = "moosuri.personal.google-setup.v1." + account;

function setup({ configured = false, failure = "", status = 400 } = {}) {
  const requests: { method: string; body?: unknown; account: string | null }[] = [];
  vi.stubGlobal("fetch", vi.fn(async (path: string, init: RequestInit) => {
    const method = init.method ?? "GET";
    expect(path).toBe(endpoint);
    expect(init.credentials).toBe("same-origin");
    expect(init.cache).toBe("no-store");
    requests.push({ method, body: init.body && JSON.parse(String(init.body)), account: new Headers(init.headers).get("X-Moosuri-Account") });
    if (method === "POST" && failure) return Response.json({ code: failure, detail: secret }, { status });
    return Response.json({ configured: method === "POST" || configured, redirect_uri });
  }));
  return requests;
}

function mount() {
  const props = { account, expired: vi.fn(), unavailable: vi.fn(), onConnect: vi.fn(), onStart: vi.fn() };
  return { props, ...render(<PersonalGoogleSetup {...props} />) };
}

async function openManual(user: ReturnType<typeof userEvent.setup>) {
  const summary = await screen.findByText(/고급: 기존 Google 웹 클라이언트 JSON/);
  await user.click(summary);
  expect(summary.closest("details")).toHaveAttribute("open");
}

function jsonFile(text: string | Promise<string>) {
  const file = new File([typeof text === "string" ? text : ""], "google-client.json", { type: "application/json" });
  Object.defineProperty(file, "text", { value: () => Promise.resolve(text), configurable: true });
  return file;
}

afterEach(() => {
  cleanup();
  localStorage.clear();
  vi.unstubAllGlobals();
});

it("lets a first installer continue to Google connection without any JSON, upload, or configured claim", async () => {
  const requests = setup();
  const user = userEvent.setup();
  localStorage.setItem(draftKey, JSON.stringify({project:"example",step:1,method:"shell"}));
  const { props } = mount();
  const start = await screen.findByRole("button", { name: "서비스 선택하고 계속" });
  expect(screen.getByText(/각 회원이 자신의 계정으로 따로 동의/)).toBeVisible();
  expect(screen.getByText(/Google이 추가 확인을 요청하면/)).toBeVisible();
  expect(screen.getByText(/고급: 기존 Google 웹 클라이언트 JSON/).closest("details")).not.toHaveAttribute("open");
  expect(screen.queryByRole("link")).not.toBeInTheDocument();
  await user.click(start);
  expect(props.onStart).toHaveBeenCalledOnce();
  expect(props.onConnect).not.toHaveBeenCalled();
  expect(requests).toEqual([{ method: "GET", account }]);
  expect(localStorage.getItem(draftKey)).toBeNull();
});

it("copies the callback and imports an existing JSON through the account-scoped API only", async () => {
  const requests = setup();
  const user = userEvent.setup();
  const writeText = vi.spyOn(navigator.clipboard, "writeText");
  const { props } = mount();
  await openManual(user);
  const callback = screen.getByRole("textbox", { name: "이 설치의 콜백 주소" });
  expect(callback).toHaveAttribute("readonly");
  expect(callback).toHaveValue(redirect_uri);
  await user.click(screen.getByRole("button", { name: "콜백 주소 복사" }));
  expect(writeText).toHaveBeenCalledExactlyOnceWith(redirect_uri);
  await user.upload(screen.getByLabelText("Google 웹 클라이언트 JSON"), jsonFile(client));
  await user.click(screen.getByRole("button", { name: "이 설치에 저장" }));
  const result = await screen.findByRole("heading", { name: "Google 앱 설정을 저장했습니다." });
  expect(result).toHaveFocus();
  expect(requests).toEqual([{ method: "GET", account }, { method: "POST", body: { web_client_json: client }, account }]);
  expect(localStorage.getItem(draftKey)).toBeNull();
  expect(sessionStorage.length).toBe(0);
  expect(document.body).not.toHaveTextContent(secret);
  await user.click(screen.getByRole("button", { name: "커넥터 열기" }));
  expect(props.onConnect).toHaveBeenCalledOnce();
});

it("shows a repair message without rendering provider details when the callback differs", async () => {
  const requests = setup({ failure: "google_redirect_uri_mismatch" });
  const user = userEvent.setup();
  mount();
  await openManual(user);
  const file = screen.getByLabelText("Google 웹 클라이언트 JSON");
  await user.upload(file, jsonFile("x".repeat(16385)));
  await user.click(screen.getByRole("button", { name: "이 설치에 저장" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("16KB");
  expect(requests).toHaveLength(1);
  await user.upload(file, jsonFile(client));
  await user.click(screen.getByRole("button", { name: "이 설치에 저장" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("콜백 주소가 다릅니다.");
  expect(file).toHaveValue("");
  expect(screen.getByRole("button", { name: "이 설치에 저장" })).toBeDisabled();
  expect(document.body).not.toHaveTextContent(secret);
});

it.each([
  [{ installed: { client_secret: secret } }, "데스크톱용 OAuth 파일"],
  [{ web: { client_id: "test", client_secret: secret, redirect_uris: ["https://other.example.test/callback"] } }, "콜백 주소가 다릅니다"],
  [{ web: { client_id: "test" } }, "웹 애플리케이션 클라이언트"],
])("keeps an incompatible client file on the device", async (invalid, message) => {
  const requests = setup();
  const user = userEvent.setup();
  mount();
  await openManual(user);
  await user.upload(screen.getByLabelText("Google 웹 클라이언트 JSON"), jsonFile(JSON.stringify(invalid)));
  await user.click(screen.getByRole("button", { name: "이 설치에 저장" }));
  expect(await screen.findByRole("alert")).toHaveTextContent(String(message));
  expect(requests.map(r => r.method)).toEqual(["GET"]);
  expect(document.body).not.toHaveTextContent(secret);
});

it("does not transmit a pending file read after an account switch", async () => {
  const requests = setup();
  const user = userEvent.setup();
  let release: (value: string) => void = () => {};
  const pending = new Promise<string>(resolve => { release = resolve; });
  const view = mount();
  await openManual(user);
  const file = jsonFile(pending);
  const read = vi.spyOn(file, "text");
  await user.upload(screen.getByLabelText("Google 웹 클라이언트 JSON"), file);
  await user.click(screen.getByRole("button", { name: "이 설치에 저장" }));
  expect(read).toHaveBeenCalledOnce();
  view.rerender(<PersonalGoogleSetup {...view.props} account="other-installer" />);
  await screen.findByLabelText("Google 웹 클라이언트 JSON");
  await act(async () => release(client));
  expect(requests).toEqual([{ method: "GET", account }, { method: "GET", account: "other-installer" }]);
});

it("uses the stored status to resolve a conflicting upload", async () => {
  const requests = setup({ failure: "google_setup_already_configured", status: 409 });
  const refresh = vi.mocked(fetch).getMockImplementation()!;
  vi.mocked(fetch).mockImplementation(async (path, init) => {
    const response = await refresh(path, init);
    return (init?.method === "GET" && requests.length > 1) ? Response.json({ configured: true, redirect_uri }) : response;
  });
  const user = userEvent.setup();
  mount();
  await openManual(user);
  await user.upload(screen.getByLabelText("Google 웹 클라이언트 JSON"), jsonFile(client));
  await user.click(screen.getByRole("button", { name: "이 설치에 저장" }));
  await screen.findByText("Google 앱 설정이 등록되어 있습니다.");
  expect(screen.queryByLabelText("Google 웹 클라이언트 JSON")).not.toBeInTheDocument();
  await waitFor(() => expect(localStorage.getItem(draftKey)).toBeNull());
});
