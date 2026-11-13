import { StrictMode } from "react";
import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import { RequestScope } from "./api";
import type { CommunitySkillCard, SessionView, ThreadSummary } from "../shared/api-types";

const alice: SessionView = {
  user: {
    id: "a",
    name: "Alice Example",
    email: "alice@example.test",
    role: "admin",
    must_change_password: false,
  },
  password_change_required: false,
};
const bob: SessionView = {
  user: {
    id: "b",
    name: "Bob Example",
    email: "bob@example.test",
    role: "member",
    must_change_password: false,
  },
  password_change_required: false,
};
const thread: ThreadSummary = {
  id: "a-thread",
  title: "Alice private conversation",
  created_at: 1,
  updated_at: 1,
};
function response(body: unknown, status = 200) {
  return new Response(status === 204 ? null : JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}
type Route = (path: string, init: RequestInit) => Promise<Response> | Response;
function server(
  route?: Route,
  session: SessionView | null | (() => SessionView | null) = alice,
) {
  const calls: { path: string; init: RequestInit }[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn((url: string, init: RequestInit = {}) => {
      const path = url.replace("/api/v1", "");
      calls.push({ path, init });
      if (path === "/system")
        return Promise.resolve(
          response({
            version: "0.1.0",
            initialized: true,
            stage: "account-foundation",
          }),
        );
      if (path === "/auth/session") {
        const current = typeof session === "function" ? session() : session;
        return Promise.resolve(
          current ? response(current) : response({ code: "unauthorized" }, 401),
        );
      }
      if (path === "/settings/model")
        return Promise.resolve(
          response({ selection: null, effective: null, choices: [] }),
        );
      if (/^\/threads\/[^/]+\/messages/.test(path))
        return Promise.resolve(
          response({
            thread_id: path.split("/")[2],
            cursor: 0,
            turns: [],
            next_before: null,
          }),
        );
      if (/^\/threads\/[^/]+\/events/.test(path))
        return Promise.resolve(
          new Response(new ReadableStream<Uint8Array>(), {
            headers: { "Content-Type": "text/event-stream" },
          }),
        );
      return Promise.resolve(
        route?.(path, init) ??
          (path === "/threads"
            ? response({ threads: [] })
            : path === "/admin/users"
              ? response({ users: [alice.user] })
              : response({}, 204)),
      );
    }),
  );
  return calls;
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

it("keeps an uninitialized installation behind a safe operator setup instruction", async () => {
  const fetcher = vi.fn((_: string) =>
    Promise.resolve(
      response({
        initialized: false,
        version: "0.1.0",
        stage: "account-foundation",
      }),
    ),
  );
  vi.stubGlobal("fetch", fetcher);
  render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
  expect(await screen.findByText("moosuri init")).toBeInTheDocument();
  expect(
    screen.getByText(/Never put a password in a command argument/),
  ).toBeInTheDocument();
  expect(screen.queryByLabelText("Password")).not.toBeInTheDocument();
  expect(fetcher.mock.calls.every((call) => call[0].endsWith("/system"))).toBe(
    true,
  );
});

it("finishes the old cookie transition before accepting a new account login", async () => {
  const user = userEvent.setup();
  const late = deferred<Response>();
  server((path) =>
    path === "/auth/logout" ? late.promise : response({ threads: [] }),
  );
  render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
  await screen.findByRole("heading", { name: "Hello, Alice." });
  await user.click(screen.getAllByRole("button", { name: "Sign out" })[0]);
  expect(await screen.findByText("Finishing sign-out…")).toBeInTheDocument();
  expect(screen.queryByLabelText("Password")).not.toBeInTheDocument();
  expect(screen.queryByText(alice.user.email)).not.toBeInTheDocument();
  await act(async () => {
    late.resolve(response({}, 204));
  });
  expect(await screen.findByLabelText("Password")).toHaveValue("");
});
async function signIn(
  user: ReturnType<typeof userEvent.setup>,
  email = bob.user.email,
) {
  await user.type(await screen.findByLabelText("Email"), email);
  await user.type(screen.getByLabelText("Password"), "synthetic-password");
  await user.click(screen.getByRole("button", { name: "Sign in" }));
}

describe("account and session boundaries", () => {
  it("opens the existing session and makes no model claim or message request", async () => {
    const calls = server();
    render(
      <StrictMode>
        <App />
      </StrictMode>,
    );
    expect(
      await screen.findByRole("heading", { name: "Hello, Alice." }),
    ).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "메시지" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "메시지 전송" })).toBeDisabled();
    expect(calls.every((call) => call.init.credentials === "same-origin")).toBe(
      true,
    );
    expect(
      calls.some(
        (call) =>
          call.path.includes("message") || call.path.includes("provider"),
      ),
    ).toBe(false);
  });
  it("requires a password change before loading any private or admin API", async () => {
    const user = userEvent.setup();
    const restricted = {
      ...alice,
      user: { ...alice.user, must_change_password: true },
      password_change_required: true,
    };
    const calls = server(
      (path) =>
        path === "/auth/login"
          ? response(restricted)
          : path === "/auth/password"
            ? response(alice)
            : response({ threads: [] }),
      null,
    );
    render(
      <StrictMode>
        <App />
      </StrictMode>,
    );
    await signIn(user, alice.user.email);
    expect(
      await screen.findByRole("heading", { name: "A password of your own." }),
    ).toBeInTheDocument();
    expect(
      calls.some(
        (call) => call.path === "/threads" || call.path.startsWith("/admin"),
      ),
    ).toBe(false);
    await user.type(
      screen.getByLabelText("New password"),
      "new-synthetic-password",
    );
    await user.type(
      screen.getByLabelText("Confirm new password"),
      "new-synthetic-password",
    );
    await user.click(
      screen.getByRole("button", { name: "Save password & continue" }),
    );
    expect(
      await screen.findByRole("heading", { name: "Hello, Alice." }),
    ).toBeInTheDocument();
  });
  it("does not mistake an unreachable session endpoint for sign-out", async () => {
    const fetcher = vi.fn((url: string) =>
      url.endsWith("/system")
        ? Promise.resolve(response({ initialized: true }))
        : Promise.reject(new TypeError("offline")),
    );
    vi.stubGlobal("fetch", fetcher);
    render(
      <StrictMode>
        <App />
      </StrictMode>,
    );
    expect(
      await screen.findByRole("heading", { name: "Let’s reconnect." }),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText("Password")).not.toBeInTheDocument();
  });
  it("restarts a pending initial boot on reconnect and discards its late failure", async () => {
    server();
    const reachable = fetch;
    const late = deferred<Response>();
    let reconnected = false;
    const fetcher = vi.fn((url: string, init?: RequestInit) =>
      url.endsWith("/system") && !reconnected
        ? late.promise
        : reachable(url, init),
    );
    vi.stubGlobal("fetch", fetcher);
    render(<StrictMode><App /></StrictMode>);
    await waitFor(() => expect(fetcher).toHaveBeenCalled());
    reconnected = true;
    act(() => { window.dispatchEvent(new Event("online")); });
    await act(async () => { late.resolve(response({ code: "unavailable" }, 503)); });
    expect(await screen.findByRole("heading", { name: "Hello, Alice." })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Let’s reconnect." })).toBeNull();
  });
  it("discards A’s late response after logout and B login", async () => {
    const user = userEvent.setup();
    const late = deferred<Response>();
    let account = "a";
    server((path) => {
      if (path === "/threads")
        return account === "a" ? late.promise : response({ threads: [] });
      if (path === "/auth/login") {
        account = "b";
        return response(bob);
      }
      return response({}, 204);
    });
    render(
      <StrictMode>
        <App />
      </StrictMode>,
    );
    await screen.findByRole("heading", { name: "Hello, Alice." });
    await user.click(screen.getAllByRole("button", { name: "Sign out" })[0]);
    await signIn(user);
    expect(
      await screen.findByRole("heading", { name: "Hello, Bob." }),
    ).toBeInTheDocument();
    await act(async () => {
      late.resolve(response({ threads: [thread] }));
      await late.promise;
    });
    expect(screen.queryByText(thread.title)).not.toBeInTheDocument();
    expect(screen.queryByText("alice@example.test")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Manage installation/ }),
    ).not.toBeInTheDocument();
  });
  it("keeps the signed-in workspace after a network failure and preserves a failed deletion", async () => {
    const user = userEvent.setup();
    server((path, init) =>
      path === "/threads"
        ? response({ threads: [thread] })
        : init.method === "DELETE"
          ? Promise.reject(new TypeError("offline"))
          : response({}, 204),
    );
    render(
      <StrictMode>
        <App />
      </StrictMode>,
    );
    await screen.findByRole("button", { name: `Delete ${thread.title}` });
    await user.click(
      screen.getByRole("button", { name: `Delete ${thread.title}` }),
    );
    await user.click(
      within(screen.getByRole("dialog")).getByRole("button", {
        name: "Delete conversation",
      }),
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not reach moosuri",
    );
    expect(
      screen.getByRole("button", { name: `Delete ${thread.title}` }),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText("Password")).not.toBeInTheDocument();
    expect(screen.getByText(alice.user.email)).toBeInTheDocument();
  });
  it("keeps members out of the admin UI even at a direct admin URL", async () => {
    window.history.replaceState(null, "", "/admin");
    const calls = server(undefined, bob);
    render(
      <StrictMode>
        <App />
      </StrictMode>,
    );
    expect(
      await screen.findByRole("heading", {
        name: "This page is for administrators.",
      }),
    ).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Create invitation/ }),
    ).not.toBeInTheDocument();
    expect(calls.some((call) => call.path.startsWith("/admin"))).toBe(false);
  });
});

