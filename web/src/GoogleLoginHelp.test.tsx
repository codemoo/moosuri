import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import { GoogleLoginHelp } from "./GoogleLoginHelp";
import { clearMailReturn, mailReturnThread, rememberMailReturn } from "./mail-return";
afterEach(() => { cleanup(); vi.unstubAllGlobals(); vi.restoreAllMocks(); sessionStorage.clear(); });

it("keeps vault storage optional and gives usable first-run instructions without accepting a password", async () => {
  const fetch = vi.fn(async (_path: string) => Response.json({ available: false }));
  vi.stubGlobal("fetch", fetch);
  render(<GoogleLoginHelp accountId="owner" />);
  expect(screen.queryByRole("textbox")).toBeNull();
  expect(screen.queryByLabelText(/비밀번호/)).toBeNull();
  expect(fetch).not.toHaveBeenCalled();
  await userEvent.click(screen.getByRole("button", { name: "ID·비밀번호를 내 볼트에 보관" }));
  await screen.findByText(/이 설치에서 별도 볼트를 열 수 없습니다/);
  expect(screen.getByText(/복구 키를 보관·확인/)).toBeInTheDocument();
  expect(screen.getByText(/저장만으로 Gmail이 연결되지는 않습니다/)).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /볼트 열기/ })).toBeNull();
  expect(fetch).toHaveBeenCalledTimes(1);
  expect(fetch.mock.calls[0]?.[0]).toBe("/api/v1/vault");
});

it("uses existing secure POST handoff, keeps the original chat open, and never imports credentials", async () => {
  const calls: {path: string; body?: BodyInit | null}[] = [];
  vi.stubGlobal("fetch", vi.fn(async (path: string, init: RequestInit = {}) => {
    calls.push({path, body: init.body});
    return Response.json(path.endsWith("/handoff")
      ? { token: "a".repeat(64), vault_origin: "https://vault.example.test" }
      : { available: true });
  }));
  const popup = { closed: false, close: vi.fn() };
  vi.spyOn(window, "open").mockReturnValue(popup as unknown as Window);
  const submitted: { action: string; method: string; value: string; name: string; target: string }[] = [];
  vi.spyOn(HTMLFormElement.prototype, "submit").mockImplementation(function(this: HTMLFormElement) {
    const input = this.querySelector("input")!;
    submitted.push({action: this.action, method: this.method, value: input.value, name: input.name, target: this.target});
  });
  render(<GoogleLoginHelp accountId="owner" />);
  await userEvent.click(screen.getByRole("button", { name: "저장된 Google 로그인 사용" }));
  await userEvent.click(await screen.findByRole("button", { name: "내 Google 로그인 볼트 열기 ↗" }));
  await waitFor(() => expect(submitted).toHaveLength(1));
  expect(submitted[0]).toMatchObject({ action: "https://vault.example.test/session", method: "post", value: "a".repeat(64), name: "handoff" });
  expect(submitted[0].target).toMatch(/^moosuri-vault-/);
  expect(window.open).toHaveBeenCalledWith("about:blank", submitted[0].target);
  expect(calls.map(c => c.path)).toEqual(["/api/v1/vault", "/api/v1/vault/handoff"]);
  expect(calls[1].body).toBe("{}");
  expect(document.querySelector("form")).toBeNull();
  expect(popup.close).not.toHaveBeenCalled();
});

it("does not issue handoff when the PWA/browser blocks the new vault window", async () => {
  const fetch = vi.fn(async () => Response.json({available: true}));
  vi.stubGlobal("fetch", fetch);
  vi.spyOn(window, "open").mockReturnValue(null);
  render(<GoogleLoginHelp accountId="owner" />);
  await userEvent.click(screen.getByRole("button", {name: "저장된 Google 로그인 사용"}));
  await userEvent.click(await screen.findByRole("button", {name: "내 Google 로그인 볼트 열기 ↗"}));
  await screen.findByText(/새 창을 허용한 뒤/);
  expect(fetch).toHaveBeenCalledTimes(1);
});

it("retains only a bounded account-owned return thread and rejects invalid or expired targets", () => {
  const id = "11111111-1111-4111-8111-111111111111";
  rememberMailReturn("owner", id);
  expect(mailReturnThread("owner")).toBe(id);
  expect(mailReturnThread("another-owner")).toBeNull();
  rememberMailReturn("another-owner", "https://untrusted.example");
  expect(mailReturnThread("another-owner")).toBeNull();
  const value = sessionStorage.getItem("moosuri.gmail-return.v1.owner")!;
  expect(Object.keys(JSON.parse(value)).sort()).toEqual(["at", "threadId"]);
  vi.spyOn(Date, "now").mockReturnValue(Date.now() + 3600001);
  expect(mailReturnThread("owner")).toBeNull();
  clearMailReturn("owner");
});
