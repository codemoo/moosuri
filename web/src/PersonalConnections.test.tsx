// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const scoped = vi.hoisted(() => ({ request: vi.fn() }));
const request = scoped.request;
const google = vi.hoisted(() => ({ status: "disabled", refreshRead: vi.fn(), retry: vi.fn(), connect: vi.fn(), cancel: vi.fn() }));
vi.mock("./account-scope", () => ({ useScope: () => scoped }));
vi.mock("./PersonalGoogle", () => ({
  googleReturn: () => false,
  usePersonalGoogle: () => ({ status: google.status, retry: google.retry, busy: false, action() {}, connection: null, notice: undefined, refreshRead: google.refreshRead }),
}));
vi.mock("./kiri-google-connection", () => ({ connectInKiri: google.connect, cancelGooglePending: google.cancel }));
vi.mock("./personal-google-workspace", () => ({
  isWorkspaceGoogleService: () => false,
  workspaceGoogleReturn: () => null,
  usePersonalGoogleWorkspace: () => ({ status: "disabled", retry: async () => {}, busy: false, action() {}, connections: {}, notice: undefined }),
}));
vi.mock("./PersonalGoogleSetup", () => ({ PersonalGoogleSetup: () => null, installationGoogleSetupPath: "/setup" }));
vi.mock("./WorkspaceNavigation", () => ({ WorkspaceIcon: () => null }));
vi.mock("./member-route", () => ({ setupOpenedFromAdmin: () => false }));
vi.mock("./connector-marks/gmail.png?no-inline", () => ({ default: "mark.png" }));
vi.mock("./connector-marks/drive.png?no-inline", () => ({ default: "mark.png" }));
vi.mock("./connector-marks/docs.png?no-inline", () => ({ default: "mark.png" }));
vi.mock("./connector-marks/slides.png?no-inline", () => ({ default: "mark.png" }));
vi.mock("./connector-marks/sheets.png?no-inline", () => ({ default: "mark.png" }));
import { PersonalConnections } from "./PersonalConnections";
import { ApiFailure } from "./api";
import { clearGooglePending, readGooglePending, writeGooglePending, type GoogleKiriPending } from "./kiri-google-pending";

const ORIGIN = "https://vault.example.test";
const props: Parameters<typeof PersonalConnections>[0] = {
  session: {user: {id: "11111111-1111-4111-8111-111111111111", email: "member@example.test"}} as Parameters<typeof PersonalConnections>[0]["session"],
  expired: vi.fn(),
  onBack: vi.fn(),
  navigation: { route: { view: "connections", service: "vault", setup: false }, open: vi.fn() },
};

describe("personal vault connection", () => {
  const submitted: { action: string; target: string; handoff: string }[] = [];
  beforeEach(() => {
    request.mockReset();
    submitted.length = 0;
    vi.spyOn(HTMLFormElement.prototype, "submit").mockImplementation(function (this: HTMLFormElement) {
      submitted.push({ action: this.action, target: this.target, handoff: new FormData(this).get("handoff") as string });
    });
  });
  afterEach(() => { cleanup(); vi.restoreAllMocks(); });

  it("posts a short-lived handoff to the exact vault origin in a new window without standalone-login copy", async () => {
    const popup = { close: vi.fn() };
    vi.spyOn(window, "open").mockReturnValue(popup as never);
    request.mockImplementation(async (path: string) => path === "/personal/vault"
      ? { state: "ready", site_origin: ORIGIN }
      : { token: "ab".repeat(32), vault_origin: ORIGIN, expires_at: 1 });
    render(<PersonalConnections {...props} />);
    await userEvent.click(await screen.findByRole("button", { name: "볼트 열기" }));
    await waitFor(() => expect(submitted).toHaveLength(1));
    expect(submitted[0].action).toBe(`${ORIGIN}/session`);
    expect(submitted[0].handoff).toBe("ab".repeat(32));
    expect(submitted[0].target).toMatch(/^moosuri-vault-/);
    expect(window.open).toHaveBeenCalledWith("about:blank", submitted[0].target);
    expect(request).toHaveBeenCalledWith("/personal/vault/handoff", "POST", {});
    expect(screen.queryByText(/볼트 계정으로 로그인하세요/)).toBeNull();
    expect(popup.close).not.toHaveBeenCalled();
  });

  it("closes the window and submits nothing when the handoff names another origin", async () => {
    const popup = { close: vi.fn() };
    vi.spyOn(window, "open").mockReturnValue(popup as never);
    request.mockImplementation(async (path: string) => path === "/personal/vault"
      ? { state: "ready", site_origin: ORIGIN }
      : { token: "ab".repeat(32), vault_origin: "https://evil.example.test", expires_at: 1 });
    render(<PersonalConnections {...props} />);
    await userEvent.click(await screen.findByRole("button", { name: "볼트 열기" }));
    await waitFor(() => expect(popup.close).toHaveBeenCalled());
    expect(submitted).toHaveLength(0);
  });
});