describe("local invitations", () => {
  it("shows the one-time password, copies it only on request, and disposes it on close and sign-out", async () => {
    const user = userEvent.setup();
    const clipboard = vi
      .spyOn(navigator.clipboard, "writeText")
      .mockResolvedValue();
    const temporary = "SYNTHETIC-ONE-TIME-PASSWORD";
    const calls = server((path, init) =>
      path === "/admin/users"
        ? init.method === "POST"
          ? response(
              {
                user: { ...bob.user, must_change_password: true },
                temporary_password: temporary,
              },
              201,
            )
          : response({ users: [alice.user] })
        : response({ threads: [] }),
    );
    window.history.replaceState(null, "", "/admin");
    render(
      <StrictMode>
        <App />
      </StrictMode>,
    );
    await screen.findByRole("heading", { name: "A place for your people." });
    await user.type(screen.getByLabelText("Name"), bob.user.name);
    await user.type(screen.getByLabelText("Email"), bob.user.email);
    await user.click(screen.getByRole("button", { name: /Create invitation/ }));
    expect(await screen.findByLabelText("Temporary password")).toHaveValue(
      temporary,
    );
    expect(screen.getByRole("dialog")).toHaveTextContent("No email was sent.");
    expect(clipboard).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Copy password" }));
    expect(clipboard).toHaveBeenCalledWith(temporary);
    await user.click(screen.getByRole("button", { name: "Done" }));
    expect(screen.queryByDisplayValue(temporary)).not.toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByText(bob.user.email)).toBeInTheDocument();
    await user.type(screen.getByLabelText("Name"), "Second Member");
    await user.type(screen.getByLabelText("Email"), "second@example.test");
    await user.click(screen.getByRole("button", { name: /Create invitation/ }));
    await screen.findByLabelText("Temporary password");
    // Programmatic sign-out can occur in another app control while a modal is open.
    await user.click(screen.getAllByRole("button", { name: "Sign out" })[0]);
    await screen.findByRole("button", { name: "Sign in" });
    expect(screen.queryByDisplayValue(temporary)).not.toBeInTheDocument();
    expect(localStorage.length).toBe(0);
    expect(sessionStorage.length).toBe(0);
    expect(
      calls.filter(
        (call) => call.path === "/admin/users" && call.init.method === "POST",
      ),
    ).toHaveLength(2);
    expect(
      calls
        .filter((call) => call.path === "/admin/users")
        .every(
          (call) =>
            new Headers(call.init.headers).get("X-Moosuri-Account") === "a",
        ),
    ).toBe(true);
  });
  it("never reveals a late admin invitation after the administrator signs out", async () => {
    const user = userEvent.setup();
    const late = deferred<Response>();
    server((path, init) =>
      path === "/admin/users"
        ? init.method === "POST"
          ? late.promise
          : response({ users: [alice.user] })
        : path === "/auth/login"
          ? response(bob)
          : path === "/threads"
            ? response({ threads: [] })
            : response({}, 204),
    );
    window.history.replaceState(null, "", "/admin");
    render(
      <StrictMode>
        <App />
      </StrictMode>,
    );
    await screen.findByLabelText("Name");
    await user.type(screen.getByLabelText("Name"), "Synthetic Member");
    await user.type(screen.getByLabelText("Email"), "member@example.test");
    await user.click(screen.getByRole("button", { name: /Create invitation/ }));
    await user.click(screen.getAllByRole("button", { name: "Sign out" })[0]);
    await signIn(user);
    await screen.findByRole("heading", { name: "Hello, Bob." });
    await act(async () => {
      late.resolve(
        response(
          { user: bob.user, temporary_password: "MUST-NEVER-APPEAR" },
          201,
        ),
      );
      await late.promise;
    });
    expect(
      screen.queryByDisplayValue("MUST-NEVER-APPEAR"),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });
});

