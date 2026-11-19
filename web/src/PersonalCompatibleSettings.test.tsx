import {render, screen, waitFor, within} from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import {afterEach, expect, it, vi} from "vitest";
import {PersonalCompatibleSettings} from "./PersonalCompatibleSettings";
import {QuotaPanel} from "./QuotaPanel";

const response = (data:unknown) => new Response(JSON.stringify(data), {headers:{"Content-Type":"application/json"}});
const base = "/api/v1/settings/compatible-providers";
afterEach(() => vi.unstubAllGlobals());
it("discovers a local model and adds a verified personal choice on request", async()=>{
  const p = {id:"local",revision:2,name:"Ollama",enabled:true,config:{kind:"compatible",base_url:"http://127.0.0.1:11434/v1"}};
  const requests:unknown[]=[];
  vi.stubGlobal("fetch",vi.fn(async (url:string,init:RequestInit)=>{
    expect(new Headers(init.headers).get("X-Moosuri-Account")).toBe("owner");
    if(url===base && init.method==="POST"){requests.push(JSON.parse(String(init.body))); return response(p);}
    if(url===base) return response({providers:[],default_selection:null});
    if(url.endsWith("/discover"))return response({candidates:[{name:p.name,base_url:p.config.base_url,models:[{id:"local-other",name:"Other model"},{id:"local-chat",name:"Local chat"}]}]});
    if(url.endsWith("/local/probe")){requests.push(JSON.parse(String(init.body)));return response({});}
    throw new Error("unexpected");
  }));
  render(<PersonalCompatibleSettings accountId="owner" onExpired={vi.fn()} onVerified={vi.fn()}/>);
  const user=userEvent.setup();
  await user.click(await screen.findByRole("button",{name:"서버에서 로컬 모델 찾기"}));
  expect(requests).toEqual([]);
  await user.selectOptions(await screen.findByRole("combobox",{name:"Ollama 모델"}),"local-chat");
  await user.click(screen.getByRole("button",{name:"연결 저장 및 모델 확인"}));
  await screen.findByText(/텍스트 응답을 확인했습니다/);
  expect(requests).toEqual([{name:"Ollama",base_url:p.config.base_url,enabled:true,credential:{action:"clear"}},{revision:2,model:"local-chat",effort:null}]);
});
it("keeps compatible connection controls out of an unauthorized session", async () => {
  const fetcher = vi.fn(async () => new Response(null, {status:403}));
  vi.stubGlobal("fetch", fetcher);
  render(<PersonalCompatibleSettings accountId="member" onExpired={vi.fn()} onVerified={vi.fn()} />);
  await waitFor(() => expect(fetcher).toHaveBeenCalledOnce());
  expect(screen.queryByRole("region", {name:"개인 LLM 연결"})).not.toBeInTheDocument();
});

