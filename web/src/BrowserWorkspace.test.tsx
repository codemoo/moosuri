import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { BrowserWorkspace } from "./BrowserWorkspace";
import type { BrowserWorkspaceView } from "../shared/api-types";

const ready: BrowserWorkspaceView = {
  id: "workspace-a",
  thread_id: "thread-a",
  state: "ready",
  revision: 1,
  human_touched: false,
  url: "https://example.test/",
  title: "Synthetic page",
  reason: null,
  expires_at: 99999,
};
const picture = (revision = 1, png = "YWJj") => ({
  workspace_id: "workspace-a",
  revision,
  frame_token: `frame-${revision}`,
  png_base64: png,
  width: 1000,
  height: 500,
  url: "https://example.test/",
});
function json(value: unknown, status = 200) {
  return new Response(JSON.stringify(value), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
type Call = {
  url: string;
  method: string;
  headers: Record<string, string>;
  body: Record<string, unknown>;
};
let calls: Call[];
let workspace: BrowserWorkspaceView | null;
let configured: boolean;
let custom:
  ((call: Call) => Response | Promise<Response> | undefined) | undefined;
beforeEach(() => {
  calls = [];
  workspace = { ...ready };
  configured = true;
  custom = undefined;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init: RequestInit) => {
      const call = {
        url,
        method: init.method ?? "GET",
        headers: init.headers as Record<string, string>,
        body: init.body ? JSON.parse(init.body as string) : {},
      };
      calls.push(call);
      const override = custom?.(call);
      if (override) return override;
      if (call.method === "GET") return json({ configured, workspace });
      if (url.endsWith("/takeover")) {
        workspace = {
          ...workspace!,
          revision: workspace!.revision + 1,
          state: "user_control",
          human_touched: true,
        };
        return json({ workspace, control_token: "synthetic-control" });
      }
      if (url.endsWith("/frame")) return json(picture(workspace!.revision));
      if (url.endsWith("/close")) {
        workspace = { ...workspace!, state: "closed" };
        return new Response(null, { status: 204 });
      }
      if (url.endsWith("/resume")) {
        workspace = { ...workspace!, state: "ready" };
        return json({ run: { id: "synthetic-turn" } });
      }
      if (url.endsWith("/browser") && call.method === "POST") {
        workspace = { ...ready, url: call.body.url as string };
        return json(workspace);
      }
      return new Response(null, { status: 204 });
    }),
  );
});
afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});
const props = () => ({
  accountId: "account-a",
  threadId: "thread-a",
  onExpired: vi.fn(),
  onChanged: vi.fn(),
});
async function open() {
  fireEvent.click(await screen.findByRole("button", { name: /화면 보기/ }));
  await screen.findByRole("img");
}
async function take() {
  fireEvent.click(screen.getByRole("button", { name: "직접 조작하기" }));
  await screen.findByRole("button", { name: /조작 완료/ });
  await screen.findByRole("img");
}
async function tick(ms = 2000) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

it("hides unconfigured workspaces and scopes every private request to the account", async () => {
  configured = false;
  render(<BrowserWorkspace {...props()} />);
  await waitFor(() => expect(calls).toHaveLength(1));
  expect(screen.queryByText("브라우저 작업")).toBeNull();
  expect(calls[0].headers["X-Moosuri-Account"]).toBe("account-a");
  expect(calls[0].url).toBe("/api/v1/threads/thread-a/browser");
});

it("creates a workspace from a collapsed card without requiring a model", async () => {
  workspace = null;
  render(<BrowserWorkspace {...props()} />);
  fireEvent.click(await screen.findByRole("button", { name: "브라우저 열기" }));
  fireEvent.change(screen.getByLabelText("열 주소"), {
    target: { value: "https://example.test/new" },
  });
  fireEvent.click(screen.getByRole("button", { name: "새 브라우저 열기" }));
  await screen.findByRole("img");
  expect(
    calls.find(
      (call) => call.method === "POST" && call.url.endsWith("/browser"),
    )?.body,
  ).toEqual({ url: "https://example.test/new" });
});

