import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import { GwsInboxHelp } from "./GwsInboxHelp";
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
it("distinguishes configured GWS from a successful read and explains the bounded device setup", async () => {
  const requests: string[] = [];
  let configured = false;
  vi.stubGlobal("fetch", vi.fn(async (path: string) => {
    requests.push(path);
    return Response.json({configured, mailbox: configured ? "synthetic@example.test" : undefined});
  }));
  render(<GwsInboxHelp accountId="owner" />);
  await screen.findByText(/아직 이 사용자의 기기 Gmail 연결이 설정되지 않았습니다/);
  await userEvent.click(screen.getByText("Mac에서 GWS 연결 준비하기"));
  expect(screen.getByText("gws auth login --readonly --services gmail")).toBeInTheDocument();
  expect(screen.getByText(/Google 로그인만으로 무수리 연결이 완료되지는 않습니다/)).toBeInTheDocument();
  configured = true;
  await userEvent.click(screen.getByRole("button", {name: "기기 연결 설정 새로고침"}));
  await screen.findByText(/운영자 연결 설정 있음 · synthetic@example.test/);
  expect(screen.getByText(/실제 메일 조회 시 기기 연결과 Google 인증을 확인/)).toBeInTheDocument();
  expect(requests).toEqual(["/api/v1/mail/gws-inbox", "/api/v1/mail/gws-inbox"]);
  expect(screen.queryByRole("textbox")).toBeNull();
});
it("reports an offline bridge honestly while leaving the device instructions usable", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => { throw new TypeError("offline fixture"); }));
  render(<GwsInboxHelp accountId="owner" />);
  await screen.findByText(/기기 연결 설정을 확인하지 못했습니다/);
  expect(screen.getByRole("button", {name: "기기 연결 설정 새로고침"})).toBeEnabled();
  expect(screen.queryByText(/운영자 연결 설정 있음/)).toBeNull();
});