it("keeps a saved connection available when fetching the model catalog fails", async () => {
  const provider = {id:"local",revision:1,name:"Local API",enabled:true,config:{kind:"compatible",base_url:"https://models.example.test/v1"}};
  vi.stubGlobal("fetch", vi.fn(async (url:string, init:RequestInit) => {
    if (url === base) return response(init.method === "POST" ? provider : {providers:[]});
    return new Response(null, {status:502});
  }));
  render(<PersonalCompatibleSettings accountId="owner" onExpired={vi.fn()} onVerified={vi.fn()} />);
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", {name:"주소로 연결"}));
  await user.type(screen.getByLabelText("연결 이름"), provider.name);
  await user.type(screen.getByLabelText("API 기본 주소"), provider.config.base_url);
  await user.click(screen.getByRole("button", {name:"연결 저장"}));
  expect(await screen.findByRole("alert")).toHaveTextContent("모델 목록을 가져오지 못했습니다.");
  expect(screen.getByRole("status")).toHaveTextContent("연결을 저장했습니다.");
  expect(within(screen.getByRole("group", {name:provider.name})).getByLabelText("확인할 모델 ID")).toBeEnabled();
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

const visionChoice={source:"personal_compatible",provider_id:"owner-local",provider_revision:4,provider_name:"Owner local",kind:"compatible",model:"synthetic-vision",effort:null,verified_at:1,function_tools:false};
const visionProvider={id:"owner-local",revision:4,name:"Owner local",enabled:true,has_credential:false,config:{kind:"compatible",base_url:"http://127.0.0.1:11434/v1"},choices:[visionChoice,{...visionChoice,model:"other-model"},{...visionChoice,model:"old-revision",provider_revision:3}]};
it("verifies images for the exact saved personal model without changing the chosen text model",async()=>{
 const verified=vi.fn();const sent:unknown[]=[];
 vi.stubGlobal("fetch",vi.fn(async(url:string,init:RequestInit)=>{
  expect(new Headers(init.headers).get("X-Moosuri-Account")).toBe("owner");
  if(url.endsWith("/vision-probe")){sent.push(JSON.parse(String(init.body)));return response({vision:true})}
  return response({providers:[visionProvider],default_selection:null});
 }));
 render(<PersonalCompatibleSettings accountId="owner" onExpired={vi.fn()} onVerified={verified}/>);
 const user=userEvent.setup();
 await user.click(await screen.findByRole("button",{name:"이미지 연결 확인 · synthetic-vision · 모델 기본값"}));
 expect(await screen.findByText("이미지 읽기 확인됨")).toBeInTheDocument();
 expect(sent).toEqual([{revision:4,model:"synthetic-vision",effort:null}]);
 expect(verified).toHaveBeenCalledOnce();
 expect(screen.getByRole("button",{name:"이미지 연결 확인 · other-model · 모델 기본값"})).toBeInTheDocument();
 expect(screen.queryByText(/old-revision/)).not.toBeInTheDocument();
 expect(screen.getByText(/대화로 돌아가 직접 다시 보내세요/)).toBeInTheDocument();
});
it.each(["failed","unconfirmed"])("keeps image verification available after a %s result",async(result)=>{
 const verified=vi.fn();
 vi.stubGlobal("fetch",vi.fn(async(url:string)=>url.endsWith("/vision-probe")
  ? result==="failed"?new Response(JSON.stringify({code:"vision_probe_failed"}),{status:502}):response({vision:false})
  : response({providers:[visionProvider],default_selection:null})));
 render(<PersonalCompatibleSettings accountId="owner" onExpired={vi.fn()} onVerified={verified}/>);
 await userEvent.setup().click(await screen.findByRole("button",{name:"이미지 연결 확인 · synthetic-vision · 모델 기본값"}));
 expect(await screen.findByRole("alert")).toHaveTextContent("이미지 읽기를 확인하지 못했습니다");
 expect(screen.queryByText("이미지 읽기 확인됨")).not.toBeInTheDocument();
 expect(screen.getByRole("button",{name:"이미지 연결 확인 · synthetic-vision · 모델 기본값"})).toBeEnabled();
 expect(verified).not.toHaveBeenCalled();
});
it("offers image verification immediately after a new text model is verified",async()=>{
 let ready=false;
 vi.stubGlobal("fetch",vi.fn(async(url:string)=>{
  if(url.endsWith("/probe")){ready=true;return response(visionChoice)}
  return response({providers:[{...visionProvider,choices:ready?[visionChoice]:[]}],default_selection:null});
 }));
 render(<PersonalCompatibleSettings accountId="owner" onExpired={vi.fn()} onVerified={vi.fn()}/>);
 const user=userEvent.setup();
 await user.type(await screen.findByLabelText("확인할 모델 ID"),visionChoice.model);
 await user.click(screen.getByRole("button",{name:"텍스트 연결 확인"}));
 expect(await screen.findByRole("button",{name:"이미지 연결 확인 · synthetic-vision · 모델 기본값"})).toBeEnabled();
});