it("fetches frames through account scope, maps scaled clicks, clears secret input and resumes exactly once", async () => {
  const p = props();
  render(<BrowserWorkspace {...p} />);
  await open();
  expect(screen.getByRole("img")).toHaveAttribute(
    "src",
    "data:image/png;base64,YWJj",
  );
  expect(calls.filter((call) => call.url.endsWith("/frame"))[0].body).toEqual({
    workspace_id: "workspace-a",
    revision: 1,
  });
  await take();
  const image = screen.getByRole("img");
  vi.spyOn(image, "getBoundingClientRect").mockReturnValue({
    x: 10,
    y: 20,
    left: 10,
    top: 20,
    right: 510,
    bottom: 270,
    width: 500,
    height: 250,
    toJSON: () => ({}),
  });
  fireEvent.click(image, { clientX: 260, clientY: 145 });
  await waitFor(() =>
    expect(calls.some((call) => call.url.endsWith("/input"))).toBe(true),
  );
  expect(calls.find((call) => call.url.endsWith("/input"))?.body).toEqual({
    workspace_id: "workspace-a",
    revision: 2,
    control_token: "synthetic-control",
    frame_token: "frame-2",
    input: { kind: "click", x: 500, y: 250 },
  });
  const text = await screen.findByLabelText("선택한 칸에 입력");
  expect(text).toHaveAttribute("type", "password");
  fireEvent.change(text, { target: { value: "synthetic-password" } });
  fireEvent.click(screen.getByRole("button", { name: "입력" }));
  expect(text).toHaveValue("");
  await waitFor(() =>
    expect(calls.filter((call) => call.url.endsWith("/input"))).toHaveLength(2),
  );
  await screen.findByRole("button", { name: /조작 완료/ });
  fireEvent.click(screen.getByRole("button", { name: /조작 완료/ }));
  await waitFor(() => expect(p.onChanged).toHaveBeenCalledTimes(1));
  expect(calls.filter((call) => call.url.endsWith("/resume"))).toHaveLength(1);
  expect(
    calls.every((call) => call.headers["X-Moosuri-Account"] === "account-a"),
  ).toBe(true);
  expect(localStorage.length).toBe(0);
});

it("ignores late watch pixels after takeover and never replays uncertain human input", async () => {
  const old = deferred<Response>();
  let frames = 0;
  custom = (call) => {
    if (call.url.endsWith("/frame") && ++frames === 1) return old.promise;
    if (call.url.endsWith("/input"))
      return Promise.reject(new Error("network lost"));
  };
  render(<BrowserWorkspace {...props()} />);
  fireEvent.click(await screen.findByRole("button", { name: /화면 보기/ }));
  await waitFor(() => expect(frames).toBe(1));
  await take();
  await act(async () => old.resolve(json(picture(1, "b2xk"))));
  expect(screen.getByRole("img")).toHaveAttribute(
    "src",
    "data:image/png;base64,YWJj",
  );
  screen.getByRole("img").focus();
  fireEvent.keyDown(screen.getByRole("img"), { key: "Enter" });
  await screen.findByRole("alert");
  expect(screen.getByRole("alert").textContent).toContain(
    "자동으로 다시 실행하지 않습니다",
  );
  await waitFor(() =>
    expect(screen.getByRole("button", { name: "직접 조작하기" })).toBeEnabled(),
  );
  expect(calls.filter((call) => call.url.endsWith("/input"))).toHaveLength(1);
});

