import { render, screen, fireEvent, cleanup, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { MemberWorkMonitors, MonitorIntentCard, type MonitorView } from "./MemberWorkMonitors";
import { browserOffset, describeIntent, monitorChatReply, parseMonitorChat, resolveStopTarget, toMonitorRequest } from "./MemberWorkMonitorsChat";

afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

const ok = (text: string) => { const p = parseMonitorChat(text); if (!p.ok) throw new Error(`${text} -> ${p.reason}`); return p.intent; };
const why = (text: string) => { const p = parseMonitorChat(text); return p.ok ? "ok" : p.reason; };

it("parses straightforward Korean watches, briefs, list and stop requests", () => {
  expect(ok("‘서비스 상태’ 기억이 바뀌면 알려줘")).toEqual({ kind: "watch", scope: "memory", query: "서비스 상태", every_seconds: 3600, days: 7, assumed: ["days", "every_seconds"] });
  expect(ok("30분마다 ‘서비스 상태’ 기억이 바뀌면 알려줘 3일 동안")).toMatchObject({ kind: "watch", every_seconds: 1800, days: 3, assumed: [] });
  expect(ok("서비스 상태가 바뀌면 알려줘 한 시간마다")).toMatchObject({ kind: "watch", query: "서비스 상태", every_seconds: 3600 });
  expect(ok("매일 아침 9시에 ‘오늘 일정’ 브리핑 해줘")).toMatchObject({ kind: "brief", scope: "memory", query: "오늘 일정", at: "09:00", days: 7 });
  expect(ok("매일 오후 6시 30분에 대화 기록에서 ‘회의’ 브리핑해줘")).toMatchObject({ kind: "brief", scope: "conversation", query: "회의", at: "18:30" });
  expect(ok("매일 21:05에 ‘일정’ 브리핑해줘")).toMatchObject({ kind: "brief", at: "21:05" });
  expect(ok("‘준비물’에 변경 사항이 있을 때 알려줘")).toMatchObject({ kind: "watch", query: "준비물" });
  expect(ok("매일 ‘준비물’ 변경 사항을 알려줘")).toMatchObject({ kind: "watch", every_seconds: 86400 });
  expect(ok("내 감시 목록 보여줘")).toEqual({ kind: "list" });
  expect(ok("‘서비스 상태’ 감시 그만해줘")).toEqual({ kind: "stop", target: "서비스 상태" });
  expect(ok("서비스 상태 감시 멈춰줘")).toEqual({ kind: "stop", target: "서비스 상태" });
});

it("refuses unsupported sources, ambiguous clocks and unsupported cadences instead of guessing", () => {
  expect(why("‘서비스 상태’ 기억이 바뀌면 알려줘 메일도 같이 봐줘")).toBe("unsupported_source");
  expect(why("https://example.org 가 바뀌면 알려줘")).toBe("not_monitor");
  expect(why("매일 아침 캘린더 브리핑 해줘")).toBe("unsupported_source");
  expect(why("매일 9시에 ‘일정’ 브리핑해줘")).toBe("ambiguous_time");
  expect(why("매일 ‘일정’ 브리핑해줘")).toBe("needs_time");
  expect(why("매일 아침 9시에 브리핑해줘")).toBe("needs_query");
  expect(why("10분마다 ‘서비스 상태’ 바뀌면 알려줘")).toBe("unsupported_interval");
  expect(why("매일 아침 9시에 ‘서비스 상태’ 바뀌면 알려줘")).toBe("unsupported_interval");
  expect(why("‘서비스 상태’ 바뀌면 알려줘 200일 동안")).toBe("unsupported_duration");
  const refused = parseMonitorChat("https://example.org 가 바뀌면 알려줘");
  expect(monitorChatReply(refused)).toBeNull();
  // Ordinary chat and plain reminders pass through untouched.
  for (const text of ["아, 요일만 수요일로 바뀌었어요. 최종 요일과 시간을 다시 알려 주세요.", "회의 일정 변경 내용을 알려줘", "이 문서가 변경되면 어떻게 되는지 알려줘", "오늘 날씨 어때?", "회의 알림 취소해줘", "내일 오전 9시에 회의 알림 설정해줘", "이 문장은 바뀌면 안돼"]) {
    expect(why(text)).toBe("not_monitor");
    expect(monitorChatReply(parseMonitorChat(text))).toBeNull();
  }
});

it("builds exact request bodies, honest confirmations and unambiguous stop targets", () => {
  const watch = ok("30분마다 ‘서비스 상태’ 기억이 바뀌면 알려줘");
  expect(toMonitorRequest(watch, "r1", "Asia/Seoul")).toEqual({ request_id: "r1", kind: "watch", title: "서비스 상태 변화 감시", scope: "memory", query: "서비스 상태", every_seconds: 1800, days: 7 });
  expect(describeIntent(watch)).toMatch(/바뀐 경우에만.*메일, 캘린더는 읽지 않아요/);
  const brief = ok("매일 아침 9시에 ‘오늘 일정’ 브리핑 해줘");
  expect(toMonitorRequest(brief, "r2", "Asia/Seoul")).toEqual({ request_id: "r2", kind: "brief", title: "오늘 일정 브리핑", scope: "memory", query: "오늘 일정", at: "09:00", timezone: "Asia/Seoul", days: 7 });
  expect(describeIntent(brief)).toContain("메일과 캘린더는 읽지 않아요");
  expect(browserOffset(new Date("2026-01-01T00:00:00Z"))).toMatch(/^[+-]\d{2}:\d{2}$/);
  const mine = (id: string, query: string, state = "active"): MonitorView => ({ ...(view as any), id, query, title: `${query} 감시`, state });
  const all = [mine("a", "서비스 상태"), mine("b", "오늘 일정"), mine("c", "예전", "stopped")];
  expect(resolveStopTarget("일정", all)).toEqual({ found: "one", id: "b", revision: 1 });
  expect(resolveStopTarget("", all)).toMatchObject({ found: "many" });
  expect(resolveStopTarget("없는 것", all)).toEqual({ found: "none" });
});

const view = {
  id: "m1", kind: "watch", title: "서비스 상태 변화 감시", scope: "memory", query: "서비스 상태", state: "active",
  every_seconds: 1800, at: null, timezone: null, days: 7, revision: 1, created_at: 1, expires_at: 9999999999,
  next_due: 2000000000, last_checked_at: 1900000000, last_changed_at: 1900000000, alerts: 1,
  basis: "내가 저장한 기억만 확인합니다. 외부 사이트, 메일, 캘린더는 읽지 않습니다.",
  latest: { run_id: "r1", read_at: 1, limited: false, sources: [{ id: "a", revision: 1, available: true, title: "초록 수첩" }, { id: "b", revision: 1, available: false, title: null }] },
  recent: [{ due_at: 1, checked_at: 1, outcome: "changed", source_count: 2, skipped: 2, run_id: "r1", notified: true }],
};

it("lists status with live source references, never withdrawn text, and keeps one stop id across a lost response", async () => {
  const posts: any[] = [];
  vi.stubGlobal("fetch", vi.fn(async (url: string, init: RequestInit = {}) => {
    if (init.method === "POST") {
      posts.push({ url, body: JSON.parse(String(init.body)) });
      if (posts.length === 1) throw new TypeError("synthetic lost response");
      return Response.json({ ...view, state: "stopped", revision: 2 });
    }
    return Response.json({ items: [posts.length > 1 ? { ...view, state: "stopped", revision: 2 } : view], active_limit: 8 });
  }));
  render(<MemberWorkMonitors accountId="owner" onExpired={vi.fn()} />);
  await screen.findByText("서비스 상태 변화 감시");
  expect(screen.getByText("초록 수첩")).toBeInTheDocument();
  expect(screen.getByText("원본이 변경되었거나 더 이상 볼 수 없어요")).toBeInTheDocument();
  expect(screen.getByText(/외부 사이트, 메일, 캘린더는 읽지 않습니다/)).toBeInTheDocument();
  expect(screen.getByText(/앱이 꺼져 있던 2번은 묶어서 확인/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "멈추기" }));
  await screen.findByRole("alert");
  fireEvent.click(screen.getByRole("button", { name: "멈추기" }));
  await waitFor(() => expect(posts).toHaveLength(2));
  expect(posts[0].url).toBe("/api/v1/personal/tasks/monitors/m1/stop");
  expect(posts[1].body).toEqual(posts[0].body);
  expect(posts[0].body).toMatchObject({ revision: 1 });
  await screen.findByText(/중지됨/);
  expect(screen.queryByRole("button", { name: "멈추기" })).not.toBeInTheDocument();
});

