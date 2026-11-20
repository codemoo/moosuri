import { render, screen, within, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, expect, it, vi } from "vitest";
import { PersonalTasks, type TaskDetail, type TaskSummary } from "./PersonalTasks";

const first: TaskSummary = {
  id: "run-one", thread_id: "thread-one", title: "야간 운행 확인", created_at: 1700500000,
  state: "completed", next_actor: "none",
};
const second = { ...first, id: "run-two", title: "오전 운행 확인" };
const detail: TaskDetail = {
  task: second, request: "오전 시간표의 첫 출발을 확인해 줘", thread_title: "합성 시간표",
  result: "오전 8시 15분에 출발합니다.", observation: null, reason: null,
  history: [{state:"starting", at:1700500000},{state:"completed",at:null}],
};
const props = {accountId:"member-a",onExpired:vi.fn(),onThread:vi.fn(),onBack:vi.fn(),onOpenTask:vi.fn()};
afterEach(() => { vi.unstubAllGlobals(); vi.clearAllMocks(); });

it("finds an older task, reads the receipt and history, and returns to the same list page", async () => {
  const fetcher = vi.fn(async (url: string, init: RequestInit) => {
    expect(new Headers(init.headers).get("x-moosuri-account")).toBe("member-a");
    if (url.endsWith("/run-two")) return Response.json(detail);
    if (url.endsWith("?before=42")) return Response.json({items:[second],next_before:null});
    return Response.json({items:[first],next_before:"42"});
  });
  vi.stubGlobal("fetch",fetcher);
  function Workspace() {
    const [id,setId] = useState<string>();
    return <PersonalTasks {...props} taskId={id} onOpenTask={setId} onBack={() => setId(undefined)} />;
  }
  const user = userEvent.setup();
  render(<Workspace />);
  await user.click(await screen.findByRole("button",{name:"이전 작업 더 보기"}));
  await user.click(await screen.findByRole("button",{name:/오전 운행 확인/}));
  expect(await screen.findByText(detail.result!)).toBeVisible();
  expect(screen.getByText("시각 미기록")).toBeVisible();
  await user.click(screen.getByRole("button",{name:/원래 대화 열기/}));
  expect(props.onThread).toHaveBeenCalledWith("thread-one","run-two");
  await user.click(screen.getByRole("button",{name:"맡긴 일 목록"}));
  expect(screen.getByRole("button",{name:/오전 운행 확인/})).toBeVisible();
  expect(fetcher).toHaveBeenCalledTimes(3);
});

it("opens a waiting task directly, distinguishes observations, and refreshes its final answer", async () => {
  const fetcher = vi.fn()
    .mockResolvedValueOnce(Response.json({...detail,task:{...second,state:"ready",next_actor:"member"},result:null,observation:"주말 시간표를 선택하세요."}))
    .mockResolvedValueOnce(Response.json(detail));
  vi.stubGlobal("fetch",fetcher);
  const user = userEvent.setup();
  render(<PersonalTasks {...props} taskId="run-two" />);
  expect(await screen.findByText("주말 시간표를 선택하세요.")).toBeVisible();
  expect(screen.getByText(/원래 대화에서 페이지를 확인/)).toBeVisible();
  expect(screen.queryByRole("heading",{name:"작업 결과"})).toBeNull();
  await user.click(screen.getByRole("button",{name:"상태 새로고침"}));
  expect(await screen.findByText(detail.result!)).toBeVisible();
  expect(fetcher.mock.calls.every(([url]) => url.endsWith("/personal/tasks/run-two"))).toBe(true);
});

it("keeps older rows during pagination failure and retries the same cursor", async () => {
  const fetcher = vi.fn()
    .mockResolvedValueOnce(Response.json({items:[first],next_before:"22"}))
    .mockResolvedValueOnce(Response.json({}, {status:503}))
    .mockResolvedValueOnce(Response.json({items:[second],next_before:null}));
  vi.stubGlobal("fetch",fetcher);
  const user = userEvent.setup();
  render(<PersonalTasks {...props} />);
  await user.click(await screen.findByRole("button",{name:"이전 작업 더 보기"}));
  expect(await screen.findByRole("alert")).toBeVisible();
  expect(screen.getByRole("button",{name:/야간 운행 확인/})).toBeVisible();
  await user.click(screen.getByRole("button",{name:"다시 시도"}));
  expect(await screen.findByRole("button",{name:/오전 운행 확인/})).toBeVisible();
  expect(fetcher.mock.calls.slice(1).map(([path]) => path)).toEqual(Array(2).fill("/api/v1/personal/tasks?before=22"));
});