it("revokes local control when another tab takes over and pauses polling while hidden", async () => {
  vi.useFakeTimers();
  custom = (call) =>
    call.url.endsWith("/frame") &&
    workspace?.state === "user_control" &&
    !call.body.control_token
      ? json({ code: "conflict" }, 409)
      : undefined;
  render(<BrowserWorkspace {...props()} />);
  await tick(0);
  fireEvent.click(screen.getByRole("button", { name: /화면 보기/ }));
  await tick(0);
  fireEvent.click(screen.getByRole("button", { name: "직접 조작하기" }));
  await tick(0);
  expect(screen.getByRole("button", { name: /조작 완료/ })).toBeEnabled();
  workspace = { ...workspace!, revision: 3 };
  await tick();
  expect(screen.queryByRole("button", { name: /조작 완료/ })).toBeNull();
  expect(screen.queryByRole("img")).toBeNull();
  const before = calls.length;
  const visibility = vi
    .spyOn(document, "visibilityState", "get")
    .mockReturnValue("hidden");
  fireEvent(document, new Event("visibilitychange"));
  await tick(6000);
  expect(calls).toHaveLength(before);
  visibility.mockReturnValue("visible");
  fireEvent(document, new Event("visibilitychange"));
  await tick(0);
  expect(calls.length).toBeGreaterThan(before);
  expect(calls.some((call) => call.url.endsWith("/resume"))).toBe(false);
});

it("purges pixels and pending responses on account/thread changes and auth rejection", async () => {
  const p = props();
  const view = render(<BrowserWorkspace {...p} />);
  await open();
  await take();
  const pending = deferred<Response>();
  custom = (call) => (call.method === "GET" ? pending.promise : undefined);
  view.rerender(
    <BrowserWorkspace {...p} accountId="account-b" threadId="thread-b" />,
  );
  expect(screen.queryByRole("img")).toBeNull();
  expect(screen.queryByLabelText("선택한 칸에 입력")).toBeNull();
  await act(async () => pending.resolve(json({ code: "unauthorized" }, 401)));
  await waitFor(() => expect(p.onExpired).toHaveBeenCalledOnce());
  expect(screen.queryByRole("img")).toBeNull();
  expect(calls.at(-1)?.headers["X-Moosuri-Account"]).toBe("account-b");
  expect(calls.at(-1)?.url).toContain("/thread-b/");
});

it("stops a pending frame from reappearing after folding or changing thread", async () => {
  const late = deferred<Response>();
  custom = (call) => (call.url.endsWith("/frame") ? late.promise : undefined);
  const p = props();
  const view = render(<BrowserWorkspace {...p} />);
  fireEvent.click(await screen.findByRole("button", { name: /화면 보기/ }));
  await waitFor(() =>
    expect(calls.some((call) => call.url.endsWith("/frame"))).toBe(true),
  );
  fireEvent.click(screen.getByRole("button", { name: "브라우저 화면 접기" }));
  configured = false;
  view.rerender(<BrowserWorkspace {...p} threadId="thread-b" />);
  await act(async () => late.resolve(json(picture())));
  expect(screen.queryByRole("img")).toBeNull();
  expect(
    calls.some(
      (call) => call.url.endsWith("/resume") || call.url.endsWith("/close"),
    ),
  ).toBe(false);
});

it("retains only the uncertain resume capability for an explicit idempotent result check", async () => {
  const p = props();
  render(<BrowserWorkspace {...p} />);
  await open();
  await take();
  let attempts = 0;
  custom = (call) => {
    if (!call.url.endsWith("/resume")) return;
    attempts++;
    workspace = { ...workspace!, state: "ready", revision: 3 };
    return attempts === 1
      ? Promise.reject(new Error("response lost"))
      : json({ run: { id: "same-turn" } });
  };
  fireEvent.click(screen.getByRole("button", { name: /조작 완료/ }));
  await screen.findByRole("button", { name: "완료 결과 다시 확인" });
  expect(attempts).toBe(1);
  expect(p.onChanged).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "완료 결과 다시 확인" }));
  await waitFor(() => expect(p.onChanged).toHaveBeenCalledOnce());
  const posts = calls.filter((call) => call.url.endsWith("/resume"));
  expect(posts).toHaveLength(2);
  expect(posts[0].body).toEqual(posts[1].body);
  expect(posts[1].body.revision).toBe(2);
  expect(
    screen.queryByRole("button", { name: "완료 결과 다시 확인" }),
  ).toBeNull();
});

