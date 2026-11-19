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
  const props = { account, expired: vi.fn(), unavailable: vi.fn(), onConnect: vi.fn() };
  return { props, ...render(<PersonalGoogleSetup {...props} />) };
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

it("resumes the selected project and hands all five API activations and the OAuth app to the official console", async () => {
  const requests = setup();
  const user = userEvent.setup();
  const view = mount();
  const project = await screen.findByRole("textbox", { name: "프로젝트 ID" });
  const consoleOrigin = "https://console.cloud.google.com";
  expect(screen.getByRole("link", { name: "기존 프로젝트 선택" })).toHaveAttribute("href", consoleOrigin + "/cloud-resource-manager");
  expect(screen.getByRole("link", { name: "새 프로젝트 만들기" })).toHaveAttribute("href", consoleOrigin + "/projectcreate");
  await user.type(project, "demo-kiri");
  await user.click(screen.getByRole("button", { name: "프로젝트 사용" }));
  const enable = screen.getByRole("link", { name: "Google API 5개 사용 설정" });
  expect(enable).toHaveAttribute("target", "_blank");
  expect(enable).toHaveAttribute("rel", "noopener noreferrer");
  const url = new URL(enable.getAttribute("href") ?? "");
  expect(url.origin + url.pathname).toBe(consoleOrigin + "/apis/enableflow");
  expect(url.searchParams.get("project")).toBe("demo-kiri");
  expect(url.searchParams.get("apiid")?.split(",")).toEqual([
    "gmail.googleapis.com", "drive.googleapis.com", "docs.googleapis.com", "slides.googleapis.com", "sheets.googleapis.com",
  ]);
  await user.click(screen.getByRole("button", { name: "사용 설정을 마쳤습니다" }));
  view.unmount();
  mount();
  expect(await screen.findByRole("heading", { name: "OAuth 동의 화면" })).toBeVisible();
  expect(screen.getByRole("link", { name: "사용 대상 및 테스트 사용자" })).toHaveAttribute("href", consoleOrigin + "/auth/audience?project=demo-kiri");
  await user.click(screen.getByRole("button", { name: "동의 화면 설정을 마쳤습니다" }));
  expect(screen.getByRole("link", { name: "Google OAuth 클라이언트 열기" })).toHaveAttribute("href", consoleOrigin + "/auth/clients?project=demo-kiri");
  expect(requests).toEqual([{ method: "GET", account }, { method: "GET", account }]);
  expect(JSON.parse(localStorage.getItem(draftKey) ?? "null")).toEqual({ project: "demo-kiri", step: 3 });
});

it("copies the callback and imports an existing JSON through the account-scoped API only", async () => {
  const requests = setup();
  const user = userEvent.setup();
  const writeText = vi.spyOn(navigator.clipboard, "writeText");
  const { props } = mount();
  await user.click(await screen.findByRole("button", { name: "이미 받은 JSON 가져오기" }));
  const callback = screen.getByRole("textbox", { name: "이 설치의 콜백 주소" });
  expect(callback).toHaveAttribute("readonly");
  expect(callback).toHaveValue(redirect_uri);
  await user.click(screen.getByRole("button", { name: "콜백 주소 복사" }));
  expect(writeText).toHaveBeenCalledExactlyOnceWith(redirect_uri);
  await user.upload(screen.getByLabelText("Google 웹 클라이언트 JSON"), jsonFile(client));
  await user.click(screen.getByRole("button", { name: "이 설치에 저장" }));
  expect(await screen.findByText("Google 앱 설정을 저장했습니다.")).toBeVisible();
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
  await user.click(await screen.findByRole("button", { name: "이미 받은 JSON 가져오기" }));
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

it("does not transmit a pending file read after an account switch", async () => {
  const requests = setup();
  const user = userEvent.setup();
  let release: (value: string) => void = () => {};
  const pending = new Promise<string>(resolve => { release = resolve; });
  const view = mount();
  await user.click(await screen.findByRole("button", { name: "이미 받은 JSON 가져오기" }));
  const file = jsonFile(pending);
  const read = vi.spyOn(file, "text");
  await user.upload(screen.getByLabelText("Google 웹 클라이언트 JSON"), file);
  await user.click(screen.getByRole("button", { name: "이 설치에 저장" }));
  expect(read).toHaveBeenCalledOnce();
  view.rerender(<PersonalGoogleSetup {...view.props} account="other-installer" />);
  await screen.findByRole("textbox", { name: "프로젝트 ID" });
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
  await user.click(await screen.findByRole("button", { name: "이미 받은 JSON 가져오기" }));
  await user.upload(screen.getByLabelText("Google 웹 클라이언트 JSON"), jsonFile(client));
  await user.click(screen.getByRole("button", { name: "이 설치에 저장" }));
  await screen.findByText("Google 앱 설정이 등록되어 있습니다.");
  expect(screen.queryByLabelText("Google 웹 클라이언트 JSON")).not.toBeInTheDocument();
  await waitFor(() => expect(localStorage.getItem(draftKey)).toBeNull());
});
