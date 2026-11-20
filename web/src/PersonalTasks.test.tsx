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
  expect(await within(list).findByText("아직 맡긴 브라우저 작업이 없습니다.")).toBeVisible();
});
