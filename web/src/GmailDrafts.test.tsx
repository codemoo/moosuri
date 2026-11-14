import { render,screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect,it,vi } from "vitest";
import { GmailDrafts } from "./GmailDrafts";
import type { GmailDraft,MailAction } from "../shared/api-types";
const content={from:"sender@example.test",to:["recipient@example.test"],cc:[],bcc:["hidden@example.test"],subject:"Synthetic full preview",text:"Complete synthetic body",reply:null};
const action:MailAction={id:"a",version:1,revision:2,connection_id:"c",content,digest:"frozen",state:"approved",expires_at:9999999999,message_id:"synthetic@example.test",provider_message_id:null,provider_thread_id:null,origin_thread_id:null};
it("provider draft CRUD is explicit and carries only the frozen local revision, never send approval",async()=>{
  const draft:GmailDraft={id:"d",digest:"provider-digest",subject:content.subject,text:content.text,text_truncated:false,attachment_names:[],content};
  const request=vi.fn(async(path:string,method?:string,body?:unknown)=>{
    if(path.includes("/provider-draft")){expect(body).toMatchObject({expected:{revision:2,digest:"frozen"}});return {id:"d",message_id:"m"};}
    if(method==="DELETE")return;
    if(path.startsWith("/mail/drafts/d?"))return draft;
    return {drafts:[{id:"d",message_id:"m"}],next_page_token:null};
  });
  const imported=vi.fn();const user=userEvent.setup();
  render(<GmailDrafts connection="c" action={action} request={request as any} importDraft={imported} fail={e=>{throw e}} disabled={false}/>);
  await user.click(screen.getByRole("button",{name:"확인본을 Gmail에 새 초안으로 저장"}));
  expect(await screen.findByText("Gmail 초안함에 저장했습니다. 발송 허락은 부여하지 않았습니다.")).toBeInTheDocument();
  await user.click(screen.getByRole("button",{name:"선택한 Gmail 초안을 확인본으로 덮어쓰기"}));
  expect(request.mock.calls.find(c=>(c[2] as any)?.draft_id==="d")?.[2]).toMatchObject({expected_provider_digest:"provider-digest"});
  await user.click(screen.getByRole("button",{name:"로컬 편집기로 복사 · 새 발송 확인 필요"}));expect(imported).toHaveBeenCalledWith(content);
  await user.click(screen.getByRole("button",{name:"이 Gmail 초안 삭제"}));
  expect(request.mock.calls.some(c=>c[1]==="DELETE")).toBe(true);
  expect(request.mock.calls.some(c=>c[0].includes("approve")||c[0].includes("send"))).toBe(false);
  expect(action.state).toBe("approved");expect(action.digest).toBe("frozen");
});
it("unsupported provider MIME stays readable and cannot overwrite or import",async()=>{
  const request=vi.fn(async(path:string)=>path.includes("/drafts/d?")?{id:"d",digest:"x",subject:"Unsupported",text:"Visible excerpt",text_truncated:true,attachment_names:["synthetic.txt"],content:null}:{drafts:[{id:"d",message_id:"m"}],next_page_token:null});
  const user=userEvent.setup();render(<GmailDrafts connection="c" action={action} request={request as any} importDraft={vi.fn()} fail={vi.fn()} disabled={false}/>);
  await user.click(screen.getByRole("button",{name:"Gmail 초안 목록 읽기"}));await user.click(await screen.findByRole("button",{name:"Gmail 초안 d 읽기"}));
  expect(await screen.findByText("Visible excerpt")).toBeInTheDocument();expect(screen.getByText("일부 본문만 표시되었습니다.")).toBeInTheDocument();
  expect(screen.queryByRole("button",{name:"선택한 Gmail 초안을 확인본으로 덮어쓰기"})).toBeNull();expect(screen.queryByRole("button",{name:"로컬 편집기로 복사 · 새 발송 확인 필요"})).toBeNull();
});
