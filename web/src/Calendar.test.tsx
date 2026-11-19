import {render,screen,fireEvent,waitFor,cleanup} from "@testing-library/react";import {beforeEach,afterEach,it,expect,vi} from "vitest";import {Calendar} from "./Calendar";import type {CalendarConnection,CalendarEvent,CalendarAction} from "../shared/api-types";
const connection:CalendarConnection={id:"synthetic-connection",primary_id:"owner@example.test",title:"Synthetic primary",time_zone:"Asia/Seoul",epoch:1,revision:1,status:"active"};
const event:CalendarEvent={id:"synthetic1",etag:'"etag1"',status:"confirmed",fields:{title:"Synthetic meeting",description:"",location:"",start:"2026-11-15T10:00:00+09:00",end:"2026-11-15T11:00:00+09:00",time_zone:"Asia/Seoul"},html_link:null,writable:true,all_day:false,updated:null};
const action:CalendarAction={id:"action1",connection_id:connection.id,state:"outcome_unknown",event_id:event.id,before_etag:null,provider_etag:null,event:null,receipt:null,error:"lookup_required",version:1};
const response=(v:unknown,status=200)=>new Response(status===204?null:JSON.stringify(v),{status,headers:{"content-type":"application/json"}});
// The fixture must fall inside the calendar's initial seven-day window.
beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date("2026-11-14T00:00:00Z"));
});
afterEach(()=>{cleanup();vi.useRealTimers();vi.unstubAllGlobals();window.history.replaceState({},"","/")});
function server(override?:(path:string,init:RequestInit)=>Response|Promise<Response>|undefined){const writes:{path:string;body:any}[]=[];vi.stubGlobal("fetch",vi.fn(async(url,init:RequestInit={})=>{const path=String(url).replace("/api/v1","");if(init.method==="POST")writes.push({path,body:JSON.parse(String(init.body))});const value=override?.(path,init);if(value)return value;if(path==="/calendar/connections")return response({configured:true,connections:[connection]});if(path==="/calendar/actions")return init.method==="POST"?response({...action,state:"applied",event}):response([]);if(path.startsWith("/calendar/calendars"))return response({items:[{id:connection.primary_id,title:connection.title,time_zone:"Asia/Seoul",primary:true,access_role:"owner",email_notifications:false}],partial:false,next_page_token:null});if(path==="/calendar/events")return response({events:[event],partial:true,next_page_token:"page2",primary_id:connection.primary_id});return response({})}));return writes}
it("shows partial primary reads and explicit ETag update without an extra approval flow",async()=>{const writes=server();render(<Calendar accountId="owner" onExpired={vi.fn()} back={vi.fn()}/>);await screen.findByRole("heading",{name:"Synthetic meeting"});expect(screen.getByText(/일부 일정만 확인/)).toBeTruthy();fireEvent.click(screen.getByRole("button",{name:"수정"}));fireEvent.change(screen.getByLabelText("제목"),{target:{value:"Changed synthetic event"}});fireEvent.click(screen.getByRole("button",{name:"변경사항 저장"}));await waitFor(()=>expect(writes.some(v=>v.path==="/calendar/actions")).toBe(true));const saved=writes.find(v=>v.path==="/calendar/actions")!;expect(saved.body).toMatchObject({connection_id:connection.id,expected_epoch:1,expected_revision:1,mutation:{kind:"update",event_id:event.id,etag:event.etag,fields:{title:"Changed synthetic event"}}});expect(saved.body.mutation.fields.attendees).toBeUndefined();});
it("offers lookup after an unknown result and does not resend the mutation",async()=>{const writes=server((path,init)=>path==="/calendar/actions"&&init.method!=="POST"?response([action]):path.endsWith("/lookup")?response({...action,state:"observed_absent"}):undefined);render(<Calendar accountId="owner" onExpired={vi.fn()} back={vi.fn()}/>);await screen.findByRole("button",{name:"저장 상태 확인"});fireEvent.click(screen.getByRole("button",{name:"저장 상태 확인"}));await screen.findAllByText(/이 요청의 삭제 완료는 확정할 수 없어요/);expect(writes.filter(v=>v.path==="/calendar/actions")).toHaveLength(0);expect(writes.filter(v=>v.path.endsWith("/lookup"))).toHaveLength(1);});
it("clears private event results when owner changes while an event response is late",async()=>{let resolve!:(v:Response)=>void;const late=new Promise<Response>(r=>resolve=r);server((path,init)=>path==="/calendar/events"&&new Headers(init.headers).get("X-Moosuri-Account")==="first"?late:path==="/calendar/connections"&&new Headers(init.headers).get("X-Moosuri-Account")==="second"?response({configured:true,connections:[]}):undefined);const view=render(<Calendar accountId="first" onExpired={vi.fn()} back={vi.fn()}/>);await waitFor(()=>expect(fetch).toHaveBeenCalledWith("/api/v1/calendar/events",expect.anything()));view.rerender(<Calendar accountId="second" onExpired={vi.fn()} back={vi.fn()}/>);resolve(response({events:[event],next_page_token:null,partial:false,primary_id:connection.primary_id}));await waitFor(()=>expect(screen.queryByText("Synthetic meeting")).toBeNull());});
it("reports missing operator setup and blocks writes for calendars with email notifications",async()=>{server((path)=>path.startsWith("/calendar/calendars")?response({items:[{id:connection.primary_id,title:connection.title,time_zone:"Asia/Seoul",primary:true,access_role:"owner",email_notifications:true}],next_page_token:null,partial:false}):undefined);render(<Calendar accountId="owner" onExpired={vi.fn()} back={vi.fn()}/>);await screen.findByText(/이 캘린더에는 이메일 알림 설정/);expect(screen.getByRole("button",{name:"일정 만들기"})).toBeDisabled();expect(screen.getByRole("button",{name:"삭제"})).toBeDisabled();});
it("renews intent only after definite conflict using the refreshed revision",async()=>{let revision=1,attempts=0,dispatches=0;const writes=server((path,init)=>{if(path==="/calendar/connections")return response({configured:true,connections:[{...connection,revision}]});if(path==="/calendar/actions"&&init.method==="POST"){attempts++;if(attempts===1){revision=2;return response({code:"conflict"},409)}dispatches++;return response({...action,id:JSON.parse(String(init.body)).request_id,state:"applied",event})}});render(<Calendar accountId="owner" onExpired={vi.fn()} back={vi.fn()}/>);await screen.findByRole("heading",{name:event.fields.title});fireEvent.change(screen.getByLabelText("제목"),{target:{value:"Unchanged synthetic draft"}});fireEvent.click(screen.getByRole("button",{name:"일정 만들기"}));await screen.findByRole("alert");await waitFor(()=>expect(screen.getByRole("button",{name:"일정 만들기"})).not.toBeDisabled());expect(screen.getByLabelText("제목")).toHaveValue("Unchanged synthetic draft");fireEvent.click(screen.getByRole("button",{name:"일정 만들기"}));await waitFor(()=>expect(dispatches).toBe(1));const calls=writes.filter(v=>v.path==="/calendar/actions");expect(calls).toHaveLength(2);expect(calls[0].body.expected_revision).toBe(1);expect(calls[1].body.expected_revision).toBe(2);expect(calls[0].body.request_id).not.toBe(calls[1].body.request_id);expect(calls[0].body.mutation).toEqual(calls[1].body.mutation);});
it("keeps the exact immutable intent after ambiguous failure",async()=>{let attempts=0;const writes=server((path,init)=>path==="/calendar/actions"&&init.method==="POST"?(++attempts===1?Promise.reject(new TypeError("synthetic ACK loss")):response({...action,id:JSON.parse(String(init.body)).request_id,state:"applied",event})):undefined);render(<Calendar accountId="owner" onExpired={vi.fn()} back={vi.fn()}/>);await screen.findByRole("heading",{name:event.fields.title});fireEvent.change(screen.getByLabelText("제목"),{target:{value:"Exact retry draft"}});fireEvent.click(screen.getByRole("button",{name:"일정 만들기"}));await screen.findByRole("alert");await waitFor(()=>expect(screen.getByRole("button",{name:"일정 만들기"})).not.toBeDisabled());fireEvent.click(screen.getByRole("button",{name:"일정 만들기"}));await waitFor(()=>expect(attempts).toBe(2));const calls=writes.filter(v=>v.path==="/calendar/actions");expect(calls[1].body).toEqual(calls[0].body);});
it("removes acknowledged deletion immediately and preserves the independent draft",async()=>{server((path,init)=>path==="/calendar/actions"&&init.method==="POST"?response({...action,id:JSON.parse(String(init.body)).request_id,state:"applied",event:null}):undefined);render(<Calendar accountId="owner" onExpired={vi.fn()} back={vi.fn()}/>);await screen.findByRole("heading",{name:event.fields.title});fireEvent.change(screen.getByLabelText("제목"),{target:{value:"Independent unsaved draft"}});fireEvent.click(screen.getByRole("button",{name:"삭제"}));await waitFor(()=>expect(screen.queryByRole("heading",{name:event.fields.title})).toBeNull());expect(screen.getByLabelText("제목")).toHaveValue("Independent unsaved draft");});
it("shows confirmed update while preserving edits made during the pending receipt",async()=>{let resolve!:(v:Response)=>void;const held=new Promise<Response>(r=>resolve=r);let saved:any;server((path,init)=>{if(path==="/calendar/actions"&&init.method==="POST"){saved=JSON.parse(String(init.body));return held}});render(<Calendar accountId="owner" onExpired={vi.fn()} back={vi.fn()}/>);await screen.findByRole("heading",{name:event.fields.title});fireEvent.click(screen.getByRole("button",{name:"수정"}));fireEvent.change(screen.getByLabelText("제목"),{target:{value:"Confirmed updated title"}});fireEvent.click(screen.getByRole("button",{name:"변경사항 저장"}));await waitFor(()=>expect(saved).toBeTruthy());fireEvent.change(screen.getByLabelText("제목"),{target:{value:"Later unsaved edit"}});resolve(response({...action,id:saved.request_id,state:"applied",event:{...event,fields:saved.mutation.fields}}));await screen.findByRole("heading",{name:"Confirmed updated title"});expect(screen.queryByRole("heading",{name:event.fields.title})).toBeNull();expect(screen.getByLabelText("제목")).toHaveValue("Later unsaved edit");});
it("adds a confirmed create to the displayed range without a full reload",async()=>{server((path,init)=>path==="/calendar/actions"&&init.method==="POST"?response({...action,id:JSON.parse(String(init.body)).request_id,state:"applied",event_id:"created2",event:{...event,id:"created2",fields:JSON.parse(String(init.body)).mutation.fields}}):undefined);render(<Calendar accountId="owner" onExpired={vi.fn()} back={vi.fn()}/>);await screen.findByRole("heading",{name:event.fields.title});fireEvent.change(screen.getByLabelText("제목"),{target:{value:"New confirmed event"}});fireEvent.click(screen.getByRole("button",{name:"일정 만들기"}));await screen.findByRole("heading",{name:"New confirmed event"});expect(screen.getByRole("heading",{name:event.fields.title})).toBeTruthy();});