it("creates and deletes an empty thread only after server success", async () => {
  const user = userEvent.setup();
  const deletion = deferred<Response>();
  const calls = server((path, init) =>
    path === "/threads"
      ? init.method === "POST"
        ? response(thread, 201)
        : response({ threads: [] })
      : init.method === "DELETE"
        ? deletion.promise
        : response({}, 204),
  );
  render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
  await screen.findByRole("heading", { name: "Hello, Alice." });
  await user.click(screen.getByRole("button", { name: "New conversation" }));
  await screen.findByRole("button", { name: `Delete ${thread.title}` });
  await user.click(
    screen.getByRole("button", { name: `Delete ${thread.title}` }),
  );
  await user.click(screen.getByRole("button", { name: "Delete conversation" }));
  expect(
    screen.getByRole("button", { name: `Delete ${thread.title}` }),
  ).toBeInTheDocument();
  await act(async () => {
    deletion.resolve(response({}, 204));
  });
  await waitFor(() =>
    expect(
      screen.queryByRole("button", { name: `Delete ${thread.title}` }),
    ).not.toBeInTheDocument(),
  );
  expect(
    calls.find((call) => call.init.method === "POST")?.init.headers,
  ).toEqual({ "Content-Type": "application/json", "X-Moosuri-Account": "a" });
});

