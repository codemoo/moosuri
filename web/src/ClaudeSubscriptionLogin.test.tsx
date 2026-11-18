import { render, screen, waitFor, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, it, expect, vi } from "vitest";
import { ClaudeSubscriptionLogin } from "./ClaudeSubscriptionLogin";
const response=(body:unknown,status=200)=>new Response(JSON.stringify(body),{status,headers:{"Content-Type":"application/json"}});
const waiting={attempt_id:"synthetic",status:"awaiting_code",authorization_url:"https://claude.com/cai/oauth/authorize?state=synthetic",expires_in_seconds:290};
const props={accountId:"owner",bindingId:"owned",revision:1,disabled:false,onExpired:vi.fn()};
beforeEach(()=>vi.restoreAllMocks());
it.each(["code","cancel"])("keeps the newer %s response when an earlier status GET arrives late",async kind=>{
 let release!:(value:Response)=>void;let polled=false;
 const fetch=vi.fn((url:RequestInfo|URL,init?:RequestInit)=>{
  if(String(url).endsWith("/login"))return Promise.resolve(response(waiting));
  if(init?.method==="POST")return Promise.resolve(response({...waiting,status:"verifying",authorization_url:null}));
  if(init?.method==="DELETE")return Promise.resolve(response({...waiting,status:"cancelled",authorization_url:null}));
  polled=true;return new Promise<Response>(resolve=>{release=resolve;});
 });vi.stubGlobal("fetch",fetch);
 render(<ClaudeSubscriptionLogin {...props}/>);const user=userEvent.setup();
 await user.click(screen.getByRole("button",{name:"구독 로그인"}));
 expect(await screen.findByRole("link",{name:"Claude 공식 로그인 열기"})).toHaveAttribute("rel","noopener noreferrer");
 await waitFor(()=>expect(polled).toBe(true));
 if(kind==="code"){
  await user.type(screen.getByLabelText("2. Claude에서 받은 로그인 코드"),"synthetic-code#state");
  await user.click(screen.getByRole("button",{name:"코드로 연결 완료"}));
  expect(await screen.findByText("Claude가 로그인 코드를 확인하고 있습니다…")).toBeInTheDocument();
  expect(fetch.mock.calls.some(([,init])=>init?.body===JSON.stringify({code:"synthetic-code#state"}))).toBe(true);
 }else{
  await user.click(screen.getByRole("button",{name:"로그인 취소"}));
  expect(await screen.findByText("로그인을 취소했습니다.")).toBeInTheDocument();
 }
 await act(async()=>release(response(waiting)));
 expect(screen.queryByRole("link",{name:"Claude 공식 로그인 열기"})).not.toBeInTheDocument();
 expect(screen.queryByLabelText("2. Claude에서 받은 로그인 코드")).not.toBeInTheDocument();
 expect(localStorage.length).toBe(0);expect(sessionStorage.length).toBe(0);
});
it("never presents a nonofficial login URL or raw provider failure text",async()=>{
 vi.stubGlobal("fetch",vi.fn(async()=>response({...waiting,authorization_url:"https://untrusted.example/cai/oauth/authorize?state=synthetic"})));
 render(<ClaudeSubscriptionLogin {...props}/>);
 await userEvent.setup().click(screen.getByRole("button",{name:"구독 로그인"}));
 await screen.findByLabelText("2. Claude에서 받은 로그인 코드");
 expect(screen.queryByRole("link")).not.toBeInTheDocument();
});
it("notifies model recovery once when polling observes login success", async () => {
 const succeeded = vi.fn();
 vi.stubGlobal("fetch", vi.fn(async (url:RequestInfo|URL) => response({ ...waiting, status: String(url).endsWith("/login") ? "verifying" : "succeeded", authorization_url: null })));
 const view = render(<ClaudeSubscriptionLogin {...props} onSucceeded={succeeded}/>);
 await userEvent.setup().click(screen.getByRole("button", {name:"구독 로그인"}));
 await waitFor(() => expect(succeeded).toHaveBeenCalledOnce());
 view.rerender(<ClaudeSubscriptionLogin {...props} onSucceeded={() => succeeded()}/>);
 expect(succeeded).toHaveBeenCalledOnce();
 expect(screen.getByText(/사용할 모델 목록을 자동으로 확인합니다/)).toBeInTheDocument();
});