it("independent later edit uses the ETag returned by the acknowledged prior save",async()=>{let resolve!:(v:Response)=>void;const held=new Promise<Response>(r=>resolve=r);let first:any;let attempts=0;const writes=server((path,init)=>{if(path==="/calendar/actions"&&init.method==="POST"){attempts++;const sent=JSON.parse(String(init.body));if(attempts===1){first=sent;return held}return response({...action,id:sent.request_id,state:"conflict",event:null})}});render(<Calendar accountId="owner" onExpired={vi.fn()} back={vi.fn()}/>);await screen.findByRole("heading",{name:event.fields.title});fireEvent.click(screen.getByRole("button",{name:"수정"}));fireEvent.change(screen.getByLabelText("제목"),{target:{value:"First update"}});fireEvent.click(screen.getByRole("button",{name:"변경사항 저장"}));await waitFor(()=>expect(first).toBeTruthy());fireEvent.change(screen.getByLabelText("제목"),{target:{value:"Later preserved edit"}});resolve(response({...action,id:first.request_id,state:"applied",provider_etag:'"etag2"',event:{...event,etag:'"etag2"',fields:first.mutation.fields}}));await screen.findByRole("heading",{name:"First update"});await waitFor(()=>expect(screen.getByRole("button",{name:"변경사항 저장"})).not.toBeDisabled());fireEvent.click(screen.getByRole("button",{name:"변경사항 저장"}));await waitFor(()=>expect(attempts).toBe(2));const calls=writes.filter(v=>v.path==="/calendar/actions");expect(calls[1].body.mutation.fields.title).toBe("Later preserved edit");expect(calls[1].body.mutation.etag).toBe('"etag2"');});

