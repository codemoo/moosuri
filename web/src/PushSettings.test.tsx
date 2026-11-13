import {
  render,
  screen,
  fireEvent,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, it, expect, vi } from "vitest";
import { IDBFactory } from "fake-indexeddb";
import { PushSettings } from "./PushSettings";
import {
  readPushBinding,
  activatePushAccount,
  clearPushBinding,
} from "./push-state";
import {
  clearBrowserPushBinding,
  deactivateBrowserPushBinding,
  activatePushAccount as activateBrowserPushAccount,
} from "./push-binding";
const key = // gitleaks:allow -- synthetic Web Push fixture, not a live subscription
  "BLn9b-VR0ca83knDNZ32dCHGyjJp-1riX9ZTN40MqV8K_LpQmLqxC_DoHvqvFXO_nGdAB4W9dogZb_sM-uV4JbY"; // gitleaks:allow -- synthetic Web Push fixture, not a live subscription
const binding = "20000000-0000-4000-8000-000000000001";
let permission: NotificationPermission;
let requestPermission: ReturnType<typeof vi.fn>;
let subscribe: ReturnType<typeof vi.fn>;
let posts: any[];
let offs: any[];
let offHandler: (body: any) => Promise<Response>;
let handler: (body: unknown) => Promise<Response>;
function response(value: unknown) {
  return new Response(JSON.stringify(value), {
    headers: { "content-type": "application/json" },
  });
}
beforeEach(() => {
  permission = "default";
  posts = [];
  offs = [];
  offHandler = async () => new Response(null, { status: 204 });
  vi.stubGlobal("indexedDB", new IDBFactory());
  vi.stubGlobal("isSecureContext", true);
  vi.stubGlobal("PushManager", class {});
  let tail = Promise.resolve();
  Object.defineProperty(navigator, "locks", {
    configurable: true,
    value: {
      request: vi.fn(
        (_name: string, _options: unknown, work: () => Promise<unknown>) => {
          const prior = tail;
          let release!: () => void;
          tail = new Promise<void>((resolve) => {
            release = resolve;
          });
          return prior.then(work).finally(release);
        },
      ),
    },
  });
  requestPermission = vi.fn(async () => {
    permission = "granted";
    return permission;
  });
  class Permission {
    static get permission() {
      return permission;
    }
    static requestPermission = requestPermission;
  }
  vi.stubGlobal("Notification", Permission);
  subscribe = vi.fn(async () => ({
    options: { applicationServerKey: new Uint8Array() },
    toJSON: () => ({
      endpoint: "https://fcm.googleapis.com/fcm/send/synthetic",
      keys: { p256dh: key, auth: "_ordMnz7uTCmrpBTeUV4Bw" }, // gitleaks:allow -- synthetic Web Push fixture, not a live subscription
    }),
    unsubscribe: vi.fn(async () => true),
  }));
  Object.defineProperty(navigator, "serviceWorker", {
    configurable: true,
    value: {
      getRegistration: vi.fn(async () => ({
        pushManager: { getSubscription: vi.fn(async () => null), subscribe },
        getNotifications: vi.fn(async () => []),
      })),
    },
  });
  handler = async () =>
    response({ binding_id: binding, state: "active", last_outcome: null });
  vi.stubGlobal(
    "fetch",
    vi.fn(async (_url: string, init?: RequestInit) => {
      if (init?.method === "POST" && _url.endsWith("/disable")) {
        const body = JSON.parse(String(init.body));
        offs.push(body);
        return offHandler(body);
      }
      if (init?.method === "POST") {
        const body = JSON.parse(String(init.body));
        posts.push(body);
        return handler(body);
      }
      if (init?.method === "DELETE") return new Response(null, { status: 204 });
      return response({ application_server_key: key, device: null });
    }),
  );
});
it("permission is requested only by a user gesture and accepted connection persists only IDs", async () => {
  render(<PushSettings accountId="a" onExpired={vi.fn()} />);
  const enable = await screen.findByRole("button", {
    name: "이 기기 알림 켜기",
  });
  await waitFor(() => expect(enable).toBeEnabled());
  expect(requestPermission).not.toHaveBeenCalled();
  expect(subscribe).not.toHaveBeenCalled();
  fireEvent.click(enable);
  await screen.findByText("새 소식을 이 기기에도 알립니다.");
  expect(requestPermission).toHaveBeenCalledOnce();
  expect(subscribe).toHaveBeenCalledOnce();
  const state = await readPushBinding();
  expect(state).toMatchObject({ account_id: "a", binding_id: binding });
  expect(JSON.stringify(state)).not.toContain("fcm.googleapis");
  expect(screen.queryByText(/_ordMn/)).not.toBeInTheDocument();
});
it("denied permission is graceful and never subscribes or sends private keys", async () => {
  permission = "denied";
  render(<PushSettings accountId="a" onExpired={vi.fn()} />);
  expect(await screen.findByText(/브라우저 알림이 차단/)).toBeInTheDocument();
  expect(
    screen.getByRole("button", { name: "이 기기 알림 켜기" }),
  ).toBeDisabled();
  expect(requestPermission).not.toHaveBeenCalled();
  expect(subscribe).not.toHaveBeenCalled();
  expect(posts).toHaveLength(0);
});
it("lost enrollment ACK repeats exactly its UUID and payload without another permission/subscription", async () => {
  let attempts = 0;
  handler = async () => {
    if (++attempts === 1) throw new TypeError("offline");
    return response({
      binding_id: binding,
      state: "active",
      last_outcome: null,
    });
  };
  render(<PushSettings accountId="a" onExpired={vi.fn()} />);
  const enable = await screen.findByRole("button", {
    name: "이 기기 알림 켜기",
  });
  await waitFor(() => expect(enable).toBeEnabled());
  fireEvent.click(enable);
  fireEvent.click(
    await screen.findByRole("button", { name: "같은 연결 요청 다시 확인" }),
  );
  await screen.findByText("새 소식을 이 기기에도 알립니다.");
  expect(posts).toHaveLength(2);
  expect(posts[1]).toEqual(posts[0]);
  expect(requestPermission).toHaveBeenCalledOnce();
  expect(subscribe).toHaveBeenCalledOnce();
});
it("a held A response cannot bind the browser or supply a retry when B takes the UI", async () => {
  let release!: (response: Response) => void;
  handler = () => new Promise((resolve) => (release = resolve));
  const view = render(<PushSettings accountId="a" onExpired={vi.fn()} />);
  const enable = await screen.findByRole("button", {
    name: "이 기기 알림 켜기",
  });
  await waitFor(() => expect(enable).toBeEnabled());
  fireEvent.click(enable);
  await waitFor(() => expect(posts).toHaveLength(1));
  view.rerender(<PushSettings accountId="b" onExpired={vi.fn()} />);
  await activatePushAccount("b");
  release(
    response({ binding_id: binding, state: "active", last_outcome: null }),
  );
  await waitFor(async () =>
    expect((await readPushBinding())?.account_id).toBe("b"),
  );
  expect((await readPushBinding())?.binding_id).toBeNull();
  expect(
    screen.queryByRole("button", { name: "같은 연결 요청 다시 확인" }),
  ).not.toBeInTheDocument();
});
it("a known subscription rejection is distinct from a lost ACK", async () => {
  handler = async () =>
    new Response('{"code":"invalid_input"}', {
      status: 400,
      headers: { "content-type": "application/json" },
    });
  render(<PushSettings accountId="a" onExpired={vi.fn()} />);
  const enable = await screen.findByRole("button", {
    name: "이 기기 알림 켜기",
  });
  await waitFor(() => expect(enable).toBeEnabled());
  fireEvent.click(enable);
  await screen.findByText(/이 브라우저의 푸시 연결을 지원하지/);
  expect(
    screen.queryByRole("button", { name: "같은 연결 요청 다시 확인" }),
  ).not.toBeInTheDocument();
  expect((await readPushBinding())?.binding_id).toBeNull();
});

