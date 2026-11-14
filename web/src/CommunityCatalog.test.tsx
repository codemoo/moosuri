import {act,fireEvent,render,screen,waitFor} from "@testing-library/react";
import {beforeEach,expect,it,vi} from "vitest";
import {CommunitySkills} from "./CommunitySkills";
import type {CommunitySkillCard} from "../shared/api-types";
const offer:CommunitySkillCard={id:"github-"+"a".repeat(64),name:"Catalog recipe",description:"Synthetic public offer",source:"https://github.com/example/recipes",source_commit:"a".repeat(40),version:"1.0.0",digest:"b".repeat(64),license:"Apache-2.0",capabilities:["knowledge_search"],installed_version:null,installed_digest:null,enabled:false,hidden:false,revision:0,can_rollback:false,readiness:"preview_required",missing:[],github:{repository:"https://github.com/example/recipes",directory:"recipe"},catalog:{artifact_sha256:"c".repeat(64),verified:false,stale:false}};
const response=(body:unknown)=>new Response(JSON.stringify(body),{status:200,headers:{"content-type":"application/json"}});
const callbacks=()=>({onExpired:vi.fn(),back:vi.fn(),connect:vi.fn(),start:vi.fn()});
beforeEach(()=>vi.restoreAllMocks());
const disabledStatus=()=>response({configured:false,source:null,state:"disabled",verified_at:null,checked_at:null,stale:false});
function mockFetch(handler:(path:string,init?:RequestInit)=>Promise<Response>) { return vi.fn((path:string,init?:RequestInit)=>path.endsWith("/catalog/status")?Promise.resolve(disabledStatus()):handler(path,init)); }

