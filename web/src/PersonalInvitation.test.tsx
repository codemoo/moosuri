import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import PersonalInvitation, { takeInvitationToken } from "./PersonalInvitation";
const token = "a".repeat(43);
const pending = { name: "손님", email: "guest@example.test", ready: false, password_set: false };
afterEach(() => { cleanup(); vi.unstubAllGlobals(); sessionStorage.clear(); localStorage.clear(); });
it("removes the fragment before API calls and refuses malformed/absent links", () => {
  const replaceState = vi.fn();
  expect(takeInvitationToken({hash: "#" + token}, {replaceState})).toBe(token);
  expect(replaceState).toHaveBeenCalledWith(null, "", "/join");
  expect(takeInvitationToken({hash: "#invalid"}, {replaceState})).toBe("");
  const fetcher = vi.fn(); vi.stubGlobal("fetch", fetcher);
  render(<PersonalInvitation token=""/>);
  expect(screen.getByRole("alert")).toHaveTextContent("초대 링크를 다시");
  expect(fetcher).not.toHaveBeenCalled();
});
it("validates matching eight-character passwords and submits without account cookies or persistent secrets", async () => {
  const fetcher = vi.fn(async (url: string) => Response.json(url.endsWith("status") ? pending : {...pending, ready:true, password_set:true}));
  vi.stubGlobal("fetch", fetcher);
  const user = userEvent.setup(); render(<PersonalInvitation token={token}/>);
  const pass = await screen.findByLabelText("새 비밀번호");
  await user.type(pass, "short"); await user.type(screen.getByLabelText("비밀번호 확인"), "different");
  expect(screen.getByRole("button", {name:"가입 완료"})).toBeDisabled();
  await user.clear(pass); await user.type(pass, "chosen42");
  await user.clear(screen.getByLabelText("비밀번호 확인")); await user.type(screen.getByLabelText("비밀번호 확인"), "chosen42");
  await user.click(screen.getByRole("button", {name:"가입 완료"}));
  expect(await screen.findByRole("status")).toHaveTextContent("비밀번호를 설정했습니다");
  const [, options] = fetcher.mock.calls[1] as unknown as [string, RequestInit];
  expect(options.credentials).toBe("omit"); expect(options.referrerPolicy).toBe("no-referrer");
  expect(JSON.parse(String(options.body))).toEqual({token, password:"chosen42"});
  expect(sessionStorage.length + localStorage.length).toBe(0);
  expect(screen.queryByLabelText("새 비밀번호")).not.toBeInTheDocument();
});
it("recovers a lost completion reply by checking status without resubmitting a password", async () => {
  let completed = false;
  const fetcher = vi.fn(async (url: string) => {
    if (url.endsWith("redeem")) { completed = true; throw new TypeError("lost"); }
    return Response.json({...pending, ready:completed});
  }); vi.stubGlobal("fetch", fetcher);
  const user = userEvent.setup(); render(<PersonalInvitation token={token}/>);
  await user.type(await screen.findByLabelText("새 비밀번호"), "chosen42");
  await user.type(screen.getByLabelText("비밀번호 확인"), "chosen42");
  await user.click(screen.getByRole("button", {name:"가입 완료"}));
  await screen.findByRole("alert");
  expect(screen.getByLabelText("새 비밀번호")).toHaveValue("");
  await user.click(screen.getByRole("button", {name:"상태 다시 확인"}));
  expect(await screen.findByRole("status")).toHaveTextContent("이미 설정한 비밀번호");
  expect(fetcher.mock.calls.filter(([u]) => u.endsWith("redeem"))).toHaveLength(1);
});
it.each([401,403,404])("explains expired/revoked link %s without displaying an account or password form", async status => {
  vi.stubGlobal("fetch", vi.fn(async () => new Response("", {status})));
  render(<PersonalInvitation token={token}/>);
  expect(await screen.findByRole("alert")).toHaveTextContent("새 링크");
  expect(screen.queryByLabelText("새 비밀번호")).not.toBeInTheDocument();
});
it("reports preparation/capacity wait and clears a submitted password for an explicit retry", async () => {
  vi.stubGlobal("fetch", vi.fn(async (url: string) => url.endsWith("status") ? Response.json(pending) : new Response("", {status:409})));
  const user = userEvent.setup(); render(<PersonalInvitation token={token}/>);
  await user.type(await screen.findByLabelText("새 비밀번호"), "chosen42");
  await user.type(screen.getByLabelText("비밀번호 확인"), "chosen42");
  await user.click(screen.getByRole("button", {name:"가입 완료"}));
  expect(await screen.findByRole("alert")).toHaveTextContent("공간을 준비");
  expect(screen.getByLabelText("새 비밀번호")).toHaveValue("");
  await waitFor(() => expect(screen.getByRole("button", {name:"상태 다시 확인"})).toBeEnabled());
});
