import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { MailConnectionRecovery } from "./MailConnectionRecovery";
import type { MailConnectionRecovery as Recovery } from "../shared/api-types";
import { clearMailReturn, mailReturnThread } from "./mail-return";
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
it("uses only fixed server recovery actions and separates installer setup from owner consent", () => {
  const view = render(<MailConnectionRecovery action="admin_setup" />);
  expect(screen.queryByText(/기기 Gmail/)).toBeNull();
  expect(screen.getByRole("link", {name: "Google 설치 설정 열기 ↗"})).toHaveAttribute("href", "/admin/google");
  view.rerender(<MailConnectionRecovery action="connect" />);
  expect(screen.getByRole("link", {name: "내 Gmail 읽기 연결하기 ↗"})).toHaveAttribute("href", "/settings/mail");
  view.rerender(<MailConnectionRecovery action="member_setup" />);
  expect(screen.queryByRole("link", {name: "Google 설치 설정 열기 ↗"})).toBeNull();
  expect(screen.getByRole("link", {name: "Gmail 연결 안내 열기 ↗"})).toHaveAttribute("href", "/settings/mail");
  view.rerender(<MailConnectionRecovery action={"https://untrusted.example" as Recovery} />);
  expect(screen.queryByRole("link")).toBeNull();
  view.rerender(<MailConnectionRecovery action={"__proto__" as Recovery} />);
  expect(screen.queryByRole("link")).toBeNull();
  view.rerender(<MailConnectionRecovery />);
  expect(screen.queryByRole("link")).toBeNull();
});
it("keeps the original chat as the return target when opening guided setup", () => {
  vi.stubGlobal("fetch", vi.fn(async () => Response.json({configured:false})));
  const thread = "11111111-1111-4111-8111-111111111111";
  render(<MailConnectionRecovery action="connect" accountId="owner" threadId={thread} />);
  const link = screen.getByRole("link", {name: "내 Gmail 읽기 연결하기 ↗"});
  link.addEventListener("click", event => event.preventDefault());
  fireEvent.click(link);
  expect(link).not.toHaveAttribute("target", "_blank");
  expect(mailReturnThread("owner")).toBe(thread);
  expect(mailReturnThread("other-owner")).toBeNull();
  clearMailReturn("owner");
});
it("starts account-scoped web Google login directly and clears failed continuations", async () => {
  const calls: any[] = [];
  vi.stubGlobal("fetch", vi.fn(async (url:string, init:RequestInit={}) => {
    calls.push({url,body:JSON.parse(String(init.body)),account:new Headers(init.headers).get("X-Moosuri-Account")});
    return Response.json({authorization_url:"https://untrusted.example/sign-in"});
  }));
  render(<MailConnectionRecovery action="connect" accountId="owner" threadId="11111111-1111-4111-8111-111111111111" runId="22222222-2222-4222-8222-222222222222" canResume />);
  fireEvent.click(screen.getByRole("button", {name:"Google 로그인하고 요청 이어가기"}));
  await screen.findByRole("alert");
  expect(calls).toEqual([{url:"/api/v1/mail/oauth/start",body:{},account:"owner"}]);
  expect(sessionStorage.getItem("moosuri.google-mail-resume.v1.owner")).toBeNull();
  expect(screen.queryByText(/Mac|GWS|기기 연결/)).toBeNull();
});