function subscription() {
  return {
    options: { applicationServerKey: new Uint8Array() },
    toJSON: () => ({
      endpoint: "https://fcm.googleapis.com/fcm/send/synthetic",
      keys: { p256dh: key, auth: "_ordMnz7uTCmrpBTeUV4Bw" }, // gitleaks:allow -- synthetic Web Push fixture, not a live subscription
    }),
    unsubscribe: vi.fn(async () => true),
  };
}
function tabs() {
  return render(
    <>
      <div data-testid="first">
        <PushSettings accountId="a" onExpired={vi.fn()} />
      </div>
      <div data-testid="second">
        <PushSettings accountId="a" onExpired={vi.fn()} />
      </div>
    </>,
  );
}
async function button(tab?: string) {
  const element = tab ? within(screen.getByTestId(tab)) : screen;
  const value = await element.findByRole("button", {
    name: "이 기기 알림 켜기",
  });
  await waitFor(() => expect(value).toBeEnabled());
  return value;
}
it("immediate enable/off/enable works in the same authenticated mounted view", async () => {
  render(<PushSettings accountId="a" onExpired={vi.fn()} />);
  fireEvent.click(await button());
  await screen.findByText("새 소식을 이 기기에도 알립니다.");
  fireEvent.click(screen.getByRole("button", { name: "이 기기 알림 끄기" }));
  await waitFor(() => expect(offs).toHaveLength(1));
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "이 기기 알림 켜기" }),
    ).toBeEnabled(),
  );
  expect(await readPushBinding()).toMatchObject({
    account_id: "a",
    binding_id: null,
  });
  fireEvent.click(await button());
  await screen.findByText("새 소식을 이 기기에도 알립니다.");
  expect(posts).toHaveLength(2);
  expect(posts[0].operation_revision).toBeLessThan(offs[0].operation_revision);
  expect(offs[0].operation_revision).toBeLessThan(posts[1].operation_revision);
  expect((await readPushBinding())?.binding_id).toBe(binding);
});
it("an old held getSubscription cannot unsubscribe or POST after a newer tab connects", async () => {
  let release!: (value: any) => void;
  const getSubscription = vi
    .fn()
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          release = resolve;
        }),
    )
    .mockResolvedValue(null);
  vi.mocked(navigator.serviceWorker.getRegistration).mockResolvedValue({
    pushManager: { getSubscription, subscribe },
    getNotifications: async () => [],
  } as any);
  tabs();
  fireEvent.click(await button("first"));
  await waitFor(() => expect(getSubscription).toHaveBeenCalledOnce());
  fireEvent.click(await button("second"));
  await waitFor(async () =>
    expect((await readPushBinding())?.revision).toBeGreaterThan(2),
  );
  expect(posts).toHaveLength(0);
  const old = subscription();
  release(old);
  await within(screen.getByTestId("second")).findByText(
    "새 소식을 이 기기에도 알립니다.",
  );
  await within(screen.getByTestId("first")).findByText(/다른 탭이나 로그인/);
  expect(old.unsubscribe).not.toHaveBeenCalled();
  expect(subscribe).toHaveBeenCalledOnce();
  expect(posts).toHaveLength(1);
  expect((await readPushBinding())?.binding_id).toBe(binding);
});
it("an old held subscribe cannot POST after a newer tab connects", async () => {
  let release!: (value: any) => void;
  subscribe
    .mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          release = resolve;
        }),
    )
    .mockResolvedValue(subscription());
  tabs();
  fireEvent.click(await button("first"));
  await waitFor(() => expect(subscribe).toHaveBeenCalledOnce());
  fireEvent.click(await button("second"));
  await waitFor(async () =>
    expect((await readPushBinding())?.revision).toBeGreaterThan(2),
  );
  expect(posts).toHaveLength(0);
  release(subscription());
  await within(screen.getByTestId("second")).findByText(
    "새 소식을 이 기기에도 알립니다.",
  );
  await within(screen.getByTestId("first")).findByText(/다른 탭이나 로그인/);
  expect(posts).toHaveLength(1);
  expect((await readPushBinding())?.binding_id).toBe(binding);
});
for (const held of ["request", "ack"] as const)
  it(`a held older ${held} cannot replace the newer accepted enrollment`, async () => {
    let release!: () => void;
    let latest = 0;
    let active = "";
    const process = (body: any) => {
      if (body.operation_revision <= latest)
        return new Response('{"code":"conflict"}', {
          status: 409,
          headers: { "content-type": "application/json" },
        });
      latest = body.operation_revision;
      active = `20000000-0000-4000-8000-${String(latest).padStart(12, "0")}`;
      return response({
        binding_id: active,
        state: "active",
        last_outcome: null,
      });
    };
    handler = async (body: any) => {
      if (posts.length !== 1) return process(body);
      const ack = held === "ack" ? process(body) : undefined;
      await new Promise<void>((resolve) => {
        release = resolve;
      });
      return ack ?? process(body);
    };
    tabs();
    fireEvent.click(await button("first"));
    await waitFor(() => expect(posts).toHaveLength(1));
    fireEvent.click(await button("second"));
    await within(screen.getByTestId("second")).findByText(
      "새 소식을 이 기기에도 알립니다.",
    );
    const newer = active;
    release();
    await within(screen.getByTestId("first")).findByText(/다른 탭이나 로그인/);
    expect(posts).toHaveLength(2);
    expect(active).toBe(newer);
    expect((await readPushBinding())?.binding_id).toBe(newer);
  });