beforeEach(() => {
  vi.stubGlobal("BroadcastChannel", undefined);
});

function authSignals() {
  const channels: Channel[] = [];
  const sent: unknown[] = [];
  class Channel {
    onmessage: ((event: { data: unknown }) => void) | null = null;
    closed = false;
    constructor(readonly name: string) {
      channels.push(this);
    }
    postMessage(value: unknown) {
      sent.push(value);
    }
    close() {
      this.closed = true;
    }
  }
  vi.stubGlobal("BroadcastChannel", Channel);
  return {
    sent,
    receive() {
      for (const channel of channels)
        if (!channel.closed)
          channel.onmessage?.({ data: { type: "auth-changed" } });
    },
  };
}

describe("shared-cookie account changes", () => {
  it("asserts the expected account on private reads and writes while leaving bootstrap and login unbound", async () => {
    const fetcher = vi.fn(() => Promise.resolve(response({}, 204)));
    vi.stubGlobal("fetch", fetcher);
    const privateScope = new RequestScope("a");
    await privateScope.request("/threads");
    await privateScope.request("/admin/users", "POST", {
      name: "Synthetic",
      email: "member@example.test",
    });
    await privateScope.request("/auth/password", "POST", {
      password: "synthetic-password",
    });
    await privateScope.request("/auth/logout", "POST", {});
    const publicScope = new RequestScope();
    await publicScope.request("/system");
    await publicScope.request("/auth/session");
    await publicScope.request("/auth/login", "POST", {
      email: "member@example.test",
      password: "synthetic-password",
    });
    const calls = vi.mocked(fetch).mock.calls;
    expect(
      calls
        .slice(0, 4)
        .every(
          ([, init]) =>
            new Headers(init?.headers).get("X-Moosuri-Account") === "a",
        ),
    ).toBe(true);
    expect(
      calls
        .slice(4)
        .every(
          ([, init]) => !new Headers(init?.headers).has("X-Moosuri-Account"),
        ),
    ).toBe(true);
  });

  it("immediately fences A’s requests on another tab’s auth signal before opening B", async () => {
    const signals = authSignals();
    let cookie = alice;
    const late = deferred<Response>();
    const calls = server(
      (path) =>
        path === "/threads"
          ? cookie === alice
            ? late.promise
            : response({ threads: [] })
          : response({}, 204),
      () => cookie,
    );
    render(
      <StrictMode>
        <App />
      </StrictMode>,
    );
    await screen.findByRole("heading", { name: "Hello, Alice." });
    await waitFor(() =>
      expect(calls.some((call) => call.path === "/threads")).toBe(true),
    );
    const oldRequests = calls.filter((call) => call.path === "/threads");
    expect(oldRequests.length).toBeGreaterThan(0);
    expect(
      oldRequests.every(
        (call) =>
          new Headers(call.init.headers).get("X-Moosuri-Account") === "a",
      ),
    ).toBe(true);
    cookie = bob;
    act(() => signals.receive());
    expect(oldRequests.every((call) => call.init.signal?.aborted)).toBe(true);
    expect(screen.queryByText(alice.user.email)).not.toBeInTheDocument();
    await screen.findByRole("heading", { name: "Hello, Bob." });
    await act(async () => {
      late.resolve(response({ threads: [thread] }));
      await late.promise;
    });
    expect(screen.queryByText(thread.title)).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Manage installation/ }),
    ).not.toBeInTheDocument();
    expect(
      new Headers(
        calls.filter((call) => call.path === "/threads").at(-1)!.init.headers,
      ).get("X-Moosuri-Account"),
    ).toBe("b");
  });

  it("rechecks the shared session on focus when cross-tab messaging is unavailable", async () => {
    let cookie = alice;
    server(undefined, () => cookie);
    render(
      <StrictMode>
        <App />
      </StrictMode>,
    );
    await screen.findByRole("heading", { name: "Hello, Alice." });
    cookie = bob;
    fireEvent(window, new Event("focus"));
    await screen.findByRole("heading", { name: "Hello, Bob." });
    expect(screen.queryByText(alice.user.email)).not.toBeInTheDocument();
  });

  it("preserves the known session when a passive focus check cannot reach the server", async () => {
    let offline = false;
    const calls = server(undefined, () => {
      if (offline) throw new TypeError("offline");
      return alice;
    });
    render(
      <StrictMode>
        <App />
      </StrictMode>,
    );
    await screen.findByRole("heading", { name: "Hello, Alice." });
    const checksBefore = calls.filter(
      (call) => call.path === "/auth/session",
    ).length;
    offline = true;
    await act(async () => {
      fireEvent(window, new Event("focus"));
    });
    expect(calls.filter((call) => call.path === "/auth/session")).toHaveLength(
      checksBefore + 1,
    );
    expect(
      screen.getByRole("heading", { name: "Hello, Alice." }),
    ).toBeInTheDocument();
    expect(screen.queryByLabelText("Password")).not.toBeInTheDocument();
  });

  it("keeps the failed logout’s A assertion when an unannounced cookie switch makes B current", async () => {
    const user = userEvent.setup();
    let cookie = alice;
    let attempts = 0;
    let revocations = 0;
    const calls = server(
      (path, init) => {
        if (path !== "/auth/logout") return response({ threads: [] });
        attempts += 1;
        if (attempts === 1) return Promise.reject(new TypeError("reply lost"));
        if (
          new Headers(init.headers).get("X-Moosuri-Account") !== cookie.user.id
        )
          return response({ code: "account_mismatch" }, 401);
        revocations += 1;
        return response({}, 204);
      },
      () => cookie,
    );
    render(
      <StrictMode>
        <App />
      </StrictMode>,
    );
    await screen.findByRole("heading", { name: "Hello, Alice." });
    await user.click(screen.getAllByRole("button", { name: "Sign out" })[0]);
    await screen.findByRole("button", { name: "Retry sign-out" });
    cookie = bob;
    await user.click(screen.getByRole("button", { name: "Retry sign-out" }));
    await screen.findByRole("heading", { name: "Hello, Bob." });
    expect(revocations).toBe(0);
    expect(
      calls
        .filter((call) => call.path === "/auth/logout")
        .every(
          (call) =>
            new Headers(call.init.headers).get("X-Moosuri-Account") === "a",
        ),
    ).toBe(true);
  });
});