it.each([false,true])("lookup advances only its matching connection editing base (%s)",async(otherConnection)=>{
 const current=otherConnection?{...connection,id:"second-connection"}:connection;
 const writes=server((path,init)=>{
  if(path==="/calendar/connections")return response({configured:true,connections:[current]});
  if(path==="/calendar/actions"&&init.method!=="POST")return response([action]);
  if(path.endsWith("/lookup"))return response({...action,state:"applied",provider_etag:'"etag2"',event:{...event,etag:'"etag2"',fields:{...event.fields,title:"Confirmed lookup event"}}});
 });
 render(<Calendar accountId="owner" onExpired={vi.fn()} back={vi.fn()}/>);
 await screen.findByRole("heading",{name:event.fields.title});
 fireEvent.click(screen.getByRole("button",{name:"수정"}));
 fireEvent.change(screen.getByLabelText("제목"),{target:{value:"Preserved later draft"}});
 fireEvent.click(screen.getByRole("button",{name:"저장 상태 확인"}));
 await waitFor(()=>expect(screen.queryByRole("button",{name:"저장 상태 확인"})).toBeNull());
 await waitFor(()=>expect(screen.getByRole("button",{name:"변경사항 저장"})).toBeEnabled());
 expect(screen.getByLabelText("제목")).toHaveValue("Preserved later draft");
 if(otherConnection)expect(screen.queryByRole("heading",{name:"Confirmed lookup event"})).toBeNull();
 fireEvent.click(screen.getByRole("button",{name:"변경사항 저장"}));
 await waitFor(()=>expect(writes.some(v=>v.path==="/calendar/actions")).toBe(true));
 expect(writes.find(v=>v.path==="/calendar/actions")!.body).toMatchObject({connection_id:current.id,mutation:{kind:"update",event_id:event.id,etag:otherConnection?event.etag:'"etag2"',fields:{title:"Preserved later draft"}}});
});

