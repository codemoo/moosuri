import {useState} from "react";
import {fireEvent,render,screen,waitFor} from "@testing-library/react";
import {expect,it,vi} from "vitest";
import {ChatPanel,type PendingSend,type ChatMemory} from "./ChatPanel";
const response=(v:unknown)=>new Response(JSON.stringify(v),{status:200,headers:{"content-type":"application/json"}});
it.each([undefined,{id:"original-record",revision:1}])("an uncertain submission retains its original artifact target (%s)",async(original)=>{
 const posts:Record<string,unknown>[]=[];vi.stubGlobal("fetch",vi.fn(async(path:string,init?:RequestInit)=>{
  if(init?.method==="POST"){posts.push(JSON.parse(String(init.body)));throw new TypeError("synthetic offline")}
  if(path.includes("/settings/model")){const choice={provider_id:"p",provider_revision:1,provider_name:"fixture",kind:"compatible",model:"fixture",effort:null,function_tools:true,verified_at:1};return response({selection:null,effective:choice,choices:[choice]})}
  if(path.includes("/events"))return new Response(new ReadableStream({start(c){init?.signal?.addEventListener("abort",()=>{try{c.close()}catch{/* cancelled by the consumer before account abort */}},{once:true})}}),{headers:{"content-type":"text/event-stream"}});
  if(path.endsWith("/messages"))return response({thread_id:"thread",cursor:0,turns:[],next_before:null});
  return new Response("{}",{status:404});
 }));const pending:PendingSend={request_id:"request-original",text:"original request",status:"uncertain",artifact_target:original};
 render(<ChatPanel accountId="account" name="owner" threadId="thread" memory={{drafts:{thread:"future draft"},pending:{thread:pending}}} artifactTarget={{id:"new-selection",revision:7,title:"new selected report"}} onDraft={vi.fn()} onPending={vi.fn()} onRestoreDraft={vi.fn()} onCreated={vi.fn()} onSuspend={vi.fn()} createThread={async()=>"thread"} onExpired={vi.fn()} onThreadMissing={vi.fn()} onThreadUpdated={vi.fn()} openSettings={vi.fn()}/>);
 const button=await screen.findByRole("button",{name:"같은 요청 다시 제출"});await waitFor(()=>expect(button).toBeEnabled());fireEvent.click(button);await waitFor(()=>expect(posts).toHaveLength(1));expect(posts[0]).toEqual({request_id:"request-original",text:"original request",...(original?{artifact_target:original}:{})});expect(screen.getByLabelText("메시지")).toHaveValue("future draft");
});

it.each([null,{id:"saved-reminder",revision:3}])("uncertain scheduling freezes original timezone and receipt target (%s)",async(target)=>{
 const posts:Record<string,unknown>[]=[];
 vi.stubGlobal("fetch",vi.fn(async(path:string,init?:RequestInit)=>{
  if(init?.method==="POST"){posts.push(JSON.parse(String(init.body)));throw new TypeError("synthetic offline")}
  if(path.includes("/settings/model")){const choice={provider_id:"p",provider_revision:1,provider_name:"fixture",kind:"compatible",model:"fixture",effort:null,function_tools:true,verified_at:1};return response({selection:null,effective:choice,choices:[choice]})}
  if(path.includes("/events"))return new Response(new ReadableStream({start(c){init?.signal?.addEventListener("abort",()=>{try{c.close()}catch{/* consumer ended */}},{once:true})}}),{headers:{"content-type":"text/event-stream"}});
  if(path.endsWith("/messages"))return response({thread_id:"thread",cursor:0,turns:[],next_before:null});return new Response("{}",{status:404});
 }));
 const pending:PendingSend={request_id:"original-schedule-request",text:"pause reminder",status:"uncertain",schedule_context:{timezone:"America/New_York",target}};
 render(<ChatPanel accountId="account" name="owner" threadId="thread" memory={{drafts:{thread:"future draft"},pending:{thread:pending}}} artifactTarget={{id:"new-record",revision:7,title:"new record"}} onDraft={vi.fn()} onPending={vi.fn()} onRestoreDraft={vi.fn()} onCreated={vi.fn()} onSuspend={vi.fn()} createThread={async()=>"thread"} onExpired={vi.fn()} onThreadMissing={vi.fn()} onThreadUpdated={vi.fn()} openSettings={vi.fn()}/>);
 const button=await screen.findByRole("button",{name:"같은 요청 다시 제출"});await waitFor(()=>expect(button).toBeEnabled());fireEvent.click(button);await waitFor(()=>expect(posts).toHaveLength(1));expect(posts[0]).toEqual({request_id:pending.request_id,text:pending.text,schedule_context:pending.schedule_context});
});

