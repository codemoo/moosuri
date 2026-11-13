import {render,screen,waitFor,fireEvent,cleanup} from "@testing-library/react";
import {afterEach,describe,it,expect,vi} from "vitest";
import {Missions} from "./Missions";
import type {MissionView} from "../shared/api-types";
const mission:MissionView={id:"m",revision:1,state:"paused",request:{request_id:"r",task:"Synthetic task",constraints:"Keep original",checks:[{kind:"user_review",description:"Owner review"}],execution_mode:"durable"},steering:[],plan:["Find evidence","Verify"],completed_steps:1,receipts:[],result:null,report:[],uncertain:false,expires_at_ms:0};
afterEach(()=>{cleanup();vi.unstubAllGlobals()});
it.each(["", "배송 조건도 모두 확인"])("keeps prose conditions reviewable without an extra approval prompt: %s", async constraints=>{
 const requests:any[]=[];
 vi.stubGlobal("fetch",vi.fn(async(_url,init)=>{
  if(init?.method==="POST"){requests.push(JSON.parse(String(init.body)));return new Response(JSON.stringify(mission))}
  return new Response(JSON.stringify([{...mission,state:"partial",report:["Additional constraints need owner review"]}]));
 }));
 render(<Missions accountId="owner" onExpired={vi.fn()} back={vi.fn()}/>);
 await screen.findByText("추가 조건은 직접 검토가 필요해요.");
 fireEvent.change(screen.getByLabelText("맡길 일"),{target:{value:"정확한 합계 계산"}});
 fireEvent.change(screen.getByLabelText("지켜야 할 조건"),{target:{value:constraints}});
 fireEvent.click(screen.getByText("계산 결과를 정확히 확인할 때"));
 fireEvent.change(screen.getByLabelText("예상하는 정확한 값"),{target:{value:"0.3"}});
 fireEvent.click(screen.getByRole("button",{name:"일 맡기기"}));
 await waitFor(()=>expect(requests).toHaveLength(1));
 expect(requests[0].checks.map((c:any)=>c.kind)).toEqual(constraints?["decimal_equals","user_review"]:["decimal_equals"]);
 expect(requests[0].constraints).toBe(constraints);
});
describe("Missions",()=>{
 it("submits an explicit durable request and shows review as incomplete",async()=>{const requests:unknown[]=[];vi.stubGlobal("fetch",vi.fn(async(_url,init)=>{if(init?.method==="POST"){requests.push(JSON.parse(init.body));return new Response(JSON.stringify(mission),{status:200})}return new Response(JSON.stringify([{...mission,state:"partial",result:{text:"Synthetic partial result",evidence:[]},report:["Owner review needed"]}]),{status:200})}));render(<Missions accountId="owner" onExpired={vi.fn()} back={vi.fn()}/>);await screen.findByText("Synthetic partial result");expect(screen.getByText("일부 결과 · 검토 필요")).toBeTruthy();fireEvent.change(screen.getByLabelText("맡길 일"),{target:{value:"Research synthetic options"}});fireEvent.click(screen.getByRole("button",{name:"일 맡기기"}));await waitFor(()=>expect(requests).toHaveLength(1));expect(requests[0]).toMatchObject({task:"Research synthetic options",execution_mode:"durable",checks:[{kind:"user_review"}]});});
 it("does not offer resume for a committed unknown dispatch",async()=>{vi.stubGlobal("fetch",vi.fn(async()=>new Response(JSON.stringify([{...mission,uncertain:true}]),{status:200})));
 render(<Missions accountId="owner" onExpired={vi.fn()} back={vi.fn()}/>);await screen.findByText("Synthetic task");expect(screen.queryByRole("button",{name:"이어서 진행"})).toBeNull();expect(screen.getByText(/이미 시작한 실행/)).toBeTruthy();});
 it("fences a late list when account switches",async()=>{let resolve!:(v:Response)=>void;const late=new Promise<Response>(r=>resolve=r);vi.stubGlobal("fetch",vi.fn(async(_url,init)=>init.headers["X-Moosuri-Account"]==="first"?late:new Response("[]",{status:200})));const {rerender}=render(<Missions accountId="first" onExpired={vi.fn()} back={vi.fn()}/>);rerender(<Missions accountId="second" onExpired={vi.fn()} back={vi.fn()}/>);resolve(new Response(JSON.stringify([mission]),{status:200}));await waitFor(()=>expect(screen.queryByText("Synthetic task")).toBeNull());});
});

