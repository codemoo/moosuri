import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";
import { MemberMailWriteConsent, isGoogleConsentUrl, parseWriteState } from "./MemberMailWriteConsent";

const google = "https://accounts.google.com/o/oauth2/v2/auth?client_id=synthetic&scope=x";
const needs = (reason: string, next = "start_write_consent") => ({ state: "needs_connection", reason, next });
const connected = { state: "connected", connection_id: "c1", epoch: 2, mailbox: "a@example.test" };
const props = (request: any, extra: object = {}) => ({ request, fail: (e: unknown) => { throw e; }, disabled: false, navigate: vi.fn(), ...extra });

it("readonly member sees an explicit opt-in, and consent navigates only to Google's consent page", async () => {
  const request = vi.fn(async (path: string) => path.endsWith("/status") ? needs("write_consent_required") : { state: "authorization", authorization_url: google, flow_id: "f".repeat(64) });
  const navigate = vi.fn(); const user = userEvent.setup();
  render(<MemberMailWriteConsent {...props(request, { navigate })} />);
  expect(await screen.findByText(/지금은 읽기 전용입니다/)).toBeInTheDocument();
  expect(request).toHaveBeenCalledTimes(1); // status only: opening the page never starts consent
  expect(navigate).not.toHaveBeenCalled();
  await user.click(screen.getByRole("button", { name: "Gmail 초안 저장·발송 권한 허용" }));
  expect(request.mock.calls.at(-1)).toEqual(["/personal/google/mail-write/start", "POST", {}]);
  expect(navigate).toHaveBeenCalledWith(google);
});

it("a forged or off-Google authorization URL is never followed", async () => {
  const request = vi.fn(async (path: string) => path.endsWith("/status") ? needs("write_consent_required") : { state: "authorization", authorization_url: "https://accounts.example.test/o/oauth2/v2/auth", flow_id: "f".repeat(64) });
  const navigate = vi.fn(); const failed = vi.fn(); const user = userEvent.setup();
  render(<MemberMailWriteConsent {...props(request, { navigate, fail: failed })} />);
  await user.click(await screen.findByRole("button", { name: "Gmail 초안 저장·발송 권한 허용" }));
  expect(navigate).not.toHaveBeenCalled();
  expect(failed).toHaveBeenCalledTimes(1);
  expect(isGoogleConsentUrl("http://accounts.google.com/o/oauth2/v2/auth")).toBe(false);
  expect(isGoogleConsentUrl("javascript:alert(1)")).toBe(false);
});

it("missing app setup is actionable without a consent button; a wrong mailbox asks to consent again", async () => {
  const setup = vi.fn(async () => needs("google_setup_required", "configure_google_app"));
  const first = render(<MemberMailWriteConsent {...props(setup)} />);
  expect(await screen.findByText(/Google 앱 설정이 아직 없어/)).toBeInTheDocument();
  expect(screen.queryByRole("button")).toBeNull();
  first.unmount();
  render(<MemberMailWriteConsent {...props(vi.fn(async () => needs("mailbox_mismatch")))} />);
  expect(await screen.findByText(/읽은 편지함과 다른 주소/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Gmail 초안 저장·발송 권한 허용" })).toBeInTheDocument();
});

it("the callback handle completes once, and revoking returns to needs_connection", async () => {
  let live = false;
  const request = vi.fn(async (path: string, _m?: string, body?: any) => {
    if (path.endsWith("/complete")) { live = true; expect(body).toEqual({ handle: "h".repeat(64) }); return connected; }
    if (path.endsWith("/disconnect")) { live = false; expect(body).toEqual({ connection_id: "c1", epoch: 2 }); return undefined; }
    return live ? connected : needs("connection_inactive");
  });
  const onChange = vi.fn(); const user = userEvent.setup();
  const { rerender } = render(<MemberMailWriteConsent {...props(request, { callbackHandle: "h".repeat(64), onChange })} />);
  expect(await screen.findByText(/쓰기 권한이 허용되었습니다: a@example.test/)).toBeInTheDocument();
  rerender(<MemberMailWriteConsent {...props(request, { callbackHandle: "h".repeat(64), onChange })} />);
  expect(request.mock.calls.filter(c => c[0].endsWith("/complete"))).toHaveLength(1);
  await user.click(screen.getByRole("button", { name: "쓰기 권한 해제" }));
  expect(await screen.findByText(/다시 연결이 필요합니다/)).toBeInTheDocument();
  expect(screen.queryByText(/쓰기 권한이 허용되었습니다/)).toBeNull();
  expect(onChange).toHaveBeenLastCalledWith(needs("connection_inactive"));
});

it("responses with credentials or unknown states are rejected", () => {
  expect(() => parseWriteState({ ...connected, access_token: "X" })).toThrow();
  expect(() => parseWriteState({ state: "delivered" })).toThrow();
  expect(() => parseWriteState(needs("write_consent_required", "send_now"))).toThrow();
  expect(parseWriteState(connected)).toEqual(connected);
});