describe("saved Google connection recovery", () => {
  const account = props.session.user.id, scope = "a".repeat(64);
  const read = { thread_id: "thread-a", request_id: "request-a" };
  const flow = "b".repeat(64), email = "selected@example.test";
  const saved = (extra: Partial<GoogleKiriPending> = {}): GoogleKiriPending => ({
    job: crypto.randomUUID(), account, scope, service: "gmail", created: Date.now(), updated: Date.now(), read, ...extra,
  });
  const view = (loginScope = scope, readIntent: typeof read | null = null) => render(<PersonalConnections {...props}
    session={{ ...props.session, chat_draft_scope: loginScope } as typeof props.session}
    readIntent={readIntent} navigation={{ route: { view: "connections", service: "gmail", setup: false }, open: vi.fn() }} />);
  const ready = async (name: string) => {
    const button = await screen.findByRole("button", { name });
    await waitFor(() => expect(button).toBeEnabled()); // enabled once the Vault origin is confirmed
    return button;
  };
  beforeEach(() => {
    sessionStorage.clear();
    google.status = "ready";
    google.refreshRead.mockReset(); google.retry.mockReset(); google.connect.mockReset(); google.cancel.mockReset();
    request.mockReset();
    request.mockImplementation(async (path: string) => path === "/personal/vault" ? { state: "ready", site_origin: ORIGIN } : {});
  });
  afterEach(() => { cleanup(); sessionStorage.clear(); });

  it("replaces Start with resume/cancel after reload and finalizes the saved job and conversation exactly once", async () => {
    const pending = saved();
    writeGooglePending(pending);
    google.connect.mockResolvedValue({ job: pending.job, handle: "a".repeat(64), flow, email });
    let completes = 0;
    request.mockImplementation(async (path: string) => {
      if (path === "/personal/vault") return { state: "ready", site_origin: ORIGIN };
      if (path === "/personal/google/complete" && ++completes === 1) throw new ApiFailure(503, "lost");
      return { flow_id: flow, mailbox: email, continuation: read };
    });
    // The page now shows a different conversation than the one that started the job.
    view(scope, { thread_id: "thread-b", request_id: "request-b" });
    await ready("이어서 진행");
    expect(screen.queryByRole("button", { name: /^Google 연결/ })).toBeNull();
    await userEvent.click(screen.getByRole("button", { name: "이어서 진행" }));
    await waitFor(() => expect(completes).toBe(1));
    // The lost acknowledgment keeps the exact record and offers resume again.
    const again = await ready("이어서 진행");
    expect(readGooglePending(account, scope, "gmail")?.job).toBe(pending.job);
    await userEvent.click(again);
    await waitFor(() => expect(readGooglePending(account, scope, "gmail")).toBeNull());
    expect(google.connect).toHaveBeenCalledTimes(2);
    for (const call of google.connect.mock.calls) {
      expect(call[5]).toEqual(read);
      expect(call[8]).toMatchObject({ scope, resume: { job: pending.job } });
    }
    expect(request.mock.calls.filter(([path]) => path === "/personal/google/complete")).toHaveLength(2);
    expect(google.refreshRead).not.toHaveBeenCalled();
  });

  it("continues the original read only when the page still shows that conversation", async () => {
    const pending = saved();
    writeGooglePending(pending);
    google.connect.mockResolvedValue({ job: pending.job, handle: "a".repeat(64), flow, email });
    request.mockImplementation(async (path: string) => path === "/personal/vault" ? { state: "ready", site_origin: ORIGIN }
      : { flow_id: flow, mailbox: email, continuation: read });
    view(scope, read);
    await userEvent.click(await ready("이어서 진행"));
    await waitFor(() => expect(google.refreshRead).toHaveBeenCalledWith(read));
  });

  it("does not read into a conversation changed while completion is pending", async () => {
    const pending=saved();writeGooglePending(pending);
    google.connect.mockResolvedValue({job:pending.job,handle:"a".repeat(64),flow,email});
    let finish: ((value:unknown)=>void)|undefined;
    request.mockImplementation(async (path:string)=>path==="/personal/vault" ? {state:"ready",site_origin:ORIGIN}
      : new Promise(resolve=>{finish=resolve;}));
    const element=(readIntent:typeof read)=><PersonalConnections {...props}
      session={{...props.session,chat_draft_scope:scope} as typeof props.session} readIntent={readIntent}
      navigation={{route:{view:"connections",service:"gmail",setup:false},open:vi.fn()}} />;
    const page=render(element(read));
    await userEvent.click(await ready("이어서 진행"));
    await waitFor(()=>expect(finish).toBeDefined());
    expect(screen.queryByRole("button",{name:"취소"})).toBeNull();
    page.rerender(element({thread_id:"thread-b",request_id:"request-b"}));
    finish!({flow_id:flow,mailbox:email,continuation:read});
    await waitFor(()=>expect(readGooglePending(account,scope,"gmail")).toBeNull());
    expect(google.refreshRead).not.toHaveBeenCalled();
  });

  it("reconciles an already committed one-use handle from member status without replaying mail", async () => {
    const pending=saved();writeGooglePending(pending);
    google.connect.mockResolvedValue({job:pending.job,handle:"a".repeat(64),flow,email});
    request.mockImplementation(async(path:string)=> {
      if(path==="/personal/vault")return {state:"ready",site_origin:ORIGIN};
      if(path==="/personal/google/complete")throw new ApiFailure(409,"already_claimed");
      if(path==="/personal/google")return {connection:{mailbox:email}};
      throw Error("unexpected request");
    });
    view(scope,read);
    await userEvent.click(await ready("이어서 진행"));
    await screen.findByText(/현재 Google 계정이 연결된 것을 확인했습니다/);
    expect(readGooglePending(account,scope,"gmail")).toBeNull();
    expect(request.mock.calls.filter(([path])=>path==="/personal/google/complete")).toHaveLength(1);
    expect(google.refreshRead).not.toHaveBeenCalled();
    expect(google.retry).toHaveBeenCalled();
  });

  it("does not show another login's saved work and keeps it untouched", async () => {
    const pending = saved();
    writeGooglePending(pending);
    view("c".repeat(64));
    await ready("Google 연결");
    expect(screen.queryByRole("button", { name: "이어서 진행" })).toBeNull();
    expect(readGooglePending(account, scope, "gmail")?.job).toBe(pending.job);
  });

  it("offers a fresh choice only after the server confirms the saved cancel", async () => {
    const pending = saved();
    writeGooglePending(pending);
    google.cancel.mockResolvedValueOnce("kept").mockImplementationOnce(async (_r: unknown, record: GoogleKiriPending) => { clearGooglePending(record); return "cleared"; });
    view();
    await userEvent.click(await screen.findByRole("button", { name: "저장된 연결 취소" }));
    expect(await screen.findByText(/취소를 확인하지 못했습니다/)).toBeTruthy();
    expect(screen.getByRole("button", { name: "이어서 진행" })).toBeTruthy();
    await userEvent.click(screen.getByRole("button", { name: "저장된 연결 취소" }));
    await ready("Google 연결");
    expect(google.connect).not.toHaveBeenCalled();
  });
});