it("account and renewed same-account lifetimes fence a held browser subscription before POST", async () => {
  for (const next of ["b", "a"]) {
    let release!: (value: any) => void;
    subscribe.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          release = resolve;
        }),
    );
    const view = render(<PushSettings accountId="a" onExpired={vi.fn()} />);
    fireEvent.click(await button());
    await waitFor(() => expect(subscribe.mock.calls.length).toBeGreaterThan(0));
    await clearPushBinding("a");
    await activatePushAccount(next);
    release(subscription());
    await screen.findByText(/다른 탭이나 로그인/);
    expect(posts).toHaveLength(0);
    expect((await readPushBinding())?.binding_id).toBeNull();
    view.unmount();
    subscribe.mockClear();
  }
});
it("retry of an uncertain older enrollment is fenced before another POST", async () => {
  handler = async () => {
    if (posts.length === 1) throw new TypeError("lost ACK");
    return response({
      binding_id: binding,
      state: "active",
      last_outcome: null,
    });
  };
  tabs();
  fireEvent.click(await button("first"));
  const retry = await within(screen.getByTestId("first")).findByRole("button", {
    name: "같은 연결 요청 다시 확인",
  });
  fireEvent.click(await button("second"));
  await within(screen.getByTestId("second")).findByText(
    "새 소식을 이 기기에도 알립니다.",
  );
  fireEvent.click(retry);
  await within(screen.getByTestId("first")).findByText(/다른 탭이나 로그인/);
  expect(posts).toHaveLength(2);
  expect(
    within(screen.getByTestId("first")).queryByRole("button", {
      name: "같은 연결 요청 다시 확인",
    }),
  ).not.toBeInTheDocument();
});
it("uncertain off retries exactly and a later enable supersedes that off receipt", async () => {
  offHandler = async () => {
    if (offs.length === 1) throw new TypeError("lost off ACK");
    return new Response(null, { status: 204 });
  };
  render(<PushSettings accountId="a" onExpired={vi.fn()} />);
  fireEvent.click(await button());
  await screen.findByText("새 소식을 이 기기에도 알립니다.");
  fireEvent.click(screen.getByRole("button", { name: "이 기기 알림 끄기" }));
  await screen.findByText(/서버 연결 해제를 확인하지/);
  fireEvent.click(screen.getByRole("button", { name: "이 기기 알림 끄기" }));
  await waitFor(() => expect(offs).toHaveLength(2));
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "이 기기 알림 켜기" }),
    ).toBeEnabled(),
  );
  expect(offs[1]).toEqual(offs[0]);
  fireEvent.click(await button());
  await screen.findByText("새 소식을 이 기기에도 알립니다.");
  expect(posts[1].operation_revision).toBeGreaterThan(
    offs[1].operation_revision,
  );
});

