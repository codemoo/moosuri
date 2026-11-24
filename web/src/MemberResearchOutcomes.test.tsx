import { cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { MemberResearchOutcomes, formatMoney, parseOutcome, suggestOutcome, useResearchOutcomes, type Outcome } from "./MemberResearchOutcomes";

const outcome = (over: Partial<Outcome> = {}): Outcome => ({
  id: "o1", thread_id: "t1", kind: "shopping", title: "구매 비교 · 책상", message: "m", state: "ranked", revision: 1,
  criteria: { deadline: "2026-12-10", currency: "USD" },
  citations: [{ n: 1, kind: "browser", label: "Synthetic page", host: "shop.example", observed_at: 1796000000, status: "current", run_id: "run-1", request_id: null }],
  facts: [], missing: [], conflicts: [], timing: { need_by: "2026-12-10", delivery_by: "2026-12-09", margin_days: 1, deadline_in_days: 10 },
  draft: null, receipts: [], handoff: null, next: { kind: "none", text: "", url: null }, question: null, created_at: 1796000000,
  offers: [
    { seller: "Beta Mart", price: { currency: "USD", minor: 3800 }, shipping: { currency: "USD", minor: 0 }, total: { currency: "USD", minor: 3800 }, delivery_by: "2026-12-09", available: true, verdict: "best", reasons: [], cites: [1] },
    { seller: "Alpha Shop", price: { currency: "USD", minor: 4000 }, shipping: { currency: "USD", minor: 500 }, total: { currency: "USD", minor: 4500 }, delivery_by: "2026-12-12", available: true, verdict: "late", reasons: ["기한 2026-12-10 보다 늦음"], cites: [1] },
  ],
  ...over,
});
const page = (items: Outcome[], thread = "t1") => Response.json({ thread_id: thread, outcomes: items });

let calls: { url: string; method: string; body: any }[];
let route: (url: string, method: string, body: any) => Response | Promise<Response>;
const expired = vi.fn(), openBrowser = vi.fn(), browser = vi.fn(), readMail = vi.fn();
beforeEach(() => {
  calls = [];
  vi.stubGlobal("fetch", vi.fn(async (url: string, init: RequestInit = {}) => {
    const call = { url: url.replace("/api/v1", ""), method: init.method ?? "GET", body: init.body ? JSON.parse(String(init.body)) : null };
    calls.push(call);
    return route(call.url, call.method, call.body);
  }));
});
afterEach(() => { cleanup(); vi.unstubAllGlobals(); vi.clearAllMocks(); });

function Harness({ thread = "t1" }: { thread?: string }) {
  const api = useResearchOutcomes({ accountId: "member-a", thread, enabled: true, expired });
  return <>
    <button onClick={() => void api.create("shopping", "책상 비교", { deadline: "2026-12-10", currency: "USD" })}>만들기</button>
    <MemberResearchOutcomes api={api} onOpenBrowser={openBrowser} onBrowser={browser} onReadMail={readMail} />
  </>;
}
const posts = () => calls.filter(c => c.method === "POST");

it("shows the ranked offers with their citations and the best one marked, from one read", async () => {
  route = () => page([outcome()]);
  render(<Harness />);
  const card = await screen.findByRole("listitem", { name: "구매 비교 · 책상" });
  expect(within(card).getByText("가장 유리")).toBeVisible();
  expect(within(card).getByText(/기한 2026-12-10 보다 늦음/)).toBeVisible();
  expect(within(card).getAllByText("[1]").length).toBeGreaterThan(0);
  expect(within(card).getByText(/Synthetic page · shop.example/)).toBeVisible();
  expect(calls.filter(c => c.method === "GET")).toHaveLength(1);
  expect(calls[0].url).toBe("/personal/tasks/research-outcomes/t1");
});

it("asks for a URL once, posts the answer against the exact revision, then offers the existing browser", async () => {
  const asking = outcome({ state: "needs_input", offers: [], citations: [], timing: null, question: { id: "q1", text: "URL?" }, next: { kind: "ask_url", text: "URL을 알려 주세요", url: null } });
  const answered = outcome({ state: "needs_source", revision: 2, offers: [], citations: [], timing: null, next: { kind: "open_browser", text: "브라우저로 여세요", url: "https://shop.example/offers" } });
  route = (_u, method) => (method === "GET" ? page([asking]) : Response.json(answered));
  const user = userEvent.setup();
  render(<Harness />);
  await user.click(await screen.findByRole("button", { name: "URL 알려주기" }));
  await user.type(screen.getByLabelText("공개 페이지 URL"), "https://shop.example/offers");
  await user.click(screen.getByRole("button", { name: "알려주기" }));
  await waitFor(() => expect(posts()).toHaveLength(1));
  expect(posts()[0].url).toBe("/personal/tasks/research-outcomes/t1/control");
  expect(posts()[0].body).toMatchObject({ request_id: "o1", revision: 1, action: "answer", question_id: "q1", text: "https://shop.example/offers" });
  await user.click(await screen.findByRole("button", { name: "브라우저로 열기" }));
  expect(openBrowser).toHaveBeenCalledWith("https://shop.example/offers", "o1");
  expect(screen.queryByRole("button", { name: "URL 알려주기" })).toBeNull();
});

it("hands an interrupted shared tab to the existing browser instead of inventing a result", async () => {
  const handoff = { workspace_id: "ws-1", revision: 7, browser_state: "user_control", reason: null, action: "resume" as const };
  route = () => page([outcome({ state: "handoff", handoff, next: { kind: "resume_browser", text: "공유 브라우저가 중단되어 있습니다.", url: null } })]);
  const user = userEvent.setup();
  render(<Harness />);
  await user.click(await screen.findByRole("button", { name: "브라우저에서 이어서 진행" }));
  expect(browser).toHaveBeenCalledWith(handoff);
  expect(screen.getByText("브라우저 직접 조작 중")).toBeVisible();
});

it("keeps the draft apart from observed receipts and never calls an uncertain checkout paid", async () => {
  route = () => page([
    outcome({ id: "a1", kind: "application", title: "지원 준비 · 공고", offers: [], state: "draft_ready",
      draft: { state: "draft", submitted: false, title: "공고 지원서 초안", body: "초안 본문 [1]", cites: [1] } }),
    outcome({ id: "s1", title: "구매 비교 · 의자", state: "uncertain", offers: [], next: { kind: "confirm_manually", text: "판매처의 주문 내역에서 직접 확인하세요.", url: null },
      missing: [{ code: "outcome_unknown", text: "실제로 결제가 되었는지 알 수 없습니다." }] }),
    outcome({ id: "r1", title: "구매 비교 · 책장", state: "confirmation_observed", offers: [], receipts: [{ kind: "order_confirmation_observed", reference: "ORD-1234", host: "shop.example", observed_at: 1796000100, cite: 2, verified: "observed_on_page" }] }),
  ]);
  render(<Harness />);
  const draft = await screen.findByRole("region", { name: "지원서 초안" });
  expect(within(draft).getByText("(제출되지 않음)")).toBeVisible();
  const uncertain = screen.getByRole("listitem", { name: "구매 비교 · 의자" });
  expect(within(uncertain).getByText("결과를 확인할 수 없음")).toBeVisible();
  expect(within(uncertain).queryByText(/결제 완료/)).toBeNull();
  const receipt = within(screen.getByRole("listitem", { name: "구매 비교 · 책장" })).getByRole("region", { name: "관찰된 확인 번호" });
  expect(within(receipt).getByText(/판매처·기관·카드사에 확인된 것은 아닙니다/)).toBeVisible();
  expect(within(receipt).getByText(/ORD-1234/)).toBeVisible();
});

it("retries a lost create with the same request id and re-reads after a stale 409", async () => {
  let tries = 0;
  route = (_u, method, body) => {
    if (method === "GET") return page([]);
    tries += 1;
    if (tries === 1) return new Response("{}", { status: 503 });
    return Response.json(outcome({ id: body.request_id }));
  };
  const user = userEvent.setup();
  render(<Harness />);
  await user.click(await screen.findByRole("button", { name: "만들기" }));
  await waitFor(() => expect(posts()).toHaveLength(1));
  await user.click(screen.getByRole("button", { name: "만들기" }));
  await waitFor(() => expect(posts()).toHaveLength(2));
  expect(posts()[1].body.request_id).toBe(posts()[0].body.request_id);
  expect(await screen.findByRole("listitem", { name: "구매 비교 · 책상" })).toBeVisible();
  route = (_u, method) => (method === "GET" ? page([outcome({ revision: 3 })]) : new Response("{}", { status: 409 }));
  await user.click(screen.getByRole("button", { name: "다시 확인" }));
  expect(await screen.findByText(/상태가 바뀌었습니다/)).toBeVisible();
  await waitFor(() => expect(calls.filter(c => c.method === "GET").length).toBeGreaterThan(1));
});

it("renders a closed (withdrawn / auth changed) outcome with no evidence or actions", async () => {
  route = () => page([outcome({ state: "closed", offers: [], citations: [], timing: null, missing: [{ code: "authority_changed", text: "인증 상태가 바뀌어 닫았습니다." }] })]);
  render(<Harness />);
  expect(await screen.findByText("인증 상태가 바뀌어 닫았습니다.")).toBeVisible();
  expect(screen.queryByRole("table")).toBeNull();
  expect(screen.queryByRole("button", { name: "다시 확인" })).toBeNull();
});

it("ignores a page for another thread and refuses malformed or overclaiming payloads", async () => {
  route = () => page([outcome()], "other-thread");
  render(<Harness />);
  await waitFor(() => expect(calls).toHaveLength(1));
  expect(screen.queryByRole("listitem")).toBeNull();
  expect(parseOutcome({ ...outcome(), draft: { state: "draft", submitted: true, title: "x", body: "y", cites: [] } })).toBeNull();
  expect(parseOutcome({ ...outcome(), receipts: [{ kind: "order", reference: "R-1", verified: "bank_verified" }] })).toBeNull();
  expect(parseOutcome({ id: "x" })).toBeNull();
});

it("suggests an outcome from normal language without creating anything", () => {
  expect(suggestOutcome("12월 10일까지 도착하는 판매처 가격 비교해 줘")).toMatchObject({ kind: "shopping", criteria: { deadline: expect.stringMatching(/-12-10$/) } });
  expect(suggestOutcome("공모 공고와 내 메일을 비교해서 지원서 초안 써줘")?.kind).toBe("application");
  expect(suggestOutcome("오늘 날씨 어때?")).toBeNull();
  expect(formatMoney({ currency: "KRW", minor: 12300 })).toBe("12,300 KRW");
  expect(formatMoney({ currency: "USD", minor: 3800 })).toBe("38.00 USD");
  expect(calls).toHaveLength(0);
});


it("keeps a relative delivery deadline, year rollover and exclusive won budget from the request", () => {
  const today = new Date(2026, 11, 30, 12);
  expect(suggestOutcome("모레까지 배송되는 제품 가격을 비교해 줘. 5만원 미만", today)?.criteria)
    .toMatchObject({ deadline: "2027-01-01", budget_minor: 49999, currency: "KRW" });
  expect(suggestOutcome("다음 주 월요일까지 도착하는 곳 가격 비교해 줘", today)?.criteria.deadline).toBe("2027-01-04");
  expect(suggestOutcome("1월 5일까지 도착하는 곳 가격 비교해 줘", today)?.criteria.deadline).toBe("2027-01-05");
});

it("edits a dollar budget in dollars and cents and preserves the existing deadline", async () => {
  const current = outcome({ criteria: { budget_minor: 4599, currency: "USD", deadline: "2026-12-10" } });
  route = (_url, method) => method === "POST" ? Response.json(current) : page([current]);
  render(<Harness />);
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: "조건 변경" }));
  const budget = screen.getByLabelText("예산 (USD)");
  expect(budget).toHaveValue("45.99");
  await user.clear(budget);
  await user.type(budget, "42.50");
  await user.click(screen.getByRole("button", { name: "조건 적용" }));
  expect(posts()[0].body.criteria).toMatchObject({ budget_minor: 4250, currency: "USD", deadline: "2026-12-10" });
});