it("confirms before creating and pins the request id until the server answers", async () => {
  const posts: any[] = [];
  vi.stubGlobal("fetch", vi.fn(async (_url: string, init: RequestInit = {}) => {
    posts.push(JSON.parse(String(init.body)));
    if (posts.length < 3) throw new TypeError("synthetic lost response");
    return Response.json(view);
  }));
  const done = vi.fn();
  render(<MonitorIntentCard accountId="owner" intent={ok("매일 아침 9시에 ‘오늘 일정’ 브리핑 해줘") as any} onDone={done} onCancel={vi.fn()} onExpired={vi.fn()} />);
  expect(posts).toHaveLength(0);
  expect(screen.getByText(/메일과 캘린더는 읽지 않아요/)).toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "시작" }));
  await screen.findByRole("alert");
  fireEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(posts).toHaveLength(2));
  expect(posts[1]).toEqual(posts[0]);
  expect(posts[0]).toMatchObject({ kind: "brief", at: "09:00", query: "오늘 일정" });
  expect(typeof posts[0].timezone).toBe("string");
  expect(posts[0].every_seconds).toBeUndefined();
  fireEvent.change(screen.getByLabelText("확인할 내용"), { target: { value: "내일 일정" } });
  fireEvent.click(screen.getByRole("button", { name: "시작" }));
  await waitFor(() => expect(done).toHaveBeenCalled());
  expect(posts[2].query).toBe("내일 일정");
  expect(posts[2].request_id).not.toBe(posts[0].request_id);
});
