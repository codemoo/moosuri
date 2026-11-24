import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { ApiFailure } from "./api";
import { MemberCalendarWork, parseKoreanCalendarRequest } from "./MemberCalendarWork";

afterEach(cleanup);
// 2026-11-25 09:00 in Seoul. "내일" is 2026-11-26.
const NOW = new Date("2026-11-25T00:00:00Z");
const parse = (t: string, zone = "Asia/Seoul") => parseKoreanCalendarRequest(t, NOW, zone);
const conn = { connection_id: "00000000-0000-4000-8000-000000000002", epoch: 3 };
const REQUEST = "00000000-0000-4000-8000-000000000001";

it("parses an ordinary Korean create with an explicit end time", () => {
  expect(parse("내일 오후 3시부터 4시까지 '팀 회의' 잡아줘")).toEqual({
    ok: true, kind: "create", title: "팀 회의", date: "2026-11-26", time: "15:00", time_zone: "Asia/Seoul", duration_minutes: 60,
  });
  expect(parse("12월 3일 오전 10시 30분에 치과 1시간 반 예약해줘")).toMatchObject({
    ok: true, kind: "create", title: "치과", date: "2026-12-03", time: "10:30", duration_minutes: 90,
  });
});

it("does not read 3시간 as a clock time and keeps 30분 in a clock out of the duration", () => {
  expect(parse("내일 오후 3시 1시간 '회의' 추가해줘")).toMatchObject({ ok: true, time: "15:00", duration_minutes: 60 });
  expect(parse("내일 오후 3시 30분부터 1시간 반 점심 잡아줘")).toMatchObject({ ok: true, time: "15:30", duration_minutes: 90, title: "점심" });
});

it("asks instead of guessing a meridiem, date, duration, title or unsupported shape", () => {
  expect(parse("내일 3시에 팀 회의 1시간 잡아줘")).toMatchObject({ ok: false, reason: "meridiem" });
  expect(parse("오후 3시에 팀 회의 1시간 잡아줘")).toMatchObject({ ok: false, reason: "date" });
  expect(parse("내일 오후 3시에 팀 회의 잡아줘")).toMatchObject({ ok: false, reason: "duration" });
  expect(parse("내일 오후 3시 1시간 일정 추가해줘")).toMatchObject({ ok: false, reason: "title" });
  expect(parse("내일 오후 4시부터 3시까지 '회의' 잡아줘")).toMatchObject({ ok: false, reason: "end_before_start" });
  for (const text of ["팀원 초대해서 내일 오후 3시 1시간 회의 잡아줘", "매주 월요일 오후 3시 1시간 회의 잡아줘", "내일 종일 휴가 잡아줘", "내일 오후 3시 회의 취소해줘"])
    expect(parse(text)).toMatchObject({ ok: false, reason: "unsupported" });
  expect(parse("내일 날씨 알려줘")).toBeNull();
});

it("update wording: move, change, postpone all become one move that keeps the duration", () => {
  for (const verb of ["옮겨줘", "바꿔줘", "변경해줘", "미뤄줘"])
    expect(parse(`팀 회의를 모레 오후 4시로 ${verb}`)).toEqual({
      ok: true, kind: "move", title: "팀 회의", date: "2026-11-27", time: "16:00", time_zone: "Asia/Seoul",
    });
  expect(parse("내일 팀 회의 모레 오후 4시로 옮겨줘")).toMatchObject({ ok: true, kind: "move", title: "팀 회의", from_date: "2026-11-26", date: "2026-11-27" });
  expect(parse("다음 주 금요일 오전 9시로 '보고' 이동해줘")).toMatchObject({ ok: true, kind: "move", title: "보고", date: "2026-12-04", time: "09:00" });
});

it("uses the requested timezone, not only the browser's", () => {
  expect(parse("뉴욕 시간 기준 내일 오후 3시부터 4시까지 '미팅' 잡아줘", "Asia/Seoul")).toMatchObject({
    ok: true, time_zone: "America/New_York", date: "2026-11-25", time: "15:00",
  });
  expect(parse("Europe/London 기준 내일 오후 3시 1시간 '미팅' 잡아줘")).toMatchObject({ ok: true, time_zone: "Europe/London" });
  expect(parse("Mars/Base 기준 내일 오후 3시 1시간 '미팅' 잡아줘")).toMatchObject({ ok: false, reason: "zone" });
  expect(parse("내일 오후 3시 1시간 '미팅' 잡아줘", "America/Chicago")).toMatchObject({ ok: true, time_zone: "America/Chicago" });
});

