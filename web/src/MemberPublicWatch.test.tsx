import { render, screen, fireEvent, cleanup, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import {
  PublicWatchLog, PublicWatchStart, describePublicWatch, parsePublicWatchChat, publicWatchChatReply, toPublicWatchRequest, unsafePublicUrl,
  type PublicWatchIntent, type PublicWatchView,
} from "./MemberPublicWatch";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

const URL_ = "https://status.example.test/service";
const PHRASE = `${URL_} 를 매시간 확인하고 바뀔 때만 알려줘`;
const THREAD = "11111111-1111-4111-8111-111111111111";
const intent = (): PublicWatchIntent => { const p = parsePublicWatchChat(PHRASE); if (!p || !p.ok) throw new Error("not parsed"); return p.intent; };
const ROUTE = "/api/v1/personal/tasks/public-watches";

it("parses only an explicit recurring change-only request with one exact public address", () => {
  expect(intent()).toEqual({ url: URL_, every_seconds: 3600, days: 7, assumed: ["days"], message: PHRASE });
  expect(parsePublicWatchChat(`${URL_}를 30분마다 확인해서 변하면 알려줘 3일 동안`)).toMatchObject({ ok: true, intent: { url: URL_, every_seconds: 1800, days: 3, assumed: [] } });
  expect(parsePublicWatchChat(`매일 ${URL_} 바뀌면 알려줘.`)).toMatchObject({ ok: true, intent: { every_seconds: 86400 } });
  expect(describePublicWatch(intent())).toMatch(/1시간마다.*바뀐 경우에만.*7일.*공개 페이지만/);
  expect(parsePublicWatchChat(`‘${URL_}’ 를 매시간 확인하고 바뀔 때만 알려줘`)).toMatchObject({ ok: true, intent: { url: URL_ } });
  const following = parsePublicWatchChat(`${URL_} 바뀌면 알려줘`);
  expect(following).toMatchObject({ ok: true, intent: { every_seconds: 3600, days: 7, assumed: ["every_seconds", "days"] } });
  if (following?.ok) expect(describePublicWatch(following.intent)).toContain("간격은 1시간");
  expect(parsePublicWatchChat(`${URL_} 매주 바뀌면 알려줘`)).toMatchObject({ ok: false, reason: "unsupported_interval" });
  // One-off, negated, question, quoted instruction, no address, no change-only wording: ordinary chat.
  for (const text of [
    `${URL_} 지금 한 번 확인해줘`,
    `${URL_} 한 번만 바뀌었는지 알려줘`,
    `${URL_} 를 매시간 확인하고 바뀔 때만 알려주지 마`,
    `${URL_} 매시간 바뀌면 알려줄 수 있어?`,
    `“${URL_} 를 매시간 확인하고 바뀔 때만 알려줘”라는 말은 무슨 뜻이야?`,
    "서비스 상태 페이지를 매시간 확인하고 바뀔 때만 알려줘",
    `${URL_} 를 매시간 확인해줘`,
    `${URL_} 바뀌었는지 알려줘`,
    "",
  ]) expect(parsePublicWatchChat(text)).toBeNull();
});

it("refuses private, local, credentialed or token addresses, several addresses and unsupported cadence without creating", () => {
  const cases: [string, string][] = [
    ["http://localhost/status", "unsafe_url"], ["http://192.168.0.10/status", "unsafe_url"], ["http://127.0.0.1/s", "unsafe_url"],
    ["https://[::1]/s", "unsafe_url"], ["https://printer.local/s", "unsafe_url"], ["https://intranet/s", "unsafe_url"],
    ["https://user:pw@status.example.test/s", "unsafe_url"], ["https://status.example.test/s?token=abc", "unsafe_url"],
    ["https://status.example.test/s?x=abcdefghijklmnopqrstuvwxyz", "unsafe_url"], ["https://status.example.test/s#access_token=1", "unsafe_url"],
    ["https://status.example.test:8443/s", "unsafe_url"], ["ftp://status.example.test/s", "unsafe_url"],
  ];
  for (const [url, reason] of cases) {
    const parse = parsePublicWatchChat(`${url} 를 매시간 확인하고 바뀔 때만 알려줘`);
    if (url.startsWith("ftp")) { expect(parse).toBeNull(); continue; }
    expect(parse, url).toMatchObject({ ok: false, reason });
    expect(publicWatchChatReply(parse)).toContain("감시를 만들지 않았어요");
  }
  expect(parsePublicWatchChat(`${URL_} https://other.example.test/a 매시간 바뀌면 알려줘`)).toMatchObject({ ok: false, reason: "many_urls" });
  expect(parsePublicWatchChat(`${URL_} 를 10분마다 바뀌면 알려줘`)).toMatchObject({ ok: false, reason: "unsupported_interval" });
  expect(parsePublicWatchChat(`${URL_} 를 매시간 바뀌면 알려줘 200일 동안`)).toMatchObject({ ok: false, reason: "unsupported_duration" });
  expect(unsafePublicUrl(URL_)).toBeNull();
  expect(unsafePublicUrl("https://status.example.test/s?page=2&lang=ko")).toBeTruthy();
  expect(publicWatchChatReply(parsePublicWatchChat(PHRASE))).toBeNull();
  expect(publicWatchChatReply(null)).toBeNull();
});

it("posts the exact body once and keeps one request id across a lost response", async () => {
  const posts: any[] = [];
  vi.stubGlobal("fetch", vi.fn(async (url: string, init: RequestInit = {}) => {
    posts.push({ url, body: JSON.parse(String(init.body)) });
    if (posts.length === 1) throw new TypeError("synthetic lost response");
    const { request_id, thread_id, url: watched, every_seconds, days } = posts.at(-1).body;
    return Response.json({ id: request_id, thread_id, url: watched, every_seconds, days, state: "active", revision: 1, checks: [] });
  }));
  const done = vi.fn();
  render(<PublicWatchStart accountId="owner" threadId={THREAD} intent={intent()} onDone={done} onExpired={vi.fn()} />);
  expect(await screen.findByRole("alert")).toHaveTextContent("중복되지 않아요");
  expect(done).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "다시 시도" }));
  await waitFor(() => expect(done).toHaveBeenCalledTimes(1));
  expect(posts).toHaveLength(2);
  expect(posts[0].url).toBe(ROUTE);
  expect(posts[1].body).toEqual(posts[0].body);
  expect(posts[0].body).toEqual(toPublicWatchRequest(intent(), posts[0].body.request_id, THREAD));
  expect(Object.keys(posts[0].body).sort()).toEqual(["days", "every_seconds", "message", "request_id", "thread_id", "url"]);
  expect(posts[0].body).toMatchObject({ thread_id: THREAD, message: PHRASE, url: URL_, every_seconds: 3600, days: 7 });
  expect(screen.getByRole("status")).toHaveTextContent("시작했어요");
});

