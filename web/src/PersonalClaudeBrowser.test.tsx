import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import { claudeOrigin, PersonalClaudeBrowser, validClaudeFrame } from "./PersonalClaudeBrowser";

afterEach(() => vi.unstubAllGlobals());
const id = "11111111-1111-4111-8111-111111111111";
const live = { request_id: id, state: "awaiting_code", browser_available: true };
const base = { png_base64: "iVBORw0KGgo=", width: 400, height: 800, url: "https://claude.ai/login?next=synthetic-secret#frag" };
type Body = Record<string, unknown>;

/** Serves frames with a fresh token each time and records every request body. */
function serve(input: (body: Body, calls: Body[]) => Response = () => Response.json({ operation: live })) {
  const calls: Body[] = [];
  let n = 0;
  vi.stubGlobal("fetch", vi.fn(async (_url: string, init: RequestInit) => {
    const body = JSON.parse(String(init.body)) as Body;
    calls.push(body);
    if (body.op === "frame") return Response.json({ operation: live, frame: { ...base, frame_token: `token-${++n}` } });
    return input(body, calls);
  }));
  return calls;
}
function mount(extra: Partial<Parameters<typeof PersonalClaudeBrowser>[0]> = {}) {
  const props = { account: "synthetic-admin", requestId: id, onOperation: vi.fn(), onFailure: vi.fn(() => false), onRefresh: vi.fn(), frameMs: 10, ...extra };
  return { props, ...render(<PersonalClaudeBrowser {...props} />) };
}
const screenImage = () => screen.findByAltText("Claude 로그인 화면");
const inputs = (calls: Body[]) => calls.filter(call => call.op === "input");
const sleep = (ms: number) => new Promise(resolve => setTimeout(resolve, ms));

it("keeps refreshing the frame by itself and shows only a sanitized origin", async () => {
  const calls = serve();
  mount();
  await screenImage();
  await waitFor(() => expect(calls.filter(call => call.op === "frame").length).toBeGreaterThan(2));
  expect(screen.getByLabelText("현재 페이지 출처")).toHaveTextContent(/^https:\/\/claude\.ai$/);
  expect(document.body.innerHTML).not.toContain("synthetic-secret");
  expect(calls.every(call => call.request_id === id)).toBe(true);
});

it("stops requesting and removes pixels and typed text when it unmounts", async () => {
  const calls = serve();
  const { unmount } = mount();
  await screenImage();
  await userEvent.setup().type(screen.getByLabelText("선택한 칸에 입력"), "synthetic-secret-text");
  unmount();
  expect(screen.queryByAltText("Claude 로그인 화면")).not.toBeInTheDocument();
  await sleep(20);
  const seen = calls.length;
  await sleep(60);
  expect(calls.length).toBe(seen);
  expect(document.body.innerHTML).not.toContain("synthetic-secret-text");
});

it("maps a click on a scaled mobile screen to frame pixels", async () => {
  const calls = serve();
  mount();
  const image = await screenImage();
  // Displayed at half width, offset in the page: 200x400 css px for a 400x800 frame.
  image.getBoundingClientRect = () => ({ left: 10, top: 20, width: 200, height: 400, right: 210, bottom: 420, x: 10, y: 20, toJSON() {} });
  fireEvent.click(image, { clientX: 110, clientY: 220 });
  await waitFor(() => expect(inputs(calls)).toHaveLength(1));
  expect(inputs(calls)[0]).toMatchObject({ op: "input", request_id: id, input: { kind: "click", x: 200, y: 400 } });
  expect(String(inputs(calls)[0].frame_token)).toMatch(/^token-\d+$/);
});

it("sends only plain mapped keys from the focused screen", async () => {
  const calls = serve();
  mount();
  const image = await screenImage();
  fireEvent.keyDown(image, { key: "a" });
  fireEvent.keyDown(image, { key: "Tab", shiftKey: true });
  fireEvent.keyDown(image, { key: "Enter", ctrlKey: true });
  await sleep(30);
  expect(inputs(calls)).toHaveLength(0);
  fireEvent.keyDown(image, { key: "ArrowDown" });
  await waitFor(() => expect(inputs(calls)).toHaveLength(1));
  expect(inputs(calls)[0].input).toEqual({ kind: "key", key: "arrow_down" });
});

it("sends masked text once, clears the field at submit and never stores it", async () => {
  const calls = serve();
  mount();
  await screenImage();
  const user = userEvent.setup();
  const field = screen.getByLabelText("선택한 칸에 입력");
  expect(field).toHaveAttribute("type", "password");
  await user.type(field, "한글 secret-값");
  await user.click(screen.getByRole("button", { name: "입력 보내기" }));
  expect(field).toHaveValue("");
  await waitFor(() => expect(inputs(calls)).toHaveLength(1));
  expect(inputs(calls)[0].input).toEqual({ kind: "insert_text", text: "한글 secret-값" });
  expect(JSON.stringify(localStorage) + JSON.stringify(sessionStorage) + document.body.innerHTML).not.toContain("secret-값");
});