function fakeScope(replies: unknown[]) {
  const calls: { path: string; method?: string; body: any }[] = [];
  const scope = {
    request: vi.fn(async (path: string, method?: string, body?: unknown) => {
      calls.push({ path, method, body });
      const next = replies.shift();
      if (next instanceof Error) throw next;
      return next;
    }),
  } as any;
  return { scope, calls };
}
const preview = (extra: object = {}) => ({
  state: "preview",
  preview: { kind: "create", digest: "a".repeat(64), title: "팀 회의", start: "2026-11-26T15:00:00+09:00", end: "2026-11-26T16:00:00+09:00", time_zone: "Asia/Seoul", current: null, ...extra },
});
const props = (scope: any, text: string, extra: object = {}) => ({ scope, text, connection: conn, onConnect: vi.fn(), now: NOW, zone: "Asia/Seoul", requestId: REQUEST, threadId: "dddddddd-dddd-4ddd-8ddd-dddddddddddd", ...extra });

it("create: shows a local preview first, then a confirmed provider receipt; no invitation field is sent", async () => {
  const { scope, calls } = fakeScope([
    preview(),
    { state: "confirmed", confirmed_by: "provider_response", receipt: "created", event: { id: "x", title: "팀 회의", start: "2026-11-26T15:00:00+09:00", end: "2026-11-26T16:00:00+09:00", time_zone: "Asia/Seoul" } },
  ]);
  render(<MemberCalendarWork {...props(scope, "내일 오후 3시부터 4시까지 '팀 회의' 잡아줘")} />);
  expect(await screen.findByText(/아직 Google 캘린더에 저장되지 않았습니다/)).toBeTruthy();
  expect(screen.queryByText("Google 캘린더에서 확인됨")).toBeNull();
  expect(calls).toHaveLength(1);
  expect(calls[0]).toMatchObject({ path: "/personal/google/calendar/prepare", method: "POST" });
  expect(calls[0].body).toEqual({
    request_id: REQUEST, thread_id: "dddddddd-dddd-4ddd-8ddd-dddddddddddd", connection_id: conn.connection_id, epoch: 3, kind: "create", title: "팀 회의", date: "2026-11-26", time: "15:00", time_zone: "Asia/Seoul", duration_minutes: 60,
  });
  fireEvent.click(screen.getByRole("button", { name: "Google 캘린더에 저장" }));
  expect(await screen.findByText("Google 캘린더에서 확인됨")).toBeTruthy();
  expect(screen.getByText(/Google의 응답으로 확인했습니다/)).toBeTruthy();
  expect(calls[1].path).toBe("/personal/google/calendar/apply");
  expect(calls[1].body.approved_digest).toBe("a".repeat(64));
  expect(calls[1].body.request.request_id).toBe(REQUEST);
  expect(JSON.stringify(calls)).not.toMatch(/attendee|invit/i);
});

it("move: two same-title candidates need an explicit choice; the chosen handle is what is applied", async () => {
  const cand = (ref: string, start: string, end: string) => ({ candidate_ref: ref, title: "팀 회의", start, end, time_zone: "Asia/Seoul" });
  const first = cand("1".repeat(32), "2026-11-26T10:00:00+09:00", "2026-11-26T11:30:00+09:00");
  const second = cand("2".repeat(32), "2026-11-26T14:00:00+09:00", "2026-11-26T15:00:00+09:00");
  const { scope, calls } = fakeScope([
    { state: "needs_selection", candidates: [first, second], partial: false },
    preview({ kind: "move", start: "2026-11-27T16:00:00+09:00", end: "2026-11-27T17:00:00+09:00", current: second }),
    { state: "confirmed", confirmed_by: "readback", receipt: "moved", event: { id: "y", title: "팀 회의", start: "2026-11-27T16:00:00+09:00", end: "2026-11-27T17:00:00+09:00", time_zone: "Asia/Seoul" } },
  ]);
  render(<MemberCalendarWork {...props(scope, "팀 회의를 모레 오후 4시로 옮겨줘")} />);
  expect(await screen.findByText(/옮길 일정을 직접 선택하세요/)).toBeTruthy();
  expect(screen.queryByRole("button", { name: "일정 옮기기" })).toBeNull();
  expect(calls[0].body.candidate_ref).toBeUndefined();
  fireEvent.click(screen.getByLabelText(/14:00/));
  expect(await screen.findByText(/현재: 2026-11-26 14:00/)).toBeTruthy();
  expect(calls[1].body.candidate_ref).toBe("2".repeat(32));
  fireEvent.click(screen.getByRole("button", { name: "일정 옮기기" }));
  expect(await screen.findByText("Google 캘린더에서 확인됨")).toBeTruthy();
  expect(screen.getByText(/다시 읽어 확인했습니다. 중복 생성되지 않았습니다/)).toBeTruthy();
  expect(calls[2].body.request.candidate_ref).toBe("2".repeat(32));
});

