import {render, screen, within} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {expect, it, vi} from "vitest";
import {PersonalCompatibleSettings} from "./PersonalCompatibleSettings";
import {QuotaPanel} from "./QuotaPanel";

const response = (data:unknown) => new Response(JSON.stringify(data), {headers:{"Content-Type":"application/json"}});
const base = "/api/v1/settings/compatible-providers";
it("discovers a local model and adds a verified personal choice on request", async()=>{
  const p = {id:"local",revision:2,name:"Ollama",enabled:true,config:{kind:"compatible",base_url:"http://127.0.0.1:11434/v1"}};
  const requests:unknown[]=[];
  vi.stubGlobal("fetch",vi.fn(async (url:string,init:RequestInit)=>{
    expect(new Headers(init.headers).get("X-Moosuri-Account")).toBe("owner");
    if(url===base && init.method==="POST"){requests.push(JSON.parse(String(init.body))); return response(p);}
    if(url===base) return response({providers:[],default_selection:null});
    if(url.endsWith("/discover"))return response({candidates:[{name:p.name,base_url:p.config.base_url,models:[{id:"local-chat",name:"Local chat"}]}]});
    if(url.endsWith("/local/probe")){requests.push(JSON.parse(String(init.body)));return response({});}
    throw new Error("unexpected");
  }));
  render(<PersonalCompatibleSettings accountId="owner" onExpired={vi.fn()} onVerified={vi.fn()}/>);
  const user=userEvent.setup();
  await user.click(screen.getByRole("button",{name:"서버에서 로컬 모델 찾기"}));
  expect(requests).toEqual([]);
  await user.click(await screen.findByRole("button",{name:"모델 확인하고 연결"}));
  await screen.findByText(/텍스트 응답을 확인했습니다/);
  expect(requests).toEqual([{name:"Ollama",base_url:p.config.base_url,enabled:true,credential:{action:"clear"}},{revision:2,model:"local-chat",effort:null}]);
});
it("shows reported zero and reset time, and distinguishes an unknown subscription limit", async()=>{
  vi.stubGlobal("fetch",vi.fn(async(url:string)=>response(url.includes("/claude/")
    ? {status:"available",limits:[{label:"Weekly",scope:null,remaining_percent:0,resets_at:"2026-11-25T10:00:00Z"}]}
    : {status:"unsupported",metrics:[],observed_at:null})));
  render(<><section aria-label="Claude"><QuotaPanel accountId="owner" id="a" revision={1} kind="claude" onExpired={vi.fn()}/></section>
    <section aria-label="ChatGPT"><QuotaPanel accountId="owner" id="b" revision={1} kind="chatgpt" onExpired={vi.fn()}/></section></>);
  const user=userEvent.setup();
  await user.click(within(screen.getByRole("region",{name:"Claude"})).getByRole("button"));
  expect(await screen.findByText("구독 공통 · Weekly")).toHaveTextContent("Weekly");
  expect(screen.getByText("구독 공통 · Weekly").parentElement).toHaveTextContent("남은 양 0% · 리셋");
  const gpt=within(screen.getByRole("region",{name:"ChatGPT"})); await user.click(gpt.getByRole("button"));
  expect(await gpt.findByText(/리셋 시각을 제공하지 않습니다/)).toBeInTheDocument();
  expect(gpt.getByRole("link")).toHaveAttribute("href","https://chatgpt.com/settings/usage");
});