it("on a stale-frame refusal shows the current screen and never replays the input", async () => {
  const calls = serve(() => new Response(null, { status: 409 }));
  mount();
  const image = await screenImage();
  image.getBoundingClientRect = () => ({ left: 0, top: 0, width: 400, height: 800, right: 400, bottom: 800, x: 0, y: 0, toJSON() {} });
  fireEvent.click(image, { clientX: 5, clientY: 5 });
  expect(await screen.findByRole("alert")).toHaveTextContent("화면이 바뀌었습니다");
  const first = String(inputs(calls)[0].frame_token);
  await sleep(80); // many refreshes later
  expect(inputs(calls)).toHaveLength(1);
  // A deliberate new action uses the newer frame, not the refused one.
  fireEvent.click(image, { clientX: 6, clientY: 6 });
  await waitFor(() => expect(inputs(calls)).toHaveLength(2));
  expect(inputs(calls)[1].frame_token).not.toBe(first);
});

it("does not resend an input whose reply was lost and tells the user it is uncertain", async () => {
  const calls = serve(() => { throw new TypeError("synthetic response lost"); });
  mount();
  await screenImage();
  fireEvent.keyDown(screen.getByAltText("Claude 로그인 화면"), { key: "Enter" });
  expect(await screen.findByRole("alert")).toHaveTextContent("자동으로 다시 보내지 않습니다");
  await sleep(80);
  expect(inputs(calls)).toHaveLength(1);
});

it("ignores a second action while the first is in flight", async () => {
  let release = () => {};
  const calls = serve(() => { throw new Error("unused"); });
  vi.stubGlobal("fetch", vi.fn(async (_url: string, init: RequestInit) => {
    const body = JSON.parse(String(init.body)) as Body;
    calls.push(body);
    if (body.op === "frame") return Response.json({ operation: live, frame: { ...base, frame_token: "token-x" } });
    await new Promise<void>(resolve => { release = resolve; });
    return Response.json({ operation: live });
  }));
  mount();
  const image = await screenImage();
  fireEvent.keyDown(image, { key: "Enter" });
  await waitFor(() => expect(inputs(calls)).toHaveLength(1));
  fireEvent.keyDown(image, { key: "Tab" });
  fireEvent.keyDown(image, { key: "Enter" });
  release();
  await sleep(30);
  expect(inputs(calls)).toHaveLength(1);
  // A delayed server frame bearing the consumed proof cannot authorize a second action.
  fireEvent.keyDown(image, { key: "Enter" });
  await sleep(30);
  expect(inputs(calls)).toHaveLength(1);
});

it("removes the screen and reports the new state once the operation leaves awaiting_code", async () => {
  let over = false;
  let n = 0;
  vi.stubGlobal("fetch", vi.fn(async () => over
    ? Response.json({ operation: { request_id: id, state: "verifying" } })
    : (n++, Response.json({ operation: live, frame: { ...base, frame_token: `token-${n}` } }))));
  const { props } = mount();
  await screenImage();
  over = true;
  await waitFor(() => expect(screen.queryByAltText("Claude 로그인 화면")).not.toBeInTheDocument());
  expect(props.onOperation).toHaveBeenLastCalledWith({ request_id: id, state: "verifying" });
  expect(screen.queryByLabelText("선택한 칸에 입력")).toBeDisabled();
});

it("hands authority loss to the owner flow, drops pixels and stops polling", async () => {
  let revoked = false;
  const calls: Body[] = [];
  vi.stubGlobal("fetch", vi.fn(async (_url: string, init: RequestInit) => {
    calls.push(JSON.parse(String(init.body)) as Body);
    return revoked ? new Response(null, { status: 401 }) : Response.json({ operation: live, frame: { ...base, frame_token: "token-1" } });
  }));
  const { props } = mount({ onFailure: vi.fn(() => true) });
  await screenImage();
  revoked = true;
  await waitFor(() => expect(props.onFailure).toHaveBeenCalled());
  await sleep(30);
  const seen = calls.length;
  await sleep(60);
  expect(calls.length).toBe(seen);
  expect(screen.queryByAltText("Claude 로그인 화면")).not.toBeInTheDocument();
});

it("rejects malformed frames and non-https origins", () => {
  expect(validClaudeFrame({ ...base, frame_token: "t" })).toBe(true);
  expect(validClaudeFrame({ ...base, frame_token: "" })).toBe(false);
  expect(validClaudeFrame({ ...base, frame_token: "t", png_base64: "<script>" })).toBe(false);
  expect(validClaudeFrame({ ...base, frame_token: "t", width: 0 })).toBe(false);
  expect(claudeOrigin("https://claude.ai/a?b=c#d")).toBe("https://claude.ai");
  expect(claudeOrigin("http://claude.ai")).toBe("");
  expect(claudeOrigin("https://user:pw@claude.ai/")).toBe("");
  expect(claudeOrigin("javascript:alert(1)")).toBe("");
});
