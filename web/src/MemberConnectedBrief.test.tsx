import { render, screen, fireEvent, cleanup, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { ConnectedBriefCard, ConnectedBriefLog, type BriefView } from "./MemberConnectedBrief";
import { describeConnectedBrief, monitorChatReply, parseConnectedBriefChat, parseMonitorChat, toConnectedBriefRequest } from "./MemberWorkMonitorsChat";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

const PHRASE = "매일 아침 8시에 오늘 일정과 중요한 메일을 알려줘";
const intent = () => { const p = parseConnectedBriefChat(PHRASE); if (!p || !p.ok) throw new Error("not parsed"); return p.intent; };
const THREAD = "11111111-1111-4111-8111-111111111111";

it("parses the daily Gmail+Calendar request and leaves every existing monitor outcome unchanged", () => {
  expect(intent()).toEqual({ sources: ["gmail", "calendar"], at: "08:00", days: 7, interval_days: 1, assumed: ["days"], important_only: true });
  expect(parseConnectedBriefChat("매일 오후 6시 30분에 메일 요약해서 알려줘 3일 동안")).toMatchObject({ ok: true, intent: { sources: ["gmail"], at: "18:30", days: 3, important_only: false } });
  // Ordinary mail wording, or 중요 attached to something other than mail, keeps the ordinary recent-mail brief.
  expect(parseConnectedBriefChat("매일 아침 8시에 오늘 일정과 최근 메일을 알려줘")).toMatchObject({ ok: true, intent: { important_only: false } });
  expect(parseConnectedBriefChat("매일 아침 8시에 중요한 일정과 메일을 알려줘")).toMatchObject({ ok: true, intent: { important_only: false } });
  expect(parseConnectedBriefChat("매일 아침 8시에 중요한 일정을 알려줘")).toMatchObject({ ok: true, intent: { sources: ["calendar"], important_only: false } });
  // The member is shown what 중요 means before granting.
  expect(describeConnectedBrief(intent())).toMatch(/Gmail이 ‘중요’로 표시한 받은 메일.*IMPORTANT.*중요도를 따로 판단하지 않아요/);
  expect(describeConnectedBrief({ ...intent(), important_only: false })).not.toContain("IMPORTANT");
  expect(parseConnectedBriefChat("매일 8시에 오늘 일정 알려줘")).toMatchObject({ ok: false, reason: "ambiguous_time" });
  // Without a clock, local-only wording, quotes or no daily cadence this is not a connected brief.
  for (const text of ["매일 아침 캘린더 브리핑 해줘", "매일 아침 9시에 ‘오늘 일정’ 브리핑 해줘", "‘서비스 상태’ 기억이 바뀌면 알려줘 메일도 같이 봐줘", "오늘 일정 알려줘", "매일 아침 9시에 기억에서 메일 브리핑해줘"]) {
    expect(parseConnectedBriefChat(text)).toBeNull();
  }
  expect(parseMonitorChat("매일 아침 캘린더 브리핑 해줘")).toMatchObject({ ok: false, reason: "unsupported_source" });
  expect(parseMonitorChat("매일 아침 9시에 ‘오늘 일정’ 브리핑 해줘")).toMatchObject({ ok: true, intent: { kind: "brief", at: "09:00" } });
  // Where the card is not mounted the composer still gets an accurate reply instead of the old refusal.
  expect(monitorChatReply(parseMonitorChat(PHRASE))).toContain("읽기 전용");
  expect(describeConnectedBrief(intent())).toMatch(/읽기 전용.*보내거나.*바꾸지 않아요/);

  const pins = { gmail: { connection_id: "g", epoch: 2 }, calendar: { connection_id: "c", epoch: 3 } };
  expect(toConnectedBriefRequest(intent(), "r1", THREAD, "Asia/Seoul", pins)).toEqual({
    request_id: "r1", thread_id: THREAD, consent: true, gmail: pins.gmail, calendar: pins.calendar, at: "08:00", timezone: "Asia/Seoul", interval_days: 1, days: 7, important_only: true,
  });
  expect(toConnectedBriefRequest({ ...intent(), important_only: false }, "r1", THREAD, "Asia/Seoul", pins)).toMatchObject({ important_only: false });
  expect(toConnectedBriefRequest(intent(), "r1", THREAD, "Asia/Seoul", { gmail: pins.gmail })).toBeNull();
});

it("gives same-thread connection guidance without granting, then grants pinned to the shown connections with a stable request id", async () => {
  let calendar: unknown = { connection: null };
  const posts: any[] = [];
  vi.stubGlobal("fetch", vi.fn(async (url: string, init: RequestInit = {}) => {
    if (init.method === "POST") {
      posts.push({ url, body: JSON.parse(String(init.body)) });
      if (posts.length === 1) throw new TypeError("synthetic lost response");
      const { request_id, thread_id, at, timezone } = posts.at(-1).body;
      return Response.json({ id: request_id, thread_id, at, timezone, state: "active", sources: ["gmail", "calendar"],
        read_only: true, important_only: posts.at(-1).body.important_only, revision: 1, expires_at: 2000000000, runs: [] });
    }
    if (url.endsWith("/calendar/status")) return Response.json(calendar);
    return Response.json({ connection: { connection_id: "g1", epoch: 2, mailbox: "a@example.test" } });
  }));
  const done = vi.fn();
  const view = render(<ConnectedBriefCard accountId="owner" threadId={THREAD} intent={intent()} onDone={done} onCancel={vi.fn()} onExpired={vi.fn()} />);
  expect(await screen.findByText(/캘린더 연결이 필요해요.*브리핑도 만들어지지 않아요/)).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "읽기 전용으로 허용하고 시작" })).toBeDisabled();
  expect(posts).toHaveLength(0);
  view.unmount();

  calendar = { connection: { connection_id: "c1", epoch: 5, mailbox: "a@example.test" } };
  render(<ConnectedBriefCard accountId="owner" threadId={THREAD} intent={intent()} onDone={done} onCancel={vi.fn()} onExpired={vi.fn()} />);
  const button = screen.getByRole("button", { name: "읽기 전용으로 허용하고 시작" });
  await waitFor(() => expect(button).toBeEnabled());
  fireEvent.click(button);
  await screen.findByRole("alert");
  fireEvent.click(button);
  await waitFor(() => expect(done).toHaveBeenCalled());
  expect(posts[0].url).toBe("/api/v1/personal/tasks/briefs");
  expect(posts[1].body).toEqual(posts[0].body);
  expect(posts[0].body).toMatchObject({ important_only: true, consent: true, thread_id: THREAD, at: "08:00", gmail: { connection_id: "g1", epoch: 2 }, calendar: { connection_id: "c1", epoch: 5 } });
});

