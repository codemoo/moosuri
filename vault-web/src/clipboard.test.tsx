import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import type { Root } from "react-dom/client";
import { afterEach, beforeAll, beforeEach, expect, it, vi } from "vitest";
import type { Command, Summary } from "./types";

let application: ReactNode;
vi.mock("react-dom/client", async importOriginal => {
  const actual = await importOriginal<typeof import("react-dom/client")>();
  return { ...actual, createRoot: (container: Element) => {
    if (container.id !== "root") return actual.createRoot(container);
    return { render: (node: ReactNode) => { application = node; } } as Root;
  }};
});
beforeAll(async () => {
  const entry = document.createElement("div");
  entry.id = "root";
  document.body.append(entry);
  await import("./main");
  entry.remove();
});

const item: Summary = { id: "sample", kind: "login", title: "Example login", revision: 1,
  favorite: false, expires_at: "", has_totp: true };
const route = vi.fn<(command: Command) => unknown>();
const writeText = vi.fn(async (_text: string) => {});
class WorkerFixture {
  onmessage?: (response: {data: unknown}) => void;
  terminate() {}
  async postMessage({id, epoch, command}: {id: number; epoch: number; command: Command}) {
    if (command.op === "close") return;
    const value = await route(command);
    this.onmessage?.({data: {id, epoch, ok: true, value}});
  }
}
function otp(code: string, valid_until: number) { return {code, valid_from: valid_until - 30, valid_until}; }
beforeEach(() => {
  route.mockReset().mockImplementation(command => {
    switch (command.op) {
      case "initialize": return {initialized: true, pending: []};
      case "unlock": return {};
      case "list": return [item];
      case "view_details": return {username: "example", host: "example.test", has_url: true};
      case "totp": return otp("123456", Math.floor(Date.now() / 1000) + 30);
      default: throw new Error("unexpected command " + command.op);
    }
  });
  writeText.mockClear();
  vi.stubGlobal("Worker", WorkerFixture);
  vi.stubGlobal("scrollTo", vi.fn());
  vi.stubGlobal("fetch", vi.fn(async (url: string) => {
    if (url === "/public/config") return Response.json({assistant_origin: "https://assistant.example.test"});
    if (url === "/session/current") return Response.json({owner: {account: "sample"}});
    throw new Error("unexpected request");
  }));
  Object.defineProperty(navigator, "clipboard", {configurable: true, value: {writeText}});
});
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });
async function openItem() {
  render(application);
  fireEvent.change(await screen.findByLabelText("볼트 비밀번호"), {target: {value: "synthetic"}});
  fireEvent.click(screen.getByRole("button", {name: "열기"}));
  fireEvent.click(await screen.findByRole("button", {name: item.title}));
  await screen.findByText("123456");
}
it("locks before a pending password copy returns and discards its plaintext", async () => {
  await openItem();
  let release!: (value: string) => void;
  route.mockImplementationOnce(() => new Promise<string>(resolve => { release = resolve; }));
  fireEvent.click(screen.getByRole("button", {name: "비밀번호 복사"}));
  await waitFor(() => expect(route).toHaveBeenCalledWith({op: "reveal", item: item.id, field: "secret"}));
  fireEvent.click(screen.getByRole("button", {name: "잠그기"}));
  await act(async () => release("SYNTHETIC LATE VALUE"));
  expect(screen.getByLabelText("볼트 비밀번호")).toBeTruthy();
  expect(screen.queryByText("SYNTHETIC LATE VALUE")).toBeNull();
  expect(writeText).not.toHaveBeenCalled();
});

it("recomputes an expired worker reply before writing the current code to the clipboard", async () => {
  await openItem();
  const boundary = Math.floor(Date.now() / 1000) + 2;
  vi.spyOn(Date, "now").mockReturnValue(boundary * 1000 - 1);
  let release!: (value: ReturnType<typeof otp>) => void;
  route.mockImplementationOnce(() => new Promise<ReturnType<typeof otp>>(resolve => { release = resolve; }));
  fireEvent.click(screen.getByRole("button", {name: "현재 코드 복사"}));
  await waitFor(() => expect(release).toBeTypeOf("function"));
  vi.mocked(Date.now).mockReturnValue(boundary * 1000);
  route.mockReturnValueOnce(otp("654321", boundary + 30));
  await act(async () => release(otp("123456", boundary)));
  expect(writeText.mock.calls).toEqual([["654321"]]);
  expect(screen.getByRole("status").textContent).toContain("현재 인증 코드를 복사했습니다");
});

it("leaves the clipboard untouched when both code requests arrive expired", async () => {
  await openItem();
  const expired = otp("123456", Math.floor(Date.now() / 1000));
  route.mockReturnValueOnce(expired).mockReturnValueOnce(expired);
  fireEvent.click(screen.getByRole("button", {name: "현재 코드 복사"}));
  expect(await screen.findByText("코드가 갱신되었습니다. 다시 복사해 주세요.")).toBeTruthy();
  expect(writeText).not.toHaveBeenCalled();
});


it.each([31, -31])("hides the previous code across a %s-second clock change while a new code is pending", async delta => {
  const initial = Math.floor(Date.now() / 1000) * 1000;
  const clock = vi.spyOn(Date, "now").mockReturnValue(initial);
  await openItem();
  let release!: (value: ReturnType<typeof otp>) => void;
  const regular = route.getMockImplementation()!;
  route.mockImplementation(command => command.op === "totp"
    ? new Promise<ReturnType<typeof otp>>(resolve => { release = resolve; })
    : regular(command));
  const changed = initial + delta * 1000;
  clock.mockReturnValue(changed);
  fireEvent.focus(window);
  await waitFor(() => expect(release).toBeTypeOf("function"));
  expect(screen.queryByText("123456")).toBeNull();
  expect(screen.queryByRole("button", {name: "현재 코드 복사"})).toBeNull();
  await act(async () => release(otp("678901", changed / 1000 + 30)));
  expect(await screen.findByText("678901")).toBeTruthy();
  expect(writeText).not.toHaveBeenCalled();
});

it("rejects a future code on both clipboard attempts when the clock moves backward", async () => {
  await openItem();
  const future = otp("987654", Math.floor(Date.now() / 1000) + 90);
  route.mockReturnValueOnce(future).mockReturnValueOnce(future);
  fireEvent.click(screen.getByRole("button", {name: "현재 코드 복사"}));
  expect(await screen.findByText("코드가 갱신되었습니다. 다시 복사해 주세요.")).toBeTruthy();
  expect(writeText).not.toHaveBeenCalled();
});
