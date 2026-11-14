import {render,screen,waitFor} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {beforeEach,expect,it,vi} from "vitest";
import {InvitationEmail} from "./InvitationEmail";
import type {InvitationDelivery} from "../shared/api-types";
const body="Hello Synthetic Invitee,\nEmail: invitee@example.test\nSign in: https://assistant.example.test/app\nTemporary access expires UTC2030-01-01.\nTemporary password: synthetic-one-time-password";
const fixture:InvitationDelivery={id:"00000000-0000-4000-8000-000000000001",account_id:"target",state:"delivery_pending",digest:"frozen-digest",expires_at:9999999999,from:"system@example.test",to:"invitee@example.test",subject:"Your moosuri invitation",text:body,provider_message_id:null};
let sends=0,preview=0,rows:InvitationDelivery[],configured=true,lost=false;
beforeEach(()=>{sends=preview=0;rows=[];configured=true;lost=false;vi.stubGlobal("fetch",vi.fn(async(path:string,init:RequestInit={})=>{
 const method=init.method??"GET";
 expect(new Headers(init.headers).get("X-Moosuri-Account")).toBe("admin");
 if(path.endsWith("/sender"))return Response.json({configured,from:configured?"system@example.test":null});
 if(path.endsWith("/invitations/")&&method==="GET")return Response.json(rows.map(v=>({...v,text:null})));
 if(path.includes("/invitations/00000000-")&&method==="GET")return Response.json(rows[0]);
 if(path.endsWith("/invitations/")&&method==="POST"){
  preview++;const input=JSON.parse(String(init.body));expect(input.account_id).toBe("target");expect(input.expected_revision).toBe(2);expect(input.temporary_password).toBe("synthetic-one-time-password");rows=[{...fixture}];return Response.json(rows[0]);
 }
 if(path.endsWith("/approve")){
  sends++;expect(JSON.parse(String(init.body))).toEqual({digest:"frozen-digest"});rows=[{...fixture,state:lost?"delivery_unknown":"sent",text:null,provider_message_id:lost?null:"synthetic-receipt"}];if(lost)throw new TypeError("lost synthetic ACK");return Response.json(rows[0]);
 }
 throw new Error("unexpected synthetic route");
}));});
it("invitation full-preview and password opt-in precede exactly one explicit send",async()=>{
 const user=userEvent.setup();render(<InvitationEmail accountId="admin" onExpired={vi.fn()} target={{account_id:"target",revision:2,temporary_password:"synthetic-one-time-password"}}/>);
 await screen.findByText(/운영자가 설정한 발신자/);expect(sends).toBe(0);expect(preview).toBe(0);
 await user.click(screen.getByRole("checkbox"));await user.click(screen.getByRole("button",{name:"초대 이메일 전체 내용 준비"}));
 expect(await screen.findByText(/Hello Synthetic Invitee/)).toBeInTheDocument();expect(sends).toBe(0);
 await user.click(screen.getByRole("button",{name:"전체 내용을 허락하고 초대 이메일 보내기"}));
 expect(await screen.findByText(/Gmail 접수 ID: synthetic-receipt/)).toBeInTheDocument();expect(sends).toBe(1);expect(screen.queryByText(/Hello Synthetic Invitee/)).toBeNull();expect(screen.queryByRole("button",{name:"전체 내용을 허락하고 초대 이메일 보내기"})).toBeNull();
});
it("lost ACK is status-only recovery and unavailable sender preserves copy/account flow",async()=>{
 const user=userEvent.setup();lost=true;const view=render(<InvitationEmail accountId="admin" onExpired={vi.fn()} target={{account_id:"target",revision:2,temporary_password:"synthetic-one-time-password"}}/>);
 await screen.findByText(/운영자가 설정한 발신자/);await user.click(screen.getByRole("checkbox"));await user.click(screen.getByRole("button",{name:"초대 이메일 전체 내용 준비"}));await user.click(await screen.findByRole("button",{name:"전체 내용을 허락하고 초대 이메일 보내기"}));
 await waitFor(()=>expect(screen.queryByRole("button",{name:"전체 내용을 허락하고 초대 이메일 보내기"})).toBeNull());await user.click(screen.getByRole("button",{name:"초대 발송 상태 새로 읽기"}));
 await screen.findAllByText(/발송 결과 불명/);expect(sends).toBe(1);view.unmount();
 configured=false;render(<InvitationEmail accountId="admin" onExpired={vi.fn()} target={{account_id:"target",revision:2,temporary_password:"synthetic-one-time-password"}}/>);
 expect(await screen.findByText(/시스템 발신자가 설정되지 않았습니다/)).toBeInTheDocument();expect(screen.queryByRole("button",{name:"초대 이메일 전체 내용 준비"})).toBeNull();expect(sends).toBe(1);
});
it("target revision change clears a stale sensitive preview and approval",async()=>{
 const user=userEvent.setup(),target={account_id:"target",revision:2,temporary_password:"synthetic-one-time-password"};const view=render(<InvitationEmail accountId="admin" onExpired={vi.fn()} target={target}/>);
 await screen.findByText(/운영자가 설정한 발신자/);await user.click(screen.getByRole("checkbox"));await user.click(screen.getByRole("button",{name:"초대 이메일 전체 내용 준비"}));expect(await screen.findByText(/Hello Synthetic Invitee/)).toBeInTheDocument();
 view.rerender(<InvitationEmail accountId="admin" onExpired={vi.fn()} target={{...target,revision:3,temporary_password:"new-synthetic-password"}}/>);
 expect(screen.queryByText(/Hello Synthetic Invitee/)).toBeNull();expect(screen.queryByRole("button",{name:"전체 내용을 허락하고 초대 이메일 보내기"})).toBeNull();expect(sends).toBe(0);
});
