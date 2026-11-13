import {act,fireEvent,render,screen,waitFor} from "@testing-library/react";
import {beforeEach,expect,it,vi} from "vitest";
import {Schedules} from "./Schedules";
import type {ScheduleView,ScheduleDraft,LocalNotification,QuietHours} from "../shared/api-types";
const response=(v:unknown,status=200)=>new Response(JSON.stringify(v),{status,headers:{"Content-Type":"application/json"}});
const quiet:QuietHours={revision:0,enabled:false,timezone:"UTC",start_minute:1320,end_minute:420};
const item=(extra:Partial<ScheduleView>={}):ScheduleView=>({id:"schedule-1",revision:1,title:"Synthetic schedule",message:"A fixed local reminder",rule:{kind:"interval",anchor:1800000000,seconds:3600},state:"active",reason:null,next_fire_at:1800003600,next_local_slot:"utc:1800003600",valid_until:null,timezone_db:"2025b",created_at:1800000000,updated_at:1800000000,...extra});
const note=(extra:Partial<LocalNotification>={}):LocalNotification=>({id:"notice-1",schedule_id:"schedule-1",occurrence_id:"occurrence-1",title:"Synthetic local notice",message:"Review the synthetic notes",scheduled_for:1800000000,delivered_at:1800000010,delayed:true,quiet:true,read:false,dismissed:false,...extra});
const props={accountId:"a",onExpired:vi.fn(),back:vi.fn()};
beforeEach(()=>{vi.restoreAllMocks();vi.clearAllMocks();});
function data(path:string,list:ScheduleView[]=[],notes:LocalNotification[]=[]){return path.includes("/notifications/settings")?quiet:path.includes("/notifications")?{items:notes,next_cursor:null}:list;}
async function ready(){await screen.findByText("등록한 예약이 없습니다.");}
function fill(){fireEvent.change(screen.getByLabelText("알림 제목"),{target:{value:"New synthetic reminder"}});fireEvent.change(screen.getByLabelText("알려 줄 내용"),{target:{value:"Only show this fixed reminder"}});}

it("creates interpreted daily local reminders and edits/pauses/resumes/revokes with pinned revisions",async()=>{
 let list:ScheduleView[]=[];const calls:{path:string;method:string;body:unknown}[]=[];
 const fetch=vi.fn(async(path:string,init?:RequestInit)=>{const method=init?.method??"GET";const body=init?.body?JSON.parse(String(init.body)):undefined;calls.push({path,method,body});if(path.endsWith("/preview"))return response({next_fire_at:1800000000,local_slot:"2027-01-15T09:00@America/New_York",timezone_db:"2025b",skipped_gap:null});if(method==="POST"&&path.endsWith("/schedules")){list=[item({...body.draft,id:"new",revision:1})];return response(list[0]);}if(method==="PUT"&&path.includes("/schedules/")){list=[{...list[0],...body.draft,revision:list[0].revision+1}];return response(list[0]);}if(path.endsWith("/control")){const next={...list[0],revision:list[0].revision+1,state:body.action==="pause"?"paused":body.action==="revoke"?"revoked":"active"};list=next.state==="revoked"?[]:[next];return response(next);}return response(data(path,list));});vi.stubGlobal("fetch",fetch);
 render(<Schedules {...props}/>);await ready();fill();fireEvent.change(screen.getByLabelText("반복"),{target:{value:"daily"}});fireEvent.change(screen.getByLabelText("예약 시간대"),{target:{value:"America/New_York"}});fireEvent.click(screen.getByRole("button",{name:"다음 시각 확인"}));await screen.findByText(/다음 알림:/);fireEvent.click(screen.getByRole("button",{name:"알림 예약"}));await screen.findByRole("button",{name:"일시 정지"});
 const created=calls.find(c=>c.path.endsWith("/schedules")&&c.method==="POST")!;expect((created.body as {draft:ScheduleDraft}).draft.rule).toEqual({kind:"daily",time:"09:00",timezone:"America/New_York"});expect(screen.getByText(/모델 작업 실행·메일 발송/)).toBeInTheDocument();
 fireEvent.click(screen.getByRole("button",{name:"일시 정지"}));await screen.findByRole("button",{name:"다시 시작"});fireEvent.click(screen.getByRole("button",{name:"편집"}));fireEvent.change(screen.getByLabelText("알림 제목"),{target:{value:"Edited synthetic reminder"}});fireEvent.click(screen.getByRole("button",{name:"변경 저장"}));await screen.findByText("Edited synthetic reminder");expect((calls.find(c=>c.method==="PUT")!.body as {expected_revision:number}).expected_revision).toBe(2);
 fireEvent.click(screen.getByRole("button",{name:"다시 시작"}));await screen.findByRole("button",{name:"일시 정지"});fireEvent.click(screen.getByRole("button",{name:"예약 삭제"}));await ready();expect(calls.filter(c=>c.path.endsWith("/control")).map(c=>(c.body as {expected_revision:number}).expected_revision)).toEqual([1,3,4]);expect(calls.every(c=>c.path.startsWith("/api/v1/schedules")||c.path.startsWith("/api/v1/notifications"))).toBe(true);
});

