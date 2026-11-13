import {fireEvent,render,screen,waitFor} from "@testing-library/react";
import {expect,it,vi} from "vitest";
import {ChatPanel,type PendingSend} from "./ChatPanel";
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