it("rejects a receipt for another thread instead of claiming success", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => Response.json({ id: "w", thread_id: "22222222-2222-4222-8222-222222222222", url: URL_, every_seconds: 3600, days: 7, state: "active", revision: 1, checks: [] })));
  const done = vi.fn();
  render(<PublicWatchStart accountId="owner" threadId={THREAD} intent={intent()} onDone={done} onExpired={vi.fn()} />);
  expect(await screen.findByRole("alert")).toBeInTheDocument();
  expect(done).not.toHaveBeenCalled();
});

const watch = (over: Partial<PublicWatchView> = {}): PublicWatchView => ({
  id: "w1", thread_id: THREAD, url: URL_, every_seconds: 3600, days: 7, state: "active", revision: 3,
  checks: [
    { due_at: 1800000000, state: "unchanged", excerpt: "모든 시스템 정상" },
    { due_at: 1800003600, state: "changed", excerpt: "점검 중" },
    { due_at: 1800007200, state: "failed" },
    { due_at: 1800010800, state: "unavailable", excerpt: "오래된 내용" },
  ], ...over,
});

it("lists this thread's worklog with source, unchanged/changed/failed/unavailable, and no extra UI otherwise", async () => {
  vi.stubGlobal("fetch", vi.fn(async (url: string) => {
    expect(url).toBe(`${ROUTE}?thread_id=${THREAD}`);
    return Response.json({ items: [watch(), watch({ id: "w2", state: "stopped", checks: [] }), watch({ id: "x", thread_id: "22222222-2222-4222-8222-222222222222" })] });
  }));
  render(<PublicWatchLog accountId="owner" threadId={THREAD} onExpired={vi.fn()} />);
  expect(await screen.findAllByText(URL_)).toHaveLength(2);
  expect(screen.getByText(/바뀐 게 없어요/)).toBeInTheDocument();
  expect(screen.getByText(/· 바뀌었어요/)).toBeInTheDocument();
  expect(screen.getByText(/확인하지 못했어요$/)).toBeInTheDocument();
  expect(screen.getByText(/지금 읽을 수 없어 확인하지 못했어요. 바뀐 것으로 보지 않아요/)).toBeInTheDocument();
  expect(screen.getByText("읽은 내용: 점검 중")).toBeInTheDocument();
  expect(screen.queryByText(/오래된 내용/)).toBeNull(); // nothing stale is shown for an unavailable check
  expect(screen.getAllByRole("article")).toHaveLength(2);
  expect(screen.getAllByRole("button", { name: "멈추기" })).toHaveLength(1);
  expect(screen.getByText(/중지됨/)).toBeInTheDocument();
});

it("renders nothing when the thread has no watch", async () => {
  const fetchMock = vi.fn(async () => Response.json({ items: [] }));
  vi.stubGlobal("fetch", fetchMock);
  const { container } = render(<PublicWatchLog accountId="owner" threadId={THREAD} onExpired={vi.fn()} />);
  await waitFor(() => expect(fetchMock).toHaveBeenCalled());
  expect(container).toBeEmptyDOMElement();
});

it("stops with the shown revision, reusing one request id after a lost response", async () => {
  let current = watch();
  const stops: any[] = [];
  vi.stubGlobal("fetch", vi.fn(async (url: string, init: RequestInit = {}) => {
    if (init.method === "POST") {
      stops.push({ url, body: JSON.parse(String(init.body)) });
      if (stops.length === 1) throw new TypeError("synthetic lost response");
      current = watch({ state: "stopped", revision: 4 });
      return Response.json(current);
    }
    return Response.json({ items: [current] });
  }));
  render(<PublicWatchLog accountId="owner" threadId={THREAD} onExpired={vi.fn()} />);
  const button = await screen.findByRole("button", { name: "멈추기" });
  fireEvent.click(button);
  expect(await screen.findByRole("alert")).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "멈추기" }));
  await waitFor(() => expect(screen.queryByRole("button", { name: "멈추기" })).toBeNull());
  expect(screen.getByText(/중지됨/)).toBeInTheDocument();
  expect(stops).toHaveLength(2);
  expect(stops[0].url).toBe(`${ROUTE}/w1/stop`);
  expect(stops[1].body).toEqual(stops[0].body);
  expect(stops[0].body).toEqual({ request_id: expect.any(String), revision: 3 });
});