it("marks local inbox read/dismiss idempotently and saves quiet-hour state without requesting Push",async()=>{
 let n=note();let q={...quiet};const calls:{path:string;body:unknown}[]=[];const permission=vi.fn();vi.stubGlobal("Notification",{requestPermission:permission});
 vi.stubGlobal("fetch",vi.fn(async(path:string,init?:RequestInit)=>{const body=init?.body?JSON.parse(String(init.body)):undefined;calls.push({path,body});if(path.endsWith("/read")){n={...n,read:true};return response(n);}if(path.endsWith("/dismiss")){n={...n,read:true,dismissed:true};return response(n);}if(path.endsWith("/settings")&&init?.method==="PUT"){q={...body,revision:1};return response(q);}if(path.endsWith("/settings"))return response(q);if(path.includes("/notifications"))return response({items:n.dismissed&&!path.includes("dismissed=true")?[]:[n],next_cursor:null});return response([]);}));
 render(<Schedules {...props}/>);await screen.findByText("Synthetic local notice");expect(screen.getByText(/로컬 알림.*조용한 시간/)).toBeInTheDocument();fireEvent.click(screen.getByRole("button",{name:"읽음"}));await waitFor(()=>expect(screen.queryByRole("button",{name:"읽음"})).not.toBeInTheDocument());fireEvent.click(screen.getByRole("button",{name:"숨기기"}));await waitFor(()=>expect(screen.queryByText("Synthetic local notice")).not.toBeInTheDocument());fireEvent.click(screen.getByRole("checkbox",{name:"숨긴 소식 포함"}));await screen.findByText("Synthetic local notice");
 fireEvent.click(screen.getByRole("checkbox",{name:"조용한 시간 사용"}));fireEvent.change(screen.getByLabelText("조용한 시간 시간대"),{target:{value:"America/New_York"}});fireEvent.click(screen.getByRole("button",{name:"조용한 시간 저장"}));await screen.findByText(/조용한 시간을 저장했습니다/);expect(calls.find(c=>c.body&&c.path.endsWith("/settings"))!.body).toEqual({expected_revision:0,enabled:true,timezone:"America/New_York",start_minute:1320,end_minute:420});expect(permission).not.toHaveBeenCalled();
});

it("fences delayed previous-account create/read responses and clears the previous account form",async()=>{
 let release!:(v:Response)=>void;const held=new Promise<Response>(r=>release=r);
 vi.stubGlobal("fetch",vi.fn(async(path:string,init?:RequestInit)=>init?.method==="POST"?held:response(data(path))));const view=render(<Schedules {...props}/>);await ready();fill();fireEvent.click(screen.getByRole("button",{name:"알림 예약"}));view.rerender(<Schedules {...props} accountId="b"/>);await ready();await act(async()=>release(response(item({title:"Late synthetic previous-account content"}))));expect(screen.queryByText("Late synthetic previous-account content")).not.toBeInTheDocument();expect(screen.getByLabelText("알림 제목")).toHaveValue("");
});