for (const held of ["request", "ack"] as const)
  it(`explicit off fences an older enrollment held at ${held}`, async () => {
    let release!: () => void;
    let revision = 0;
    let active = false;
    const admit = (body: any) => {
      if (body.operation_revision <= revision)
        return new Response('{"code":"conflict"}', {
          status: 409,
          headers: { "content-type": "application/json" },
        });
      revision = body.operation_revision;
      active = true;
      return response({
        binding_id: binding,
        state: "active",
        last_outcome: null,
      });
    };
    handler = async (body: any) => {
      const ack = held === "ack" ? admit(body) : undefined;
      await new Promise<void>((resolve) => {
        release = resolve;
      });
      return ack ?? admit(body);
    };
    offHandler = async (body: any) => {
      expect(body.operation_revision).toBeGreaterThan(revision);
      revision = body.operation_revision;
      active = false;
      return new Response(null, { status: 204 });
    };
    tabs();
    fireEvent.click(await button("first"));
    await waitFor(() => expect(posts).toHaveLength(1));
    fireEvent.click(
      within(screen.getByTestId("second")).getByRole("button", {
        name: "이 기기 알림 끄기",
      }),
    );
    await waitFor(() => expect(offs).toHaveLength(1));
    await waitFor(() =>
      expect(
        within(screen.getByTestId("second")).getByRole("button", {
          name: "이 기기 알림 켜기",
        }),
      ).toBeEnabled(),
    );
    release();
    await within(screen.getByTestId("first")).findByText(/다른 탭이나 로그인/);
    expect(active).toBe(false);
    expect(await readPushBinding()).toMatchObject({
      account_id: "a",
      binding_id: null,
    });
  });