it("marks a failed step and shows the reason in its own worklog", async () => {
  const failed = { ...detail, task: { ...second, kind: "text", state: "failed" }, result: null,
    history: [{state: "starting", at: first.created_at}, {state: "failed", at: first.created_at, detail: "응답을 받지 못했습니다."}] };
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(Response.json(failed)));
  const user = userEvent.setup();
  render(<PersonalTasks {...props} taskId="run-two"/>);
  const step = await screen.findByRole("button", {name: /실패/});
  expect(step.querySelector(".task-marker")).toHaveTextContent("!");
  await user.click(step);
  expect(screen.getByRole("region", {name: "단계 상세"})).toHaveTextContent("응답을 받지 못했습니다.");
});

it("drops the old account's in-flight detail on a member change and distinguishes missing from empty", async () => {
  let finish!: (value: Response) => void;
  const fetcher = vi.fn()
    .mockReturnValueOnce(new Promise<Response>(resolve => { finish = resolve; }))
    .mockResolvedValueOnce(Response.json({}, {status:404}))
    .mockResolvedValueOnce(Response.json({items:[],next_before:null}));
  vi.stubGlobal("fetch",fetcher);
  const {rerender} = render(<PersonalTasks {...props} taskId="run-two" />);
  rerender(<PersonalTasks {...props} accountId="member-b" taskId="run-two" />);
  expect(await screen.findByRole("alert")).toHaveTextContent("이 작업을 찾을 수 없습니다.");
  finish(Response.json(detail));
  await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(2));
  expect(screen.queryByText(detail.result!)).toBeNull();
  rerender(<PersonalTasks {...props} accountId="member-b" />);
  const list = screen.getByRole("region",{name:"맡긴 일 목록"});
  expect(await within(list).findByText("아직 맡긴 일이 없습니다.")).toBeVisible();
});

it("opens a source step with its own evidence and restores the timeline control", async()=>{
  vi.stubGlobal("fetch",vi.fn().mockResolvedValue(Response.json({...detail,task:{...second,kind:"research"},history:[
    {state:"searched",at:1700500000,title:"내 기억에서 검색",detail:"검색어 수첩"},
    {state:"source_read",at:1700500001,title:"준비 기록",detail:"합성 초록 수첩"},
  ]})));
  const user=userEvent.setup();
  render(<PersonalTasks {...props} taskId="run-two" />);
  const source=await screen.findByRole("button",{name:/준비 기록/});
  await user.click(source);
  expect(within(screen.getByRole("region",{name:"단계 상세"})).getByText("합성 초록 수첩")).toBeVisible();
  await user.click(screen.getByRole("button",{name:"진행 기록으로"}));
  await waitFor(()=>expect(source).toHaveFocus());
});

it("shows three scheduled previews, opens all five, and renders history by local date", async()=>{
  const today=new Date();today.setHours(12,0,0,0);const yesterday=new Date(today);yesterday.setDate(today.getDate()-1);
  const schedules=Array.from({length:5},(_,i)=>({id:String(i),title:`예약 ${i+1}`,scope:"memory",query:"수첩",period:1800,next_due:null,enabled:false,revision:1}));
  vi.stubGlobal("fetch",vi.fn().mockResolvedValue(Response.json({items:[{...first,created_at:today.getTime()/1000},{...second,created_at:yesterday.getTime()/1000}],next_before:null,schedules})));
  const user=userEvent.setup();render(<PersonalTasks {...props}/>);
  expect(await screen.findByRole("heading",{name:"오늘"})).toBeVisible();expect(screen.getByRole("heading",{name:"어제"})).toBeVisible();
  const olderDay = screen.getByRole("heading", {name:"어제"});
  await user.click(olderDay);
  expect(screen.getByText(second.title)).not.toBeVisible();
  await user.click(olderDay);
  expect(screen.getByRole("button", {name:/오전 운행 확인/})).toBeVisible();
  expect(screen.queryByRole("button",{name:/예약 5 /})).toBeNull();
  await user.click(screen.getByRole("button",{name:"예약 작업 모두 보기"}));
  await user.click(screen.getByRole("button",{name:/예약 5/}));
  expect(screen.getByRole("button",{name:"예약 재개"})).toBeVisible();
});