it("creates all-day dates from inclusive form days and keeps unknown intent exact",async()=>{
 let attempts=0;
 const writes=server((path,init)=>path==="/calendar/actions"&&init.method==="POST"?(++attempts===1?Promise.reject(new TypeError("synthetic ACK loss")):response({...action,id:JSON.parse(String(init.body)).request_id,state:"outcome_unknown"})):undefined);
 render(<Calendar accountId="owner" onExpired={vi.fn()} back={vi.fn()}/>);
 await screen.findByRole("heading",{name:event.fields.title});
 fireEvent.change(screen.getByLabelText("제목"),{target:{value:"Synthetic holiday"}});
 fireEvent.click(screen.getByLabelText("종일"));
 fireEvent.change(screen.getByLabelText("시작일"),{target:{value:"2027-03-14"}});
 fireEvent.change(screen.getByLabelText("마지막 날"),{target:{value:"2027-03-16"}});
 fireEvent.click(screen.getByRole("button",{name:"일정 만들기"}));
 await screen.findByRole("alert");
 await waitFor(()=>expect(screen.getByRole("button",{name:"일정 만들기"})).toBeEnabled());
 fireEvent.click(screen.getByRole("button",{name:"일정 만들기"}));
 await waitFor(()=>expect(attempts).toBe(2));
 const calls=writes.filter(v=>v.path==="/calendar/actions");
 expect(calls[1].body).toEqual(calls[0].body);
 expect(calls[0].body.mutation.fields).toEqual({title:"Synthetic holiday",description:"",location:"",start:"2027-03-14",end:"2027-03-17",time_zone:null});
 expect(screen.getByLabelText("시작일")).toHaveValue("2027-03-14");
 expect(screen.getByLabelText("마지막 날")).toHaveValue("2027-03-16");
});
it("moves an observed all-day span across DST and deletes with the acknowledged ETag",async()=>{
 const allDay={...event,all_day:true,fields:{...event.fields,start:"2027-03-14",end:"2027-03-17",time_zone:null}};
 const writes=server((path,init)=>{
  if(path==="/calendar/events")return response({events:[allDay],partial:false,next_page_token:null,primary_id:connection.primary_id});
  if(path==="/calendar/actions"&&init.method==="POST"){
   const sent=JSON.parse(String(init.body));
   return response({...action,id:sent.request_id,state:"applied",provider_etag:'"etag2"',event:sent.mutation.kind==="delete"?null:{...allDay,etag:'"etag2"',fields:sent.mutation.fields}});
  }
 });
 render(<Calendar accountId="owner" onExpired={vi.fn()} back={vi.fn()}/>);
 await screen.findByText("2027-03-14 → 2027-03-16 · 종일");
 // Keep the confirmed moved event inside the currently displayed range.
 fireEvent.change(screen.getByLabelText("조회 시작"),{target:{value:"2027-11-06T00:00"}});
 fireEvent.change(screen.getByLabelText("조회 종료"),{target:{value:"2027-11-12T00:00"}});
 fireEvent.click(screen.getByRole("button",{name:"일정 불러오기"}));
 await waitFor(()=>expect(screen.getByRole("button",{name:"수정"})).toBeEnabled());
 fireEvent.click(screen.getByRole("button",{name:"수정"}));
 expect(screen.getByLabelText("종일")).toBeChecked(); expect(screen.getByLabelText("종일")).toBeDisabled();
 expect(screen.getByLabelText("마지막 날")).toHaveValue("2027-03-16");
 fireEvent.change(screen.getByLabelText("시작일"),{target:{value:"2027-11-07"}});
 expect(screen.getByLabelText("마지막 날")).toHaveValue("2027-11-09");
 fireEvent.click(screen.getByRole("button",{name:"변경사항 저장"}));
 await screen.findByText("2027-11-07 → 2027-11-09 · 종일");
 await waitFor(()=>expect(screen.getByRole("button",{name:"삭제"})).toBeEnabled());
 fireEvent.click(screen.getByRole("button",{name:"삭제"}));
 await waitFor(()=>expect(screen.queryByRole("heading",{name:event.fields.title})).toBeNull());
 const calls=writes.filter(v=>v.path==="/calendar/actions");
 expect(calls).toHaveLength(2);
 expect(calls[0].body.mutation).toMatchObject({kind:"update",event_id:event.id,etag:event.etag,fields:{start:"2027-11-07",end:"2027-11-10",time_zone:null}});
 expect(calls[1].body.mutation).toEqual({kind:"delete",event_id:event.id,etag:'"etag2"'});
});
it("rejects reversed and oversized all-day form spans before sending",async()=>{
 const writes=server();render(<Calendar accountId="owner" onExpired={vi.fn()} back={vi.fn()}/>);
 await screen.findByRole("heading",{name:event.fields.title});
 fireEvent.change(screen.getByLabelText("제목"),{target:{value:"Synthetic holiday"}});fireEvent.click(screen.getByLabelText("종일"));
 fireEvent.change(screen.getByLabelText("시작일"),{target:{value:"2027-03-14"}});
 for(const last of ["2027-03-13","2027-04-14"]){
  fireEvent.change(screen.getByLabelText("마지막 날"),{target:{value:last}});fireEvent.click(screen.getByRole("button",{name:"일정 만들기"}));
  await screen.findByRole("alert");
 }
 expect(writes.filter(v=>v.path==="/calendar/actions")).toHaveLength(0);
});