it("sends current cancel after a committed resume loses its ACK and polling observes the new revision", async()=>{
 const commands:any[]=[];let item={...mission,state:"paused",revision:2};
 vi.stubGlobal("fetch",vi.fn(async(_url,init)=>{
  if(init?.method==="POST"){const body=JSON.parse(String(init.body));commands.push(body);if(commands.length===1){item={...item,state:"queued",revision:3};throw new TypeError("synthetic lost committed ACK")}item={...item,state:"cancelled",revision:4};return new Response(JSON.stringify(item))}
  return new Response(JSON.stringify([item]));
 }));
 render(<Missions accountId="owner" onExpired={vi.fn()} back={vi.fn()}/>);
 fireEvent.click(await screen.findByRole("button",{name:"이어서 진행"}));await screen.findByRole("alert");
 await new Promise(resolve=>setTimeout(resolve,2600));
 fireEvent.click(await screen.findByRole("button",{name:"취소"}));await waitFor(()=>expect(commands).toHaveLength(2));
 expect(commands[1]).toMatchObject({action:{kind:"cancel"},expected_revision:3});expect(commands[1].request_id).not.toBe(commands[0].request_id);await screen.findByText("취소됨");
});
it("retries only the same uncertain mission/action with its exact UUID and payload", async()=>{
 const commands:any[]=[];let item={...mission,state:"paused",revision:2};
 vi.stubGlobal("fetch",vi.fn(async(_url,init)=>{if(init?.method==="POST"){commands.push(JSON.parse(String(init.body)));if(commands.length===1)throw new TypeError("synthetic unacknowledged request");item={...item,state:"queued",revision:3};return new Response(JSON.stringify(item))}return new Response(JSON.stringify([item]))}));
 render(<Missions accountId="owner" onExpired={vi.fn()} back={vi.fn()}/>);fireEvent.click(await screen.findByRole("button",{name:"이어서 진행"}));await screen.findByRole("alert");fireEvent.click(screen.getByRole("button",{name:"이어서 진행"}));
 await waitFor(()=>expect(commands).toHaveLength(2));expect(commands[1]).toEqual(commands[0]);await screen.findByText("차례를 기다리는 중");
});
it("never sends a previous card's uncertain resume for a new card cancellation", async()=>{
 const requests:{url:string;body:any}[]=[];let items=[{...mission,id:"first",state:"paused",revision:2}];
 vi.stubGlobal("fetch",vi.fn(async(url,init)=>{if(init?.method==="POST"){const body=JSON.parse(String(init.body));requests.push({url:String(url),body});if(requests.length===1){items=[{...mission,id:"first",state:"achieved",revision:3},{...mission,id:"second",state:"paused",revision:1,request:{...mission.request,task:"Synthetic second task"}}];throw new TypeError("synthetic first card lost ACK")}items=items.map(m=>m.id==="second"?{...m,state:"cancelled",revision:2}:m);return new Response(JSON.stringify(items[1]))}return new Response(JSON.stringify(items))}));
 render(<Missions accountId="owner" onExpired={vi.fn()} back={vi.fn()}/>);fireEvent.click(await screen.findByRole("button",{name:"이어서 진행"}));await screen.findByRole("alert");fireEvent.click(screen.getByRole("button",{name:"상태 새로고침"}));await screen.findByText("Synthetic second task");fireEvent.click(screen.getByRole("button",{name:"취소"}));
 await waitFor(()=>expect(requests).toHaveLength(2));expect(requests[1].url).toBe("/api/v1/missions/second/control");expect(requests[1].body).toMatchObject({action:{kind:"cancel"},expected_revision:1});expect(requests[1].body.request_id).not.toBe(requests[0].body.request_id);
});
it("reconciles a delayed old resume winning CAS and retries the requested cancel, never resume", async()=>{
 const commands:any[]=[];let item={...mission,state:"paused",revision:2};
 vi.stubGlobal("fetch",vi.fn(async(_url,init)=>{if(init?.method==="POST"){const body=JSON.parse(String(init.body));commands.push(body);if(commands.length===1)throw new TypeError("synthetic old resume still ambiguous");if(commands.length===2){item={...item,state:"queued",revision:3};return new Response(JSON.stringify({code:"conflict"}),{status:409})}item={...item,state:"cancelled",revision:4};return new Response(JSON.stringify(item))}return new Response(JSON.stringify([item]))}));
 render(<Missions accountId="owner" onExpired={vi.fn()} back={vi.fn()}/>);fireEvent.click(await screen.findByRole("button",{name:"이어서 진행"}));await screen.findByRole("alert");fireEvent.click(screen.getByRole("button",{name:"취소"}));await screen.findByText("취소됨");
 expect(commands.map(c=>c.action.kind)).toEqual(["resume","cancel","cancel"]);expect(commands.map(c=>c.expected_revision)).toEqual([2,2,3]);expect(new Set(commands.map(c=>c.request_id)).size).toBe(3);
});
it("clears a known rejected control rather than replaying its UUID", async()=>{
 const commands:any[]=[];let item={...mission,state:"paused",revision:2};
 vi.stubGlobal("fetch",vi.fn(async(_url,init)=>{if(init?.method==="POST"){commands.push(JSON.parse(String(init.body)));if(commands.length===1)return new Response(JSON.stringify({code:"invalid_input"}),{status:400});item={...item,state:"queued",revision:3};return new Response(JSON.stringify(item))}return new Response(JSON.stringify([item]))}));
 render(<Missions accountId="owner" onExpired={vi.fn()} back={vi.fn()}/>);fireEvent.click(await screen.findByRole("button",{name:"이어서 진행"}));await screen.findByRole("alert");fireEvent.click(screen.getByRole("button",{name:"이어서 진행"}));await screen.findByText("차례를 기다리는 중");expect(commands).toHaveLength(2);expect(commands[1].request_id).not.toBe(commands[0].request_id);
});
it("a changed steering instruction is a new intent rather than a retry of the old payload", async()=>{
 const commands:any[]=[];let item={...mission,state:"paused",revision:2};
 vi.stubGlobal("fetch",vi.fn(async(_url,init)=>{if(init?.method==="POST"){commands.push(JSON.parse(String(init.body)));if(commands.length===1)throw new TypeError("synthetic steering ACK lost");item={...item,state:"queued",revision:3};return new Response(JSON.stringify(item))}return new Response(JSON.stringify([item]))}));
 render(<Missions accountId="owner" onExpired={vi.fn()} back={vi.fn()}/>);const input=await screen.findByLabelText("추가 지시");fireEvent.change(input,{target:{value:"Synthetic first instruction"}});fireEvent.click(screen.getByRole("button",{name:"추가 지시하고 진행"}));await screen.findByRole("alert");fireEvent.change(input,{target:{value:"Synthetic new instruction"}});fireEvent.click(screen.getByRole("button",{name:"추가 지시하고 진행"}));await screen.findByText("차례를 기다리는 중");expect(commands[1].action.instruction).toBe("Synthetic new instruction");expect(commands[1].request_id).not.toBe(commands[0].request_id);
});