it("move: a single match applies with the handle carried in the preview", async () => {
  const one = { candidate_ref: "3".repeat(32), title: "보고", start: "2026-11-26T10:00:00+09:00", end: "2026-11-26T11:00:00+09:00", time_zone: "Asia/Seoul" };
  const { scope, calls } = fakeScope([preview({ kind: "move", current: one }), { state: "conflict" }]);
  render(<MemberCalendarWork {...props(scope, "'보고'를 모레 오후 4시로 옮겨줘")} />);
  fireEvent.click(await screen.findByRole("button", { name: "일정 옮기기" }));
  expect(await screen.findByText(/다른 곳에서 일정이 바뀌어 옮기지 않았습니다/)).toBeTruthy();
  expect(calls[1].body.request.candidate_ref).toBe("3".repeat(32));
});

it("needs_connection: no request is sent without a grant, and the existing consent UI is offered", async () => {
  const onConnect = vi.fn();
  const none = fakeScope([]);
  render(<MemberCalendarWork {...props(none.scope, "내일 오후 3시 1시간 '회의' 잡아줘", { connection: null, onConnect })} />);
  fireEvent.click(await screen.findByRole("button", { name: "Google 캘린더 연결" }));
  expect(onConnect).toHaveBeenCalledOnce();
  expect(none.calls).toHaveLength(0);
  cleanup();
  const refused = fakeScope([{ state: "needs_connection", reason: "calendar_permission_denied" }]);
  render(<MemberCalendarWork {...props(refused.scope, "내일 오후 3시 1시간 '회의' 잡아줘")} />);
  expect(await screen.findByRole("button", { name: "Google 캘린더 연결" })).toBeTruthy();
  expect(screen.getByText(/초대는 보내지 않습니다/)).toBeTruthy();
});

it("unknown outcome is never shown as saved, and retry reuses the same request and digest", async () => {
  const { scope, calls } = fakeScope([
    preview(),
    { state: "unknown", code: "readback_failed" },
    { state: "confirmed", confirmed_by: "readback", receipt: "created", event: { id: "x", title: "팀 회의", start: "2026-11-26T15:00:00+09:00", end: "2026-11-26T16:00:00+09:00", time_zone: "Asia/Seoul" } },
  ]);
  render(<MemberCalendarWork {...props(scope, "내일 오후 3시부터 4시까지 '팀 회의' 잡아줘")} />);
  fireEvent.click(await screen.findByRole("button", { name: "Google 캘린더에 저장" }));
  expect(await screen.findByText(/저장 결과를 아직 확인하지 못했습니다/)).toBeTruthy();
  expect(screen.queryByText("Google 캘린더에서 확인됨")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "다시 확인" }));
  await waitFor(() => expect(screen.getByText("Google 캘린더에서 확인됨")).toBeTruthy());
  expect(calls[2].body).toEqual(calls[1].body);
});

it("an edited approval or a stale connection is reported without claiming success", async () => {
  const { scope } = fakeScope([preview(), new ApiFailure(409, "approval_mismatch")]);
  render(<MemberCalendarWork {...props(scope, "내일 오후 3시부터 4시까지 '팀 회의' 잡아줘")} />);
  fireEvent.click(await screen.findByRole("button", { name: "Google 캘린더에 저장" }));
  expect(await screen.findByText(/승인한 내용과 달라졌습니다/)).toBeTruthy();
  expect(screen.queryByText("Google 캘린더에서 확인됨")).toBeNull();
});

it("renders nothing for a message that is not a calendar request and explains unsupported ones", () => {
  const { scope } = fakeScope([]);
  const { container } = render(<MemberCalendarWork {...props(scope, "오늘 점심 뭐 먹지")} />);
  expect(container.textContent).toBe("");
  cleanup();
  render(<MemberCalendarWork {...props(scope, "팀원 초대해서 내일 오후 3시 1시간 회의 잡아줘")} />);
  expect(screen.getByText(/초대, 삭제, 반복, 종일 일정은 지원하지 않습니다/)).toBeTruthy();
});