it("captures browser timezone and receipt-selected revision once before an uncertain send",async()=>{
 let zone="Asia/Seoul";const original=Intl.DateTimeFormat.prototype.resolvedOptions;
 vi.spyOn(Intl.DateTimeFormat.prototype,"resolvedOptions").mockImplementation(function(this:Intl.DateTimeFormat){return {...original.call(this),timeZone:zone}});
 const posts:Record<string,unknown>[]=[];
 const choice={provider_id:"p",provider_revision:1,provider_name:"fixture",kind:"compatible",model:"fixture",effort:null,function_tools:true,verified_at:1};
 const receipt={request_id:"saved-request",id:"saved-reminder",revision:4,action:"create",state:"active",title:"Synthetic reminder",next_local_slot:"utc:1800000000",timezone:"UTC",url:"/app/schedules"};
 const turn={sequence:1,user_text:"saved reminder",assistant:[],run:{id:"saved-run",request_id:"saved-request",thread_id:"thread",attempt:1,state:"completed",version:2,model:choice,error:null,usage:{input_tokens:null,output_tokens:null,cache_read_tokens:null,cache_write_tokens:null},evidence:[],can_cancel:false,can_retry:false,created_at:1,schedule_receipts:[receipt]}};
 vi.stubGlobal("fetch",vi.fn(async(path:string,init?:RequestInit)=>{
  if(init?.method==="POST"){posts.push(JSON.parse(String(init.body)));throw new TypeError("synthetic lost ACK")}
  if(path.includes("/settings/model"))return response({selection:null,effective:choice,choices:[choice]});
  if(path.includes("/events"))return new Response(new ReadableStream({start(c){init?.signal?.addEventListener("abort",()=>{try{c.close()}catch{/* consumer ended */}},{once:true})}}),{headers:{"content-type":"text/event-stream"}});
  if(path.endsWith("/messages"))return response({thread_id:"thread",cursor:0,turns:[turn],next_before:null});return new Response("{}",{status:404});
 }));
 function Harness(){const [memory,setMemory]=useState<ChatMemory>({drafts:{thread:"pause reminder"},pending:{}});return <ChatPanel accountId="account" name="owner" threadId="thread" memory={memory} onDraft={(slot,text)=>setMemory(m=>({...m,drafts:{...m.drafts,[slot]:text}}))} onPending={(slot,value)=>setMemory(m=>{const pending={...m.pending};if(value)pending[slot]=value;else delete pending[slot];return {...m,pending}})} onRestoreDraft={vi.fn()} onCreated={vi.fn()} onSuspend={vi.fn()} createThread={async()=>"thread"} onExpired={vi.fn()} onThreadMissing={vi.fn()} onThreadUpdated={vi.fn()} openSettings={vi.fn()}/>}
 render(<Harness/>);fireEvent.click(await screen.findByRole("button",{name:"이 알림 선택"}));expect(screen.getByText(/선택한 알림: Synthetic reminder/)).toBeInTheDocument();const send=screen.getByRole("button",{name:"메시지 전송"});await waitFor(()=>expect(send).toBeEnabled());fireEvent.click(send);const retry=await screen.findByRole("button",{name:"같은 요청 다시 제출"});zone="America/New_York";await waitFor(()=>expect(retry).toBeEnabled());fireEvent.click(retry);await waitFor(()=>expect(posts).toHaveLength(2));expect(posts[0].schedule_context).toEqual({timezone:"Asia/Seoul",target:{id:"saved-reminder",revision:4}});expect(posts[1]).toEqual(posts[0]);expect(screen.getByRole("link",{name:/알림 · Synthetic reminder/})).toHaveAttribute("href","/app/schedules");
});