it("clears an existing private frame immediately on authentication failure without hijacking chat keys", async () => {
  const p = props();
  render(
    <>
      <textarea aria-label="대화 입력" />
      <BrowserWorkspace {...p} />
    </>,
  );
  await open();
  await take();
  fireEvent.keyDown(screen.getByLabelText("대화 입력"), { key: "Enter" });
  expect(calls.some((call) => call.url.endsWith("/input"))).toBe(false);
  custom = (call) =>
    call.url.endsWith("/input")
      ? json({ code: "unauthorized" }, 401)
      : undefined;
  screen.getByRole("img").focus();
  fireEvent.keyDown(screen.getByRole("img"), { key: "Enter" });
  await waitFor(() => expect(p.onExpired).toHaveBeenCalledOnce());
  expect(screen.queryByRole("img")).toBeNull();
  expect(screen.queryByLabelText("선택한 칸에 입력")).toBeNull();
});

it("keeps chat focus when a slow browser mutation finishes", async () => {
  render(
    <>
      <textarea aria-label="대화 입력" />
      <BrowserWorkspace {...props()} />
    </>,
  );
  await open();
  await take();
  const refresh = deferred<Response>();
  let delayed = false;
  custom = (call) => {
    if (call.url.endsWith("/frame") && !delayed) {
      delayed = true;
      return refresh.promise;
    }
    return undefined;
  };
  screen.getByRole("img").focus();
  fireEvent.keyDown(screen.getByRole("img"), { key: "Enter" });
  const chat = screen.getByLabelText("대화 입력");
  chat.focus();
  await act(async () => refresh.resolve(json(picture(2))));
  await screen.findByRole("img");
  expect(chat).toHaveFocus();
});

it("keeps control through a navigation capture conflict without repeating acknowledged input", async () => {
  const p = props();
  render(<BrowserWorkspace {...p} />);
  await open();
  await take();
  let captured = 0;
  custom = (call) => {
    if (call.url.endsWith("/frame") && ++captured === 1)
      return json({ code: "conflict" }, 409);
  };
  screen.getByRole("img").focus();
  fireEvent.keyDown(screen.getByRole("img"), { key: "Enter" });
  await waitFor(() => expect(captured).toBeGreaterThan(1));
  await waitFor(() => expect(screen.getByRole("button", { name: /조작 완료/ })).toBeEnabled());
  expect(calls.filter((call) => call.url.endsWith("/input"))).toHaveLength(1);
  expect(calls.filter((call) => call.url.endsWith("/takeover"))).toHaveLength(1);
  expect(calls.filter((call) => call.url.endsWith("/frame")).at(-1)?.body.control_token).toBe("synthetic-control");
  fireEvent.click(screen.getByRole("button", { name: /조작 완료/ }));
  await waitFor(() => expect(p.onChanged).toHaveBeenCalledOnce());
  expect(screen.queryByText(/직접 조작하는 동안 에이전트의 관측/)).toBeNull();
});

it("drops control when a conflicting capture reveals a newer controller", async () => {
  render(<BrowserWorkspace {...props()} />);
  await open();
  await take();
  custom = (call) => {
    if (call.url.endsWith("/frame")) {
      workspace = { ...workspace!, revision: 3 };
      return json({ code: "conflict" }, 409);
    }
  };
  screen.getByRole("img").focus();
  fireEvent.keyDown(screen.getByRole("img"), { key: "Enter" });
  await waitFor(() => expect(screen.getByRole("button", { name: "직접 조작하기" })).toBeEnabled());
  expect(screen.queryByRole("button", { name: /조작 완료/ })).toBeNull();
  expect(screen.queryByRole("img")).toBeNull();
  expect(calls.filter((call) => call.url.endsWith("/input"))).toHaveLength(1);
  expect(calls.some((call) => call.url.endsWith("/resume"))).toBe(false);
});