it("missing Web Locks is honestly unsupported before permission or browser mutation", async () => {
  Object.defineProperty(navigator, "locks", {
    configurable: true,
    value: undefined,
  });
  render(<PushSettings accountId="a" onExpired={vi.fn()} />);
  await screen.findByText(/안전한 기기 알림 연결을 지원하지/);
  expect(
    screen.queryByRole("button", { name: "이 기기 알림 켜기" }),
  ).not.toBeInTheDocument();
  expect(requestPermission).not.toHaveBeenCalled();
  expect(subscribe).not.toHaveBeenCalled();
  expect(posts).toHaveLength(0);
});

function heldUnsubscribe() {
  let live = true;
  let release!: () => void;
  const existing = subscription();
  existing.options.applicationServerKey = Uint8Array.from(
    atob(key.replace(/-/g, "+").replace(/_/g, "/")),
    (c) => c.charCodeAt(0),
  );
  existing.unsubscribe.mockImplementation(
    () =>
      new Promise<boolean>((resolve) => {
        release = () => {
          live = false;
          resolve(true);
        };
      }),
  );
  const fresh = subscription();
  fresh.toJSON = () => ({
    endpoint: "https://fcm.googleapis.com/fcm/send/synthetic-fresh",
    keys: { p256dh: key, auth: "_ordMnz7uTCmrpBTeUV4Bw" }, // gitleaks:allow -- synthetic Web Push fixture, not a live subscription
  });
  subscribe.mockImplementation(async () => {
    live = true;
    return fresh;
  });
  const getSubscription = vi.fn(async () => (live ? existing : null));
  vi.mocked(navigator.serviceWorker.getRegistration).mockResolvedValue({
    pushManager: { getSubscription, subscribe },
    getNotifications: async () => [],
  } as any);
  let gets = 0;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (_url: string, init?: RequestInit) => {
      if (init?.method === "POST") {
        const body = JSON.parse(String(init.body));
        if (_url.endsWith("/disable")) {
          offs.push(body);
          return offHandler(body);
        }
        posts.push(body);
        return handler(body);
      }
      gets++;
      return response({
        application_server_key: key,
        device:
          gets === 1
            ? { binding_id: binding, state: "off", last_outcome: "rejected" }
            : null,
      });
    }),
  );
  return {
    existing,
    getSubscription,
    release: () => release(),
    live: () => live,
  };
}
it("review: in-flight old unsubscribe cannot invalidate a newer accepted binding", async () => {
  permission = "granted";
  const browser = heldUnsubscribe();
  tabs();
  fireEvent.click(await button("first"));
  await waitFor(() =>
    expect(browser.existing.unsubscribe).toHaveBeenCalledOnce(),
  );
  fireEvent.click(await button("second"));
  await waitFor(async () =>
    expect((await readPushBinding())?.revision).toBeGreaterThan(2),
  );
  expect(browser.getSubscription).toHaveBeenCalledOnce();
  expect(posts).toHaveLength(0);
  expect(subscribe).not.toHaveBeenCalled();
  browser.release();
  await within(screen.getByTestId("second")).findByText(
    "새 소식을 이 기기에도 알립니다.",
  );
  await within(screen.getByTestId("first")).findByText(/다른 탭이나 로그인/);
  expect(posts).toHaveLength(1);
  expect(posts[0].subscription.endpoint).toMatch(/synthetic-fresh$/);
  expect(subscribe).toHaveBeenCalledOnce();
  expect(browser.live()).toBe(true);
  expect((await readPushBinding())?.binding_id).toBe(binding);
  const names = vi
    .mocked(navigator.locks.request)
    .mock.calls.map(([name]) => name);
  expect(new Set(names)).toEqual(new Set(["moosuri-push-subscription-v1"]));
});
it("explicit off waits for an old unsubscribe and a later on obtains a live subscription", async () => {
  permission = "granted";
  const browser = heldUnsubscribe();
  tabs();
  fireEvent.click(await button("first"));
  await waitFor(() =>
    expect(browser.existing.unsubscribe).toHaveBeenCalledOnce(),
  );
  fireEvent.click(
    within(screen.getByTestId("second")).getByRole("button", {
      name: "이 기기 알림 끄기",
    }),
  );
  await waitFor(async () =>
    expect((await readPushBinding())?.revision).toBeGreaterThan(2),
  );
  expect(offs).toHaveLength(0);
  expect((await readPushBinding())?.binding_id).toBeNull();
  browser.release();
  await waitFor(() => expect(offs).toHaveLength(1));
  fireEvent.click(await button("second"));
  await within(screen.getByTestId("second")).findByText(
    "새 소식을 이 기기에도 알립니다.",
  );
  expect(browser.live()).toBe(true);
  expect(posts).toHaveLength(1);
  expect(posts[0].operation_revision).toBeGreaterThan(
    offs[0].operation_revision,
  );
});
it("account switch and logout fence old mutation while new enrollment waits for browser ownership", async () => {
  permission = "granted";
  const browser = heldUnsubscribe();
  const view = render(<PushSettings accountId="a" onExpired={vi.fn()} />);
  fireEvent.click(await button());
  await waitFor(() =>
    expect(browser.existing.unsubscribe).toHaveBeenCalledOnce(),
  );
  const logout = clearBrowserPushBinding("a");
  await waitFor(async () =>
    expect((await readPushBinding())?.account_id).toBeNull(),
  );
  view.rerender(<PushSettings accountId="b" onExpired={vi.fn()} />);
  const activation = activateBrowserPushAccount("b");
  await waitFor(async () =>
    expect((await readPushBinding())?.account_id).toBe("b"),
  );
  expect(posts).toHaveLength(0);
  expect(browser.getSubscription).toHaveBeenCalledOnce();
  browser.release();
  await Promise.all([logout, activation]);
  fireEvent.click(await button());
  await screen.findByText("새 소식을 이 기기에도 알립니다.");
  expect(posts).toHaveLength(1);
  expect((await readPushBinding())?.account_id).toBe("b");
  expect(browser.live()).toBe(true);
});

it("a queued enrollment rechecks generation after acquiring browser ownership", async () => {
  permission = "granted";
  const browser = heldUnsubscribe();
  tabs();
  fireEvent.click(await button("first"));
  await waitFor(() =>
    expect(browser.existing.unsubscribe).toHaveBeenCalledOnce(),
  );
  fireEvent.click(await button("second"));
  await waitFor(async () =>
    expect((await readPushBinding())?.revision).toBeGreaterThan(2),
  );
  const off = deactivateBrowserPushBinding("a");
  await waitFor(async () =>
    expect((await readPushBinding())?.revision).toBeGreaterThan(3),
  );
  browser.release();
  await off;
  await within(screen.getByTestId("second")).findByText(/다른 탭이나 로그인/);
  expect(browser.getSubscription).toHaveBeenCalledOnce();
  expect(subscribe).not.toHaveBeenCalled();
  expect(posts).toHaveLength(0);
  expect((await readPushBinding())?.binding_id).toBeNull();
});