it("shows the in-thread worklog with live content, and only guidance (no private text) after a disconnect", async () => {
  const base = { thread_id: THREAD, sources: ["gmail", "calendar"], at: "08:00", timezone: "Asia/Seoul", interval_days: 1, created_at: 1, expires_at: 2000000000, read_only: true, important_only: false };
  const live: BriefView = {
    ...base, id: "b1", state: "active", end_reason: null, revision: 1, next_due: 1900000000,
    runs: [{ due_at: 1800000000, state: "completed", freshness: "live", guidance: null, skipped: 0, gmail_count: 1, calendar_count: 1, finished_at: 1800000100,
      detail: { captured_at: 1800000100, messages: [{ subject: "분기 보고서 검토", from: "a@example.test", received_at_ms: 1 }], events: [{ title: "주간 회의", start: "10:00", end: "11:00", all_day: false }] } }],
  } as BriefView;
  const gone: BriefView = {
    ...base, id: "b2", state: "revoked", end_reason: "source_changed", revision: 2, next_due: null,
    runs: [{ due_at: 1700000000, state: "withdrawn", freshness: null, guidance: null, skipped: 0, gmail_count: 1, calendar_count: 1, finished_at: 1, detail: null }],
  } as BriefView;
  vi.stubGlobal("fetch", vi.fn(async () => Response.json({ items: [live, gone, { ...live, id: "other", thread_id: "22222222-2222-4222-8222-222222222222" }], active_limit: 4 })));
  render(<ConnectedBriefLog accountId="owner" threadId={THREAD} onExpired={vi.fn()} />);
  expect(await screen.findByText("분기 보고서 검토 — a@example.test")).toBeInTheDocument();
  expect(screen.getByText(/주간 회의 · 10:00 ~ 11:00/)).toBeInTheDocument();
  expect(screen.getByText(/브리핑 완료 · 메일 1건 · 일정 1건/)).toBeInTheDocument();
  expect(screen.getByText(/Gmail·캘린더 연결이 필요해요.*이전에 읽은 내용은 지웠어요/)).toBeInTheDocument();
  expect(screen.getByText(/연결이 바뀌어 읽은 내용을 지웠어요/)).toBeInTheDocument();
  // Only this thread's briefs are shown, and only the active one can be stopped.
  expect(screen.getAllByRole("article")).toHaveLength(2);
  expect(screen.queryByText(/IMPORTANT/)).toBeNull();
  expect(screen.getAllByRole("button", { name: "멈추기" })).toHaveLength(1);
});