it.each(["unavailable","corrupt"])("shows an empty %s catalog on entry and replaces stale status on a plain refresh",async(initial)=>{
 let state=initial;
 const fetch=vi.fn(async(path:string)=>path.endsWith("/catalog/status")?response({configured:true,source:"Public synthetic source",state,verified_at:null,checked_at:1,stale:state!=="verified_index"}):response([]));
 vi.stubGlobal("fetch",fetch);
 render(<CommunitySkills accountId="owner" {...callbacks()}/>);
 await screen.findByText(/공개 목록을 확인하지 못했습니다/);
 state="verified_index";
 fireEvent.click(screen.getByRole("button",{name:"목록 새로고침"}));
 await waitFor(()=>expect(screen.queryByText(/공개 목록을 확인하지 못했습니다/)).not.toBeInTheDocument());
 expect(fetch.mock.calls.filter(([path])=>path.endsWith("/catalog/status"))).toHaveLength(2);
});
it("discards a delayed catalog status after the account changes",async()=>{
 let resolve!:(r:Response)=>void;
 const oldStatus=new Promise<Response>(r=>resolve=r);
 const fetch=vi.fn(async(path:string,init?:RequestInit)=>{
   if(!path.endsWith("/catalog/status"))return response([]);
   return (init?.headers as Record<string,string>)["X-Moosuri-Account"]==="a"?oldStatus:disabledStatus();
 });
 vi.stubGlobal("fetch",fetch);
 const props=callbacks();const view=render(<CommunitySkills accountId="a" {...props}/>);
 await waitFor(()=>expect(fetch.mock.calls.some(([path])=>path.endsWith("/catalog/status"))).toBe(true));
 view.rerender(<CommunitySkills accountId="b" {...props}/>);
 await screen.findByText(/별도 공개 목록이 설정되지 않았습니다/);
 await act(async()=>resolve(response({configured:true,source:"Old account catalog",state:"unavailable",verified_at:null,checked_at:1,stale:true})));
 expect(screen.queryByText(/공개 목록을 확인하지 못했습니다/)).not.toBeInTheDocument();
 expect(screen.getByText(/별도 공개 목록이 설정되지 않았습니다/)).toBeInTheDocument();
});
it("requires selected public-origin preview before explicit install and never claims index readiness",async()=>{
 const calls:{path:string;body:unknown}[]=[];vi.stubGlobal("fetch",mockFetch(async(path:string,init?:RequestInit)=>{calls.push({path,body:init?.body?JSON.parse(String(init.body)):null});if(path.endsWith("/preview"))return response({...offer,readiness:"installable",catalog:{...offer.catalog,verified:true}});if(init?.method==="POST")return response([{...offer,installed_digest:offer.digest,installed_version:offer.version,enabled:true,revision:1,readiness:"ready",catalog:{...offer.catalog,verified:true}}]);return response([offer]);}));
 const props=callbacks();render(<CommunitySkills accountId="owner" {...props}/>);
 expect(await screen.findByRole("button",{name:"추가"})).toBeDisabled();expect(screen.getByText(/공개 목록 제안/)).toBeInTheDocument();expect(screen.queryByRole("button",{name:"채팅에서 시작"})).not.toBeInTheDocument();
 fireEvent.click(screen.getByRole("button",{name:"내용과 원본 확인"}));await waitFor(()=>expect(screen.getByRole("button",{name:"추가"})).toBeEnabled());expect(props.start).not.toHaveBeenCalled();expect(calls.filter(c=>c.body)).toHaveLength(1);
 fireEvent.click(screen.getByRole("button",{name:"추가"}));await screen.findByRole("button",{name:"채팅에서 시작"});expect(calls.filter(c=>c.body)).toHaveLength(2);expect(props.start).not.toHaveBeenCalled();
 expect(calls.find(c=>c.path.endsWith("/preview"))?.body).toEqual({expected_digest:offer.digest});
});
it.each(["ready","connection_needed"])("keeps the installed version %s after previewing a different update",async(readiness)=>{
 const installed={...offer,version:"2.0.0",installed_version:"1.0.0",installed_digest:"d".repeat(64),enabled:true,revision:1,readiness,missing:readiness==="ready"?[]:["web_read"],capabilities:readiness==="ready"?["knowledge_search","web_read"]:["knowledge_search"]};
 const writes:string[]=[];
 vi.stubGlobal("fetch",mockFetch(async(path:string,init?:RequestInit)=>{
  if(init?.method==="POST"){
   writes.push(path);expect(path.endsWith("/preview")).toBe(true);
   return response({...installed,catalog:{...installed.catalog,verified:true}});
  }
  return response([installed]);
 }));
 const props=callbacks();render(<CommunitySkills accountId="owner" {...props}/>);
 fireEvent.click(await screen.findByRole("button",{name:"내용과 원본 확인"}));
 await waitFor(()=>expect(screen.getByRole("button",{name:"업데이트"})).toBeEnabled());
 expect(!!screen.queryByRole("button",{name:"채팅에서 시작"})).toBe(readiness==="ready");
 expect(screen.getByText(/설치 버전 1.0.0/)).toBeInTheDocument();
 expect(writes).toHaveLength(1);expect(props.start).not.toHaveBeenCalled();
});
it("reports last verified offline cache without replacing installed cards or automatically installing",async()=>{
 let posts=0;vi.stubGlobal("fetch",mockFetch(async(path:string,init?:RequestInit)=>{if(init?.method==="POST"){posts++;expect(path).toContain("/catalog/refresh");return response({configured:true,source:"Public synthetic source",state:"offline_cache",verified_at:1,checked_at:2,stale:true});}return response([offer]);}));
 render(<CommunitySkills accountId="owner" {...callbacks()}/>);await screen.findByText(offer.name);fireEvent.click(screen.getByRole("button",{name:"공개 목록 업데이트 확인"}));await screen.findByText(/마지막 확인한 목록/);expect(posts).toBe(1);expect(screen.getByText(offer.name)).toBeInTheDocument();expect(screen.getByRole("button",{name:"추가"})).toBeDisabled();
});
it("fences a late public preview across account switch",async()=>{
 let resolve!:(r:Response)=>void;const pending=new Promise<Response>(r=>resolve=r);vi.stubGlobal("fetch",mockFetch(async(_path:string,init?:RequestInit)=>init?.method==="POST"?pending:response((init?.headers as Record<string,string>)["X-Moosuri-Account"]==="a"?[offer]:[])));
 const props=callbacks();const view=render(<CommunitySkills accountId="a" {...props}/>);fireEvent.click(await screen.findByRole("button",{name:"내용과 원본 확인"}));view.rerender(<CommunitySkills accountId="b" {...props}/>);await act(async()=>resolve(response({...offer,name:"Old owner response",catalog:{...offer.catalog,verified:true}})));await waitFor(()=>expect(screen.queryByText("Old owner response")).not.toBeInTheDocument());expect(props.start).not.toHaveBeenCalled();
});
