import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { CommunitySkills } from "./CommunitySkills";
import type { CommunitySkillCard } from "../shared/api-types";
const card = (extra: Partial<CommunitySkillCard> = {}): CommunitySkillCard => ({id:"source-briefing",name:"Saved briefing",description:"Ground saved evidence",source:"https://github.com/codemoo/moosuri",source_commit:"a".repeat(40),version:"1.0.0",digest:"a".repeat(64),license:"Apache-2.0",capabilities:["knowledge_search"],installed_version:null,installed_digest:null,enabled:false,hidden:false,revision:0,can_rollback:false,readiness:"installable",missing:[],...extra});
const response = (value: unknown) => new Response(JSON.stringify(value),{status:200,headers:{"Content-Type":"application/json"}});
beforeEach(()=>vi.restoreAllMocks());
const disabledStatus=()=>response({configured:false,source:null,state:"disabled",verified_at:null,checked_at:null,stale:false});
function mockFetch(handler:(path:string,init?:RequestInit)=>Promise<Response>) { return vi.fn((path:string,init?:RequestInit)=>path.endsWith("/catalog/status")?Promise.resolve(disabledStatus()):handler(path,init)); }

it("installs only on deliberate action, preserves package revision, and starts via chat without hidden execution",async()=>{
 const calls: {path:string;init?:RequestInit}[]=[];const start=vi.fn();
 vi.stubGlobal("fetch",mockFetch(async(path:string,init?:RequestInit)=>{calls.push({path,init});return response([init?.method==="POST"?card({installed_digest:"a".repeat(64),installed_version:"1.0.0",enabled:true,readiness:"ready",revision:1}):card()]);}));
 render(<CommunitySkills accountId="a" onExpired={vi.fn()} back={vi.fn()} connect={vi.fn()} start={start}/>);
 fireEvent.click(await screen.findByRole("button",{name:"추가"}));
 await screen.findByRole("button",{name:"채팅에서 시작"});
 expect(calls.filter(c=>c.init?.method==="POST")).toHaveLength(1);
 expect(JSON.parse(String(calls.find(c=>c.init?.method==="POST")?.init?.body))).toEqual({action:"install",expected_revision:0,expected_digest:"a".repeat(64)});
 expect((calls.find(c=>c.init?.method==="POST")?.init?.headers as Record<string,string>)["X-Moosuri-Account"]).toBe("a");
 expect(start).not.toHaveBeenCalled();fireEvent.click(screen.getByRole("button",{name:"채팅에서 시작"}));expect(start).toHaveBeenCalledWith("Saved briefing");expect(calls.filter(c=>c.init?.method==="POST")).toHaveLength(1);
});
it("does not advertise missing connections as ready and persists hiding through stable package action",async()=>{
 const connect=vi.fn();const fetch=mockFetch(async(_path:string,init?:RequestInit)=>response([card({readiness:"connection_needed",missing:["mail_read"],hidden:init?.method==="POST",revision:init?.method==="POST"?1:0})]));vi.stubGlobal("fetch",fetch);
 render(<CommunitySkills accountId="a" onExpired={vi.fn()} back={vi.fn()} connect={connect} start={vi.fn()}/>);
 fireEvent.click(await screen.findByRole("button",{name:"Gmail 연결하기"}));expect(connect).toHaveBeenCalledOnce();expect(screen.queryByRole("button",{name:"채팅에서 시작"})).not.toBeInTheDocument();
 fireEvent.click(screen.getByRole("button",{name:"제안 숨기기"}));await waitFor(()=>expect(screen.queryByText("Saved briefing")).not.toBeInTheDocument());
 fireEvent.click(screen.getByRole("checkbox"));expect(screen.getByText("Saved briefing")).toBeInTheDocument();expect(JSON.parse(String(fetch.mock.calls.find(c=>c[1]?.method==="POST")?.[1]?.body)).action).toBe("hide");
});
it("fences late installation results and local forms when the account changes",async()=>{
 let resolve!: (value:Response)=>void;const late=new Promise<Response>(r=>resolve=r);
 const fetch=mockFetch(async(_path:string,init?:RequestInit)=>init?.method==="POST"?late:response((init?.headers as Record<string,string>)["X-Moosuri-Account"]==="a"?[card()]:[]));vi.stubGlobal("fetch",fetch);
 const props={onExpired:vi.fn(),back:vi.fn(),connect:vi.fn(),start:vi.fn()};const view=render(<CommunitySkills accountId="a" {...props}/>);
 fireEvent.click(await screen.findByRole("button",{name:"추가"}));view.rerender(<CommunitySkills accountId="b" {...props}/>);
 await waitFor(()=>expect(fetch.mock.calls.some(c=>(c[1]?.headers as Record<string,string>)["X-Moosuri-Account"]==="b")).toBe(true));await act(async()=>resolve(response([card({name:"Private old account response",installed_digest:"a".repeat(64),enabled:true})])));
 expect(screen.queryByText("Private old account response")).not.toBeInTheDocument();expect(screen.queryByText("Saved briefing")).not.toBeInTheDocument();
});

it("inspects private candidates without public-origin claims and removes them through existing owner-scoped controls",async()=>{
 const calls:{path:string;init?:RequestInit}[]=[];
 const privateCard=card({id:"private:synthetic",name:"개인 자료 찾기",description:"새 검색어로 저장 자료를 찾아 읽습니다.",source:"private",source_commit:"",license:"private",installed_version:"1",installed_digest:"a".repeat(64),enabled:true,revision:2,readiness:"candidate"});
 vi.stubGlobal("fetch",mockFetch(async(path:string,init?:RequestInit)=>{calls.push({path,init});return response(init?.method==="POST"?[]:[privateCard]);}));
 render(<CommunitySkills accountId="a" onExpired={vi.fn()} back={vi.fn()} connect={vi.fn()} start={vi.fn()}/>);
 await screen.findByText("학습 중");
 expect(screen.queryByRole("button",{name:"채팅에서 시작"})).not.toBeInTheDocument();
 expect(screen.queryByRole("button",{name:"추가"})).not.toBeInTheDocument();
 expect(screen.queryByRole("link",{name:"공개 원본"})).not.toBeInTheDocument();
 expect(screen.getByText(/전체 업무 성공을 보증하지 않습니다/)).toBeInTheDocument();
 fireEvent.click(screen.getByRole("button",{name:"삭제"}));
 await waitFor(()=>expect(screen.queryByText("개인 자료 찾기")).not.toBeInTheDocument());
 const mutation=calls.find(c=>c.init?.method==="POST");
 expect(mutation?.path).toContain("private%3Asynthetic");
 expect(JSON.parse(String(mutation?.init?.body))).toEqual({action:"remove",expected_revision:2,expected_digest:"a".repeat(64)});
});
