import {render,screen,fireEvent,cleanup,waitFor} from "@testing-library/react";
import {afterEach,it,expect,vi} from "vitest";
import {TaskMonitors,type TaskMonitor} from "./TaskMonitors";
const monitor:TaskMonitor={id:"monitor",parent_mission_id:"first",linked_mission_ids:["first","later"],revision:1,state:"proposed",scope:{kind:"connected_thread",connection_id:"saved",config_revision:1,credential_epoch:1,thread_id:"support"},sources:[{id:"source",title:"Saved support message",revision:1}],cadence_seconds:null,expires_at:null,reusable:false,reason:null,last_checked_at:null,observations:[]};
afterEach(()=>{cleanup();vi.unstubAllGlobals()});
it("discloses future same-thread messages and retains exact consent on lost response",async()=>{
 const requests:any[]=[];let item=monitor;vi.stubGlobal("fetch",vi.fn(async(_url,init)=>{if(init?.method==="POST"){requests.push(JSON.parse(String(init.body)));if(requests.length===1)throw new TypeError("synthetic lost response");item={...monitor,state:"active",revision:2,cadence_seconds:3600,expires_at:requests[0].expires_at};return new Response(JSON.stringify(item))}return new Response(JSON.stringify([item]))}));
 render(<TaskMonitors accountId="owner" missionId="later" onExpired={vi.fn()}/>);await screen.findByText(/나중에 들어오는 답장/);fireEvent.click(screen.getByRole("button",{name:"후속 확인 시작"}));await screen.findByRole("alert");fireEvent.click(screen.getByRole("button",{name:"후속 확인 시작"}));await screen.findByText("허용한 범위 확인 중");expect(requests).toHaveLength(2);expect(requests[1]).toEqual(requests[0]);expect(requests[0]).toMatchObject({read:true,check:true,notify:true,reuse:false});
});
it("shows coverage limits and records monitor-local resolution",async()=>{
 const writes:any[]=[];const item={...monitor,state:"active",reason:"coverage_limited_needs_review",observations:[{ordinal:1,mission_id:"child",state:"failed",created_at:100,resolved_at:null,note:null}]};vi.stubGlobal("fetch",vi.fn(async(url,init)=>{if(init?.method==="POST"){writes.push({url:String(url),body:JSON.parse(String(init.body))});return new Response(JSON.stringify(item))}return new Response(JSON.stringify([item]))}));
 render(<TaskMonitors accountId="owner" onExpired={vi.fn()}/>);await screen.findByText(/전체를 확인하지 않았으니/);fireEvent.change(screen.getByLabelText("다음 변경 확인에만 참고할 메모 (선택)"),{target:{value:"Ignore resolved billing question"}});fireEvent.click(screen.getByRole("button",{name:"확인했어요 · 추가 조치 없음"}));await waitFor(()=>expect(writes).toHaveLength(1));expect(writes[0]).toEqual({url:"/api/v1/task-monitors/monitor/observations/1/resolve",body:{note:"Ignore resolved billing question"}});
});
it("reads current consent after a write instead of joining an older inventory request",async()=>{
 let reads=0;let release!:(response:Response)=>void;
 const active={...monitor,state:"active",revision:2,cadence_seconds:300,expires_at:9999999999};
 const stopped={...active,state:"withdrawn",revision:3};
 vi.stubGlobal("fetch",vi.fn(async(_url,init)=>{
  if(init?.method==="POST")return new Response(JSON.stringify(stopped));
  reads++;
  if(reads===2)return new Promise<Response>(resolve=>{release=resolve});
  return new Response(JSON.stringify([reads===1?active:stopped]));
 }));
 render(<TaskMonitors accountId="owner" missionId="first" onExpired={vi.fn()}/>);
 await screen.findByText("허용한 범위 확인 중");
 fireEvent.click(screen.getByRole("button",{name:"후속 확인 새로고침"}));
 await waitFor(()=>expect(reads).toBe(2));
 fireEvent.click(screen.getByRole("button",{name:"후속 확인과 재사용 허용 철회"}));
 await waitFor(()=>expect(screen.getByRole("button",{name:"후속 확인과 재사용 허용 철회"})).toBeDisabled());
 release(new Response(JSON.stringify([active])));
 await screen.findByText("권한 또는 자료 연결 철회");
 expect(reads).toBe(3);
 expect(screen.queryByRole("button",{name:"후속 확인과 재사용 허용 철회"})).not.toBeInTheDocument();
});
