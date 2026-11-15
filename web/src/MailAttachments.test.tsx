import {render,screen} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {expect,it,vi} from "vitest";
import {MailAttachments} from "./MailAttachments";
const file={part_id:"1",attachment_id:"a1",filename:"synthetic.pdf",mime_type:"application/pdf",size:700000,readable:true};
it("reads exact attachment pages with the returned digest and clears scope changes",async()=>{
  const request=vi.fn(async(_p:string,_m?:string,b?:any)=>({status:"ok",attachment:file,text:b.start_byte?"Synthetic end":"Synthetic start",partial:!b.start_byte,next_start_byte:b.start_byte?null:15,page_digest:"same-text-and-bytes",pages:2}));
  const user=userEvent.setup();const view=render(<MailAttachments connection="c" message="m1" attachments={[file]} request={request as any} fail={vi.fn()}/>);
  await user.click(screen.getByRole("button",{name:"첨부 synthetic.pdf 읽기"}));expect(await screen.findByText("Synthetic start")).toBeInTheDocument();
  expect(request.mock.calls[0][2]).toMatchObject({message_id:"m1",part_id:"1",attachment_id:"a1",start_byte:0,expected_digest:null});
  await user.click(screen.getByRole("button",{name:"첨부 다음 부분 읽기"}));expect(await screen.findByText("Synthetic end")).toBeInTheDocument();
  expect(request.mock.calls[1][2]).toMatchObject({start_byte:15,expected_digest:"same-text-and-bytes"});
  view.rerender(<MailAttachments connection="other" message="m2" attachments={[file]} request={request as any} fail={vi.fn()}/>);expect(screen.queryByText("Synthetic end")).toBeNull();
  expect(request.mock.calls.every(c=>c[0]==="/mail/attachments/read"&&c[1]==="POST")).toBe(true);
});
it("shows failed extraction without a claimed body and does not offer unsupported downloads",async()=>{
  const user=userEvent.setup();const request=vi.fn(async()=>({status:"no_text",attachment:file}));
  render(<MailAttachments connection="c" message="m" attachments={[file,{...file,part_id:"2",filename:"unsupported.zip",readable:false}]} request={request as any} fail={vi.fn()}/>);
  expect(screen.getByRole("button",{name:"첨부 unsupported.zip 읽기"})).toBeDisabled();
  await user.click(screen.getByRole("button",{name:"첨부 synthetic.pdf 읽기"}));expect(await screen.findByText(/추출할 텍스트가 없습니다/)).toBeInTheDocument();expect(screen.queryByRole("button",{name:"첨부 다음 부분 읽기"})).toBeNull();
});
it("does not display a late attachment response after changing mailbox scope",async()=>{
  let resolve!:(v:unknown)=>void;const request=vi.fn(()=>new Promise(r=>{resolve=r}));const user=userEvent.setup();
  const view=render(<MailAttachments connection="old" message="m1" attachments={[file]} request={request as any} fail={vi.fn()}/>);
  expect(screen.getByText(/684 KB/)).toBeInTheDocument();
  await user.click(screen.getByRole("button",{name:"첨부 synthetic.pdf 읽기"}));
  expect(screen.getByRole("status")).toHaveTextContent("첨부파일을 읽는 중…");
  view.rerender(<MailAttachments connection="new" message="m2" attachments={[file]} request={request as any} fail={vi.fn()}/>);
  const {act}=await import("@testing-library/react");await act(async()=>{resolve({status:"ok",attachment:file,text:"old mailbox text"});});
  expect(screen.queryByText("old mailbox text")).toBeNull();
  expect(screen.queryByRole("status")).toBeNull();
});
