import {act,fireEvent,render,screen,waitFor} from "@testing-library/react";
import {expect,it,vi} from "vitest";
import App from "./App";
const a={id:"a",name:"Admin",email:"admin@example.test",role:"admin" as const,must_change_password:false};
const b={id:"b",name:"Target Member",email:"target@example.test",role:"member" as const,must_change_password:false};
const response=(v:unknown)=>new Response(JSON.stringify(v),{status:200,headers:{"content-type":"application/json"}});
const people=(revision:number)=>({users:[a,{...b,must_change_password:revision>=3}],management:[{user:a,status:"active",revision:1,recovery_required:false,temporary_expires_at:null},{user:{...b,must_change_password:revision>=3},status:revision>=4?"suspended":"active",revision,recovery_required:false,temporary_expires_at:revision>=3?1900000000:null}]});
it("a known superseding revision removes the one-time reset secret before it can be copied",async()=>{
 window.history.replaceState(null,"","/admin");let reads=0;let release!:(v:Response)=>void;const held=new Promise<Response>(r=>release=r);
 vi.stubGlobal("fetch",vi.fn(async(path:string,init?:RequestInit)=>{
  if(path.endsWith("/system"))return response({initialized:true,version:"fixture",stage:"fixture"});
  if(path.endsWith("/auth/session"))return response({user:a,password_change_required:false});
  if(path.endsWith("/threads"))return response({threads:[]});
  if(path.endsWith("/admin/users"))return ++reads===1?response(people(2)):held;
  if(path.endsWith("/admin/users/b/manage")){const input=JSON.parse(String(init?.body));return response({receipt:{request_id:input.request_id,account_id:"b",revision:3},temporary_password:"synthetic-reset-secret"})}
  return new Response("{}",{status:404});
 }));render(<App/>);const summary=await screen.findByText("Manage Target Member");fireEvent.click(summary);summary.parentElement!.setAttribute("open","");fireEvent.click(screen.getByRole("button",{name:"Reset password"}));
 const password=await screen.findByLabelText("Temporary password");expect(password).toHaveValue("synthetic-reset-secret");await waitFor(()=>expect(reads).toBe(2));
 // Another administrator's later management action superseded reset revision3.
 await act(async()=>release(response(people(4))));await screen.findByText("Suspended");
 expect(screen.getByRole("button",{name:"Resume account"})).toBeInTheDocument();
 expect(screen.queryByLabelText("Temporary password")).not.toBeInTheDocument();expect(screen.queryByRole("button",{name:"Copy password"})).not.toBeInTheDocument();
});
import {AccountControls} from "./AccountControls";
it("a refresh observing this exact reset receipt retains its only returned password",async()=>{
 let release!:(v:Response)=>void;const held=new Promise<Response>(r=>release=r);let request="";vi.stubGlobal("fetch",vi.fn((_path:string,init?:RequestInit)=>{request=JSON.parse(String(init?.body)).request_id;return held}));
 const onPassword=vi.fn(),onChanged=vi.fn();const target={user:b,status:"active" as const,revision:2,recovery_required:false,temporary_expires_at:null};const props={accountId:"a",target,onPassword,onChanged,onExpired:vi.fn()};
 const view=render(<AccountControls {...props}/>);const summary=screen.getByText("Manage Target Member");summary.parentElement!.setAttribute("open","");fireEvent.click(screen.getByRole("button",{name:"Reset password"}));
 // A separate people refresh observed this same accepted reset, not a later one.
 view.rerender(<AccountControls {...props} target={{...target,revision:3}}/>);
 await act(async()=>release(response({receipt:{request_id:request,account_id:"b",revision:3},temporary_password:"synthetic-current-secret"})));
 expect(onPassword).toHaveBeenCalledWith("synthetic-current-secret",{request_id:request,account_id:"b",revision:3});expect(onChanged).toHaveBeenCalledTimes(1);expect(screen.queryByText(/Account details changed/)).not.toBeInTheDocument();
});
