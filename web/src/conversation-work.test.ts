import { expect, it } from "vitest";
import {
  compactMessage, eligibleActions, mergeGroup, parseGroup, parsePage, parseWorkControl, parseWorkItems, resolveControl,
  type ConversationTask, type ConversationWorkGroup,
} from "./conversation-work";

const task = (over: Partial<ConversationTask> = {}): ConversationTask => ({
  id: "t1", title: "일정 조사", scope: "conversation", query: "일정", filter: null, state: "completed", revision: 1,
  question_id: null, run_id: "run-1", runs: ["run-1"], events: [], source_count: 2, summary: "요약", report_id: null, ...over,
});
const group = (tasks: ConversationTask[], id = "g1"): ConversationWorkGroup => ({ id, thread_id: "th", turn_id: "turn", message: "m", tasks });

it("reads the natural two-part sentence as a conversation search and a memory search", () => {
  expect(parseWorkItems("지난 출장 대화에서 정해진 일정을 찾아주고, 내 기억에서 준비물도 찾아줘")).toEqual({
    kind: "items",
    items: [
      { scope: "conversation", query: "일정", title: "출장 일정 조사", filters: ["출장"] },
      { scope: "memory", query: "준비물", title: "준비물 조사" },
    ],
  });
});

it("reads the compact explicit form, keeps empty queries for a later question, and round-trips the message", () => {
  const parsed = parseWorkItems("대화 검색: 출장; 기억 검색: 준비물");
  expect(parsed).toMatchObject({ kind: "items", items: [{ scope: "conversation", query: "출장" }, { scope: "memory", query: "준비물" }] });
  if (parsed?.kind !== "items") throw new Error("expected items");
  expect(parseWorkItems(compactMessage(parsed.items))).toEqual(parsed);
  expect(parseWorkItems("대화 검색: ; 기억 검색: 준비물")).toMatchObject({ kind: "items", items: [{ query: "" }, { query: "준비물" }] });
});

it("leaves one-query searches on the existing route and never forwards a clause it cannot read", () => {
  expect(parseWorkItems("기억 검색: 준비물")).toBeNull();
  expect(parseWorkItems("내 기억에서 준비물 찾아줘")).toBeNull();
  expect(parseWorkItems("지난 출장 대화에서 일정을 찾아주고, 날씨도 알려줘")).toBeNull();
  expect(parseWorkItems("오늘 점심 뭐 먹을까, 추천해줘")).toBeNull();
  expect(parseWorkItems("대화 검색: 출장; 날씨 알려줘")).toMatchObject({ kind: "invalid" });
});

it("bounds the number of items and the query size", () => {
  const seven = Array.from({ length: 7 }, (_, i) => `기억 검색: 항목${i}`).join("; ");
  expect(parseWorkItems(seven)).toMatchObject({ kind: "invalid" });
  const six = Array.from({ length: 6 }, (_, i) => `기억 검색: 항목${i}`).join("; ");
  expect(parseWorkItems(six)).toMatchObject({ kind: "items" });
  expect(parseWorkItems(`대화 검색: ${"가".repeat(60)}; 기억 검색: x`)).toMatchObject({ kind: "invalid" });
});

it("recognizes only the supported follow-up phrases", () => {
  expect(parseWorkControl("준비물은 전자기기만")).toEqual({ kind: "refine", subject: "준비물", term: "전자기기" });
  expect(parseWorkControl("둘 다 어디까지 됐어?")).toEqual({ kind: "status" });
  expect(parseWorkControl("그거 멈춰")).toEqual({ kind: "cancel", all: false });
  expect(parseWorkControl("둘 다 멈춰")).toEqual({ kind: "cancel", all: true });
  expect(parseWorkControl("오늘 날씨 어때?")).toBeNull();
  expect(parseWorkControl("나는 사과만 먹어?")).toBeNull();
});

it("targets a refinement only when exactly one eligible child matches", () => {
  const prep = task({ id: "m1", title: "준비물 조사", scope: "memory", query: "준비물" });
  const plan = task({ id: "c1", query: "일정" });
  const one = resolveControl({ kind: "refine", subject: "준비물", term: "전자기기" }, [group([plan, prep])]);
  expect(one).toMatchObject({ kind: "act", action: "refine", text: "전자기기", targets: [{ task: { id: "m1" } }] });
  const two = resolveControl({ kind: "refine", subject: "준비물", term: "전자기기" }, [group([prep]), group([{ ...prep, id: "m2" }], "g0")]);
  expect(two).toMatchObject({ kind: "choose", action: "refine" });
  expect(resolveControl({ kind: "refine", subject: "배터리", term: "x" }, [group([plan, prep])])).toMatchObject({ kind: "clarify" });
  expect(resolveControl({ kind: "refine", subject: "준비물", term: "x" }, [])).toEqual({ kind: "pass" });
});

it("resolves stop and status requests without guessing", () => {
  const queued = task({ id: "q", state: "queued" });
  const done = task({ id: "d" });
  expect(resolveControl({ kind: "cancel", all: false }, [group([queued, done])])).toMatchObject({ kind: "act", targets: [{ task: { id: "q" } }] });
  expect(resolveControl({ kind: "cancel", all: false }, [group([queued, { ...queued, id: "q2" }])])).toMatchObject({ kind: "choose" });
  expect(resolveControl({ kind: "cancel", all: true }, [group([queued, { ...queued, id: "q2" }])])).toMatchObject({ kind: "act" });
  expect(resolveControl({ kind: "cancel", all: false }, [group([done])])).toMatchObject({ kind: "clarify" });
  expect(resolveControl({ kind: "status" }, [group([done])])).toMatchObject({ kind: "status" });
});

it("offers only the actions each state allows", () => {
  expect(eligibleActions(task({ state: "needs_input", question_id: "q1" }))).toEqual(["answer", "cancel"]);
  expect(eligibleActions(task({ state: "queued" }))).toEqual(["refine", "cancel"]);
  expect(eligibleActions(task())).toEqual(["refine", "reuse", "save_report"]);
  expect(eligibleActions(task({ report_id: "r" }))).toContain("open_report");
  expect(eligibleActions(task({ source_count: 0 }))).not.toContain("reuse");
  expect(eligibleActions(task({ state: "cancelled" }))).toEqual([]);
});

it("validates responses and never lets a late page lower a child revision", () => {
  expect(parseGroup({ id: "g", thread_id: "th", tasks: [{ id: "x", state: "weird" }] })).toBeNull();
  expect(parsePage({ thread_id: "th", groups: [group([task()])] })?.groups).toHaveLength(1);
  const newer = group([task({ revision: 4, state: "completed" })]);
  const older = group([task({ revision: 2, state: "queued" })]);
  expect(mergeGroup(newer, older).tasks[0]).toMatchObject({ revision: 4, state: "completed" });
});