const recurringFixture={schedule:{id:"scheduled",revision:1,title:"Synthetic recurring template",message:"예약한 읽기 업무",state:"active",reason:null,rule:{kind:"daily",time:"09:00",timezone:"Asia/Seoul"},next_fire_at:1800000000,next_local_slot:null,valid_until:null,timezone_db:"fixture",created_at:0,updated_at:0},template:{...mission.request,task:"Synthetic recurring calculation"},model:{provider_id:"provider",provider_revision:1,provider_name:"Synthetic provider",kind:"compatible",model:"synthetic",effort:null,verified_at:0,function_tools:true},changes_only:true,recent:[],history:{occurrences:[],skipped_ranges:[]}};
it("saves an explicit immutable recurring template and retries its exact uncertain payload without clearing draft",async()=>{
 const sent:any[]=[];let saved=false;
 vi.stubGlobal("fetch",vi.fn(async(url,init)=>{
  if(String(url).endsWith("/recurring-missions")&&init?.method==="POST"){sent.push(JSON.parse(String(init.body)));if(sent.length===1)throw new TypeError("synthetic lost ACK");saved=true;return new Response(JSON.stringify(recurringFixture))}
  return new Response(JSON.stringify(String(url).endsWith("/recurring-missions")&&saved?[recurringFixture]:[]));
 }));
 render(<Missions accountId="owner" onExpired={vi.fn()} back={vi.fn()}/>);
 fireEvent.click(screen.getByText("예약해서 맡기기"));
 fireEvent.change(screen.getByLabelText("예약할 일"),{target:{value:"Compute scheduled synthetic total"}});
 fireEvent.change(screen.getByLabelText("예약 계산의 정확한 예상값"),{target:{value:"0.3"}});
 fireEvent.click(screen.getByRole("button",{name:"업무 예약 저장"}));await screen.findByRole("alert");
 expect((screen.getByLabelText("예약할 일") as HTMLTextAreaElement).value).toBe("Compute scheduled synthetic total");
 fireEvent.click(screen.getByRole("button",{name:"업무 예약 저장"}));await waitFor(()=>expect(sent).toHaveLength(2));expect(sent[1]).toEqual(sent[0]);
 expect(sent[0]).toMatchObject({template:{execution_mode:"durable",checks:[{kind:"decimal_equals",expected:"0.3"}]},schedule:{rule:{kind:"daily",time:"09:00"}}});
 await screen.findByText("Synthetic recurring template");expect((screen.getByLabelText("예약할 일") as HTMLTextAreaElement).value).toBe("");
});
it("controls only the recurring grant and preserves unsaved form while polling updates its revision",async()=>{
 const commands:any[]=[];let item=structuredClone(recurringFixture);
 vi.stubGlobal("fetch",vi.fn(async(url,init)=>{
  if(String(url).endsWith("/recurring-missions/scheduled/control")){commands.push(JSON.parse(String(init?.body)));item={...item,schedule:{...item.schedule,revision:2,state:"paused"}};return new Response(JSON.stringify(item))}
  return new Response(JSON.stringify(String(url).endsWith("/recurring-missions")?[item]:[]));
 }));
 render(<Missions accountId="owner" onExpired={vi.fn()} back={vi.fn()}/>);fireEvent.click(screen.getByText("예약해서 맡기기"));
 fireEvent.change(screen.getByLabelText("예약할 일"),{target:{value:"Keep this unsaved recurring draft"}});
 fireEvent.click(await screen.findByRole("button",{name:"예약 멈추기"}));await screen.findByRole("button",{name:"다음 회차부터 재개"});
 expect(commands).toEqual([{action:"pause",expected_revision:1}]);expect((screen.getByLabelText("예약할 일") as HTMLTextAreaElement).value).toBe("Keep this unsaved recurring draft");
 fireEvent.click(screen.getByRole("button",{name:"예약 시간 변경"}));expect(screen.getByLabelText("예약할 일")).toBeDisabled();expect(screen.getByLabelText("정확히 같은 확인 결과는 소식함 알림 생략")).toBeDisabled();
});
it("discards a previous account recurring list and clears its draft on account lifetime change",async()=>{
 let resolve!:(r:Response)=>void;const late=new Promise<Response>(r=>resolve=r);
 vi.stubGlobal("fetch",vi.fn(async(url,init)=>String(url).endsWith("/recurring-missions")&&init?.headers["X-Moosuri-Account"]==="first"?late:new Response("[]")));
 const {rerender}=render(<Missions accountId="first" onExpired={vi.fn()} back={vi.fn()}/>);fireEvent.click(screen.getByText("예약해서 맡기기"));fireEvent.change(screen.getByLabelText("예약할 일"),{target:{value:"First owner private draft"}});
 rerender(<Missions accountId="second" onExpired={vi.fn()} back={vi.fn()}/>);resolve(new Response(JSON.stringify([recurringFixture])));
 await waitFor(()=>expect(screen.queryByText("Synthetic recurring template")).toBeNull());expect((screen.getByLabelText("예약할 일") as HTMLTextAreaElement).value).toBe("");
});