it("retries a lost manual research response with the same request identity", async()=>{
  const actions:Record<string,string>[]=[];
  vi.stubGlobal("fetch",vi.fn(async(url:string,init:RequestInit)=>{
    if(url.endsWith("/workflows")){
      actions.push(JSON.parse(String(init.body)));
      return actions.length===1?Response.json({}, {status:503}):Response.json({workflow_id:actions[0].request_id,run_id:"result-1"});
    }
    return Response.json({items:[],schedules:[],next_before:null});
  }));
  const user=userEvent.setup();render(<PersonalTasks {...props}/>);
  await user.click(screen.getByRole("button",{name:"자료 조사 맡기기"}));
  await user.type(screen.getByLabelText("검색어"),"수첩");
  await user.click(screen.getByRole("button",{name:"조사 실행"}));
  expect(await screen.findByRole("alert")).toBeVisible();
  await user.click(screen.getByRole("button",{name:"조사 실행"}));
  await waitFor(()=>expect(props.onOpenTask).toHaveBeenCalledWith("result-1"));
  expect(actions).toHaveLength(2);expect(actions[1]).toEqual(actions[0]);
});

it("keeps a schedule's history and cursor through retry without mixing in another assignment", async () => {
  const plan = { id: "plan-one", title: "수첩 점검", scope: "memory", query: "수첩", period: 1800,
    next_due: null, enabled: false, revision: 2 };
  const run = { ...first, kind: "research", thread_id: "", schedule_id: plan.id, title: "수첩 점검" };
  const attempts: string[] = [];
  vi.stubGlobal("fetch", vi.fn(async (url: string) => {
    if (url.endsWith("/personal/tasks")) return Response.json({ items: [second], schedules: [plan], next_before: null });
    if (url.endsWith("/workflows/plan-one")) return Response.json({ items: [run], schedules: [], next_before: "42" });
    attempts.push(url);
    if (attempts.length === 1) return Response.json({}, {status: 503});
    return Response.json({ items: [{...run, id: "older", scheduled_at: first.created_at}], schedules: [], next_before: null });
  }));
  const user = userEvent.setup();
  render(<PersonalTasks {...props}/>);
  await user.click(await screen.findByRole("button", {name: /수첩 점검/}));
  await user.click(screen.getByRole("button", {name: "실행 기록 보기"}));
  expect(await screen.findByRole("heading", {name: "수첩 점검 실행 기록"})).toBeVisible();
  expect(screen.queryByText(second.title)).toBeNull();
  await user.click(await screen.findByRole("button", {name: "이전 작업 더 보기"}));
  await user.click(await screen.findByRole("button", {name: "다시 시도"}));
  expect(await screen.findByRole("button", {name: /예약 실행/})).toBeVisible();
  expect(screen.getByRole("button", {name: /직접 실행/})).toBeVisible();
  expect(attempts).toEqual(Array(2).fill("/api/v1/personal/tasks/workflows/plan-one?before=42"));
  await user.click(screen.getByRole("button", {name: "전체 맡긴 일"}));
  expect(await screen.findByText(second.title)).toBeVisible();
});

it("identifies scheduled, manual and chat occurrences and keeps the current preview on return", async () => {
  const research: TaskSummary = { ...first, kind: "research", thread_id: "", preview: "기억 1건 확인 · 초록 수첩" };
  const scheduled = { ...research, id: "scheduled", scheduled_at: first.created_at - 600 };
  const chat = { ...research, id: "chat", thread_id: first.thread_id };
  const withdrawn = { ...scheduled, preview: "기억 1건 확인" };
  vi.stubGlobal("fetch", vi.fn(async (url: string) => Response.json(
    url.endsWith("/scheduled") ? { ...detail, task: withdrawn } : { items: [scheduled, research, chat], next_before: null }
  )));
  function Workspace() {
    const [id, setId] = useState<string>();
    return <PersonalTasks {...props} taskId={id} onOpenTask={setId} onBack={() => setId(undefined)} />;
  }
  const user = userEvent.setup();
  render(<Workspace />);
  const row = await screen.findByRole("button", { name: /예약 실행/ });
  expect(row).toHaveTextContent("초록 수첩");
  expect(screen.getByRole("button", { name: /직접 실행/ })).toBeVisible();
  expect(screen.getByRole("button", { name: /대화에서 실행/ })).toBeVisible();
  await user.click(row);
  expect(await screen.findByRole("heading", { name: "예약 회차" })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "맡긴 일 목록" }));
  expect(row).toHaveFocus();
  expect(row).toHaveTextContent("기억 1건 확인");
  expect(row).not.toHaveTextContent("초록 수첩");
});
