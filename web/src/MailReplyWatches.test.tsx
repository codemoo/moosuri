import {act,cleanup,fireEvent,render,screen,waitFor} from "@testing-library/react";
import {afterEach,expect,it,vi} from "vitest";
import {MailReplyWatches,type ReplyWatch} from "./MailReplyWatches";
const target={id:"receipt",title:"Vendor availability",mailbox:"owner@example.test"};
const watch:ReplyWatch={id:"watch",thread_id:"thread",title:target.title,mailbox:target.mailbox,state:"active",cadence_seconds:1800,expires_at:2000000000,last_checked_at:null,next_check_at:1800000000,reason:null,replies:[]};
const response=(v:unknown)=>new Response(JSON.stringify(v));
afterEach(()=>{cleanup();vi.unstubAllGlobals()});
it("retries lost registration with the same receipt and only then shows registered status",async()=>{
 const writes:unknown[]=[];let watches:ReplyWatch[]=[];vi.stubGlobal("fetch",vi.fn(async(_url,init)=>{if(init?.method==="POST"){writes.push(JSON.parse(String(init.body)));if(writes.length===1)throw new TypeError("synthetic response lost");watches=[watch];return response(watch)}return response({targets:[target],watches})}));
 render(<MailReplyWatches accountId="owner" threadId="thread" onExpired={vi.fn()}/>);await screen.findByText(target.title);fireEvent.click(screen.getByText("메일 답장 기다리기"));fireEvent.click(screen.getByRole("button",{name:"답장 기다리기"}));await screen.findByRole("alert");expect(screen.queryByText("답장 기다리는 중")).toBeNull();fireEvent.click(screen.getByRole("button",{name:"답장 기다리기"}));await screen.findByText(/답장 기다리는 중/);expect(writes).toHaveLength(2);expect(writes[1]).toEqual(writes[0]);expect(writes[0]).toMatchObject({thread_id:"thread",target_id:"receipt",cadence_seconds:1800,duration_days:7});
});
it("renders untrusted reply as literal text and stop clears the preview",async()=>{
 let item:ReplyWatch={...watch,state:"replied",next_check_at:null,replies:[{message_id:"m1",from:["vendor@example.test"],subject:"Reply",text:"<img src=x onerror=alert(1)> synthetic reply",partial:true}]};vi.stubGlobal("fetch",vi.fn(async(_url,init)=>{if(init?.method==="POST"){item={...item,state:"stopped",replies:[]};return response(item)}return response({targets:[],watches:[item]})}));
 const {container}=render(<MailReplyWatches accountId="owner" onExpired={vi.fn()}/>);await screen.findByText(/<img src=x/);expect(container.querySelector("img")).toBeNull();expect(screen.getByRole("link",{name:"원래 대화 열기"})).toHaveAttribute("href","/app?thread=thread#mail-replies");fireEvent.click(screen.getByRole("button",{name:"결과 닫기"}));await screen.findByText(/감시 중지/);expect(screen.queryByText(/<img src=x/)).toBeNull();
});
it("fences a late inventory from another conversation",async()=>{
 let resolve!:(r:Response)=>void;vi.stubGlobal("fetch",vi.fn(async(url)=>String(url).includes("thread_id=old")?new Promise<Response>(r=>{resolve=r}):response({targets:[],watches:[{...watch,thread_id:"new",title:"New context"}]})));
 const {rerender}=render(<MailReplyWatches accountId="owner" threadId="old" onExpired={vi.fn()}/>);await waitFor(()=>expect(resolve).toBeTypeOf("function"));rerender(<MailReplyWatches accountId="owner" threadId="new" onExpired={vi.fn()}/>);await screen.findByText("New context");await act(async()=>resolve(response({targets:[target],watches:[watch]})));expect(screen.queryByText(target.title)).toBeNull();expect(screen.getByText("New context")).toBeVisible();
});