it("a refresh captured before an accepted pause cannot put the schedule back into active UI",async()=>{
 let release!:(v:Response)=>void;let reads=0;const held=new Promise<Response>(r=>release=r);
 vi.stubGlobal("fetch",vi.fn(async(path:string,init?:RequestInit)=>{if(path.endsWith("/control"))return response(item({state:"paused",revision:2}));if(path.endsWith("/schedules")&&init?.method==="GET"){reads++;if(reads===2)return held;return response([item()]);}return response(data(path));}));render(<Schedules {...props}/>);const pause=await screen.findByRole("button",{name:"일시 정지"});const refresh=screen.getByRole("button",{name:"새로고침"});act(()=>{refresh.dispatchEvent(new MouseEvent("click",{bubbles:true}));pause.dispatchEvent(new MouseEvent("click",{bubbles:true}));});await screen.findByRole("button",{name:"다시 시작"});await act(async()=>release(response([item()])));expect(screen.getByRole("button",{name:"다시 시작"})).toBeInTheDocument();expect(screen.queryByRole("button",{name:"일시 정지"})).not.toBeInTheDocument();
});

it("background refresh preserves unsaved quiet fields and their original CAS revision",async()=>{
 let reads=0;let saved:unknown;
 vi.stubGlobal("fetch",vi.fn(async(path:string,init?:RequestInit)=>{if(path.endsWith("/settings")&&init?.method==="PUT"){saved=JSON.parse(String(init.body));return response({code:"conflict"},409);}if(path.endsWith("/settings")){reads++;return response({...quiet,revision:reads===1?0:2});}return response(data(path));}));render(<Schedules {...props}/>);await ready();fireEvent.change(screen.getByLabelText("조용한 시간 시간대"),{target:{value:"America/New_York"}});fireEvent.click(screen.getByRole("button",{name:"새로고침"}));await waitFor(()=>expect(reads).toBe(2));await waitFor(()=>expect(screen.getByRole("button",{name:"조용한 시간 저장"})).toBeEnabled());expect(screen.getByLabelText("조용한 시간 시간대")).toHaveValue("America/New_York");fireEvent.click(screen.getByRole("button",{name:"조용한 시간 저장"}));await screen.findByRole("alert");expect((saved as {expected_revision:number}).expected_revision).toBe(0);
});

it.each([
 {kind:"once",at:1800000031} as const,
 {kind:"interval",anchor:1800000031,seconds:1026} as const,
])("editing title preserves the exact UTC second and interval anchor/duration: $kind",async(rule)=>{
 let saved:ScheduleDraft|undefined;const row=item({rule});
 vi.stubGlobal("fetch",vi.fn(async(path:string,init?:RequestInit)=>{if(init?.method==="PUT"){saved=JSON.parse(String(init.body)).draft;return response({...row,...saved,revision:2});}return response(data(path,[row]));}));
 render(<Schedules {...props}/>);fireEvent.click(await screen.findByRole("button",{name:"편집"}));fireEvent.change(screen.getByLabelText("알림 제목"),{target:{value:"Title only"}});fireEvent.click(screen.getByRole("button",{name:"변경 저장"}));await screen.findByText("Title only");expect(saved!.rule).toEqual(rule);
});


it("keeps a draft editable during background reads and fences the read when the owner saves",async()=>{
 let release!:(value:Response)=>void;
 const held=new Promise<Response>(resolve=>release=resolve);
 let reads=0;let saved:ScheduleDraft|undefined;
 vi.stubGlobal("fetch",vi.fn(async(path:string,init?:RequestInit)=>{
  if(path.endsWith("/schedules")&&init?.method==="GET"){
   reads++;if(reads===2)return held;return response([]);
  }
  if(path.endsWith("/schedules")&&init?.method==="POST"){
   saved=JSON.parse(String(init.body)).draft;
   return response(item({...saved,id:"saved-during-refresh"}));
  }
  return response(data(path));
 }));
 render(<Schedules {...props}/>);await ready();fill();
 fireEvent.click(screen.getByRole("button",{name:"새로고침"}));
 await waitFor(()=>expect(reads).toBe(2));
 expect(screen.getByLabelText("알림 제목")).toBeEnabled();
 expect(screen.getByLabelText("알려 줄 내용")).toBeEnabled();
 expect(screen.getByRole("button",{name:"알림 예약"})).toBeEnabled();
 fireEvent.change(screen.getByLabelText("알림 제목"),{target:{value:"Typing while a refresh is slow"}});
 fireEvent.click(screen.getByRole("button",{name:"알림 예약"}));
 await screen.findByRole("heading",{name:"Typing while a refresh is slow"});
 expect(saved?.title).toBe("Typing while a refresh is slow");
 await act(async()=>release(response([])));
 expect(screen.getByRole("heading",{name:"Typing while a refresh is slow"})).toBeInTheDocument();
});
