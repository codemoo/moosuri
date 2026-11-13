import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import type { ManagedAccount } from "../shared/api-types";
import { AccountControls } from "./AccountControls";
const target:ManagedAccount={user:{id:"b",name:"Member",email:"member@example.test",role:"member",must_change_password:false},status:"active",revision:2,recovery_required:false,temporary_expires_at:null};
function response(v:unknown,status=200){return new Response(JSON.stringify(v),{status,headers:{"content-type":"application/json"}})}
function result(password:string|null=null){return response({receipt:{request_id:"00000000-0000-4000-8000-000000000001",account_id:"b",revision:3},temporary_password:password})}
beforeEach(()=>{vi.restoreAllMocks();vi.spyOn(crypto,"randomUUID").mockReturnValue("00000000-0000-4000-8000-000000000001");});
function open(){fireEvent.click(screen.getByText("Manage Member"));screen.getByText("Manage Member").parentElement!.setAttribute("open","")}
function props(){return{accountId:"a",target,onExpired:vi.fn(),onChanged:vi.fn(),onPassword:vi.fn()}}
it("retries an uncertain reset with the same request and never invents a second password",async()=>{
  let calls=0;const sent:unknown[]=[];vi.stubGlobal("fetch",vi.fn(async(_p:string,i:RequestInit)=>{sent.push(JSON.parse(String(i.body)));if(++calls===1)throw new TypeError("synthetic loss");return result()}));
  const p=props();render(<AccountControls {...p}/>);open();fireEvent.click(screen.getByRole("button",{name:"Reset password"}));
  fireEvent.click(await screen.findByRole("button",{name:"Retry action"}));
  await screen.findByText(/password was already issued/);expect(sent).toHaveLength(2);expect(sent[0]).toEqual(sent[1]);expect(p.onPassword).not.toHaveBeenCalled();expect(p.onChanged).toHaveBeenCalledTimes(1);
});
it("delivers the new password only to its current account and target",async()=>{
  let resolve!:(r:Response)=>void;vi.stubGlobal("fetch",vi.fn(()=>new Promise<Response>(r=>resolve=r)));
  const p=props();const view=render(<AccountControls {...p}/>);open();fireEvent.click(screen.getByRole("button",{name:"Reset password"}));
  view.rerender(<AccountControls {...p} accountId="other"/>);
  await act(async()=>resolve(result("synthetic-old-account-secret")));
  expect(p.onPassword).not.toHaveBeenCalled();expect(p.onExpired).not.toHaveBeenCalled();expect(p.onChanged).not.toHaveBeenCalled();
});
it("requires refreshed revision after conflict instead of issuing a new action automatically",async()=>{
  const fetch=vi.fn(async()=>response({code:"conflict"},409));vi.stubGlobal("fetch",fetch);const p=props();render(<AccountControls {...p}/>);open();
  fireEvent.click(screen.getByRole("button",{name:"Suspend account"}));await screen.findByRole("alert");
  expect(p.onChanged).toHaveBeenCalledTimes(1);expect(fetch).toHaveBeenCalledTimes(1);expect(screen.queryByRole("button",{name:"Retry action"})).not.toBeInTheDocument();
});
it("keeps recovery locked and protects self-management without extra confirmation dialogs",async()=>{
  const p=props();const view=render(<AccountControls {...p} target={{...target,recovery_required:true}}/>);open();expect(screen.queryByRole("button")).not.toBeInTheDocument();
  view.rerender(<AccountControls {...p} accountId="b"/>);expect(screen.queryByRole("button",{name:"Suspend account"})).not.toBeInTheDocument();
  vi.stubGlobal("fetch",vi.fn(async()=>result()));fireEvent.click(screen.getByRole("button",{name:"Sign out all devices"}));await waitFor(()=>expect(p.onExpired).toHaveBeenCalledTimes(1));
});
it("a later revision fences an in-flight password response and retains current account details",async()=>{
  let resolve!:(r:Response)=>void;vi.stubGlobal("fetch",vi.fn(()=>new Promise<Response>(r=>resolve=r)));const p=props();const view=render(<AccountControls {...p}/>);open();fireEvent.click(screen.getByRole("button",{name:"Reset password"}));
  view.rerender(<AccountControls {...p} target={{...target,revision:4,status:"suspended"}}/>);
  await act(async()=>resolve(result("synthetic-retired-password")));expect(p.onPassword).not.toHaveBeenCalled();expect(p.onChanged).toHaveBeenCalledTimes(1);expect(screen.getByRole("button",{name:"Resume account"})).toBeEnabled();
});

it("synchronous repeated clicks own one request even before React renders busy",async()=>{
  let resolve!:(r:Response)=>void;const fetch=vi.fn(()=>new Promise<Response>(r=>resolve=r));vi.stubGlobal("fetch",fetch);render(<AccountControls {...props()}/>);open();
  const button=screen.getByRole("button",{name:"Reset password"});act(()=>{button.click();button.click();});expect(fetch).toHaveBeenCalledTimes(1);await act(async()=>resolve(result("synthetic-new-password")));
});