describe("authentication mutation overlap", () => {
  it("prevents logout while a password rotation response is held", async () => {
    const user = userEvent.setup();
    const late = deferred<Response>();
    const restricted = {
      ...alice,
      user: { ...alice.user, must_change_password: true },
      password_change_required: true,
    };
    const calls = server(
      (path) =>
        path === "/auth/password" ? late.promise : response({ threads: [] }),
      restricted,
    );
    render(
      <StrictMode>
        <App />
      </StrictMode>,
    );
    await user.type(
      await screen.findByLabelText("New password"),
      "new-synthetic-password",
    );
    await user.type(
      screen.getByLabelText("Confirm new password"),
      "new-synthetic-password",
    );
    await user.click(
      screen.getByRole("button", { name: "Save password & continue" }),
    );
    const checksBefore = calls.filter(
      (call) => call.path === "/auth/session",
    ).length;
    fireEvent(window, new Event("focus"));
    fireEvent(document, new Event("visibilitychange"));
    expect(calls.filter((call) => call.path === "/auth/session")).toHaveLength(
      checksBefore,
    );
    expect(screen.getByRole("button", { name: "Sign out" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Sign out" }));
    expect(calls.some((call) => call.path === "/auth/logout")).toBe(false);
    expect(
      new Headers(
        calls.find((call) => call.path === "/auth/password")!.init.headers,
      ).get("X-Moosuri-Account"),
    ).toBe("a");
    await act(async () => {
      late.resolve(response(alice));
    });
    await screen.findByRole("heading", { name: "Hello, Alice." });
  });

  it("prevents failed-logout retry from overlapping a held login and broadcasts only a nonsecret signal", async () => {
    const signals = authSignals();
    const user = userEvent.setup();
    const late = deferred<Response>();
    const calls = server((path) =>
      path === "/auth/logout"
        ? Promise.reject(new TypeError("reply lost"))
        : path === "/auth/login"
          ? late.promise
          : response({ threads: [] }),
    );
    render(
      <StrictMode>
        <App />
      </StrictMode>,
    );
    await screen.findByRole("heading", { name: "Hello, Alice." });
    await user.click(screen.getAllByRole("button", { name: "Sign out" })[0]);
    await screen.findByRole("button", { name: "Retry sign-out" });
    await signIn(user);
    expect(
      screen.getByRole("button", { name: "Retry sign-out" }),
    ).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Retry sign-out" }));
    expect(calls.filter((call) => call.path === "/auth/logout")).toHaveLength(
      1,
    );
    await act(async () => {
      late.resolve(response(bob));
    });
    await screen.findByRole("heading", { name: "Hello, Bob." });
    expect(
      new Headers(
        calls.find((call) => call.path === "/auth/login")!.init.headers,
      ).has("X-Moosuri-Account"),
    ).toBe(false);
    expect(signals.sent).toEqual([
      { type: "auth-changed" },
      { type: "auth-changed" },
    ]);
  });
});


it.each(["", "검토 중인 초안\n예산은 10만 원"])(
  "opens a community starter without losing an unsent draft or sending a message (%s)",
  async (draft) => {
    const card: CommunitySkillCard = {
      id: "source-briefing", name: "모아 둔 자료 정리", description: "정리할 자료를 골라 주세요",
      source: "https://github.com/codemoo/moosuri", source_commit: "a".repeat(40),
      version: "1.0.0", digest: "a".repeat(64), license: "Apache-2.0",
      capabilities: ["knowledge_search"], installed_version: "1.0.0",
      installed_digest: "a".repeat(64), enabled: true, hidden: false, revision: 1,
      can_rollback: false, readiness: "ready", missing: [],
    };
    const calls = server(path => path === "/community-skills" ? response([card]) : response({ threads: [] }));
    const user = userEvent.setup();
    render(<App />);
    const composer = await screen.findByRole("textbox", { name: "메시지" });
    if (draft) await user.type(composer, draft);
    await user.click(screen.getByRole("button", { name: /제안 · 할 수 있는 일/ }));
    await user.click(await screen.findByRole("button", { name: "채팅에서 시작" }));
    expect(await screen.findByRole("textbox", { name: "메시지" })).toHaveValue(`모아 둔 자료 정리 절차로 도와줘: ${draft}`);
    expect(calls.filter(call => call.init.method === "POST")).toHaveLength(0);
    expect(window.location.pathname).toBe("/app");
  },
);

it("opens explicit missions and returns without sending or losing the chat draft",async()=>{
 const calls=server(path=>path==="/missions"?response([]):response({threads:[]}));
 const user=userEvent.setup();render(<App/>);
 const composer=await screen.findByRole("textbox",{name:"메시지"});
 await user.type(composer,"확인 중인 채팅 초안");
 await user.click(screen.getByRole("button",{name:/맡긴 일/}));
 await screen.findByRole("heading",{name:"맡긴 일",level:2});
 await user.click(screen.getByRole("button",{name:"← 대화로"}));
 expect(await screen.findByRole("textbox",{name:"메시지"})).toHaveValue("확인 중인 채팅 초안");
 expect(calls.filter(c=>c.init.method==="POST")).toHaveLength(0);
});

it("opens Calendar and returns with the unsent chat draft intact",async()=>{const calls=server(path=>path==="/calendar/connections"?response({configured:false,connections:[]}):path==="/calendar/actions"?response([]):response({threads:[]}));const user=userEvent.setup();render(<App/>);const composer=await screen.findByRole("textbox",{name:"메시지"});await user.type(composer,"달력 확인 전 초안");await user.click(screen.getByRole("button",{name:/내 Calendar/}));await screen.findByRole("heading",{name:"내 Google Calendar"});await user.click(screen.getByRole("button",{name:"← 대화로"}));expect(await screen.findByRole("textbox",{name:"메시지"})).toHaveValue("달력 확인 전 초안");expect(calls.filter(c=>c.init.method==="POST")).toHaveLength(0);});

it("opens a typed inbox mission link in the app without losing the chat draft",async()=>{
 const id="00000000-0000-4000-8000-000000000001";
 const calls=server(path=>path.startsWith("/notifications?")?response({items:[{id:"notice",title:"예약한 일의 실행 기록",message:"실행 상태: achieved",mission_id:id,scheduled_for:1,delivered_at:1,read:false,dismissed:false,quiet:false,delayed:false}],next_cursor:null}):path==="/notifications/settings"?response({revision:1,enabled:false,timezone:"UTC",start_minute:1320,end_minute:420}):["/missions","/recurring-missions","/schedules"].includes(path)?response([]):response({threads:[]}));
 const user=userEvent.setup();render(<App/>);
 await user.type(await screen.findByRole("textbox",{name:"메시지"}),"소식함 확인 후 이어 쓸 초안");
 await user.click(screen.getByRole("button",{name:/예약 · 소식/}));
 await user.click(await screen.findByRole("link",{name:"맡긴 일의 실행 기록"}));
 await screen.findByRole("heading",{name:"맡긴 일",level:2});
 expect(window.location.pathname).toBe("/app/missions");expect(window.location.hash).toBe(`#mission-${id}`);
 await user.click(screen.getByRole("button",{name:"← 대화로"}));
 expect(await screen.findByRole("textbox",{name:"메시지"})).toHaveValue("소식함 확인 후 이어 쓸 초안");
 expect(calls.filter(c=>c.init.method==="POST")).toHaveLength(0);
});
