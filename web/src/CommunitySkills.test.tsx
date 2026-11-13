import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { CommunitySkills } from "./CommunitySkills";
import type { CommunitySkillCard } from "../shared/api-types";
const card = (extra: Partial<CommunitySkillCard> = {}): CommunitySkillCard => ({id:"source-briefing",name:"Saved briefing",description:"Ground saved evidence",source:"https://github.com/codemoo/moosuri",source_commit:"a".repeat(40),version:"1.0.0",digest:"a".repeat(64),license:"Apache-2.0",capabilities:["knowledge_search"],installed_version:null,installed_digest:null,enabled:false,hidden:false,revision:0,can_rollback:false,readiness:"installable",missing:[],...extra});
const response = (value: unknown) => new Response(JSON.stringify(value),{status:200,headers:{"Content-Type":"application/json"}});
beforeEach(()=>vi.restoreAllMocks());
it("installs only on deliberate action, preserves package revision, and starts via chat without hidden execution",async()=>{
 const calls: {path:string;init?:RequestInit}[]=[];const start=vi.fn();
 vi.stubGlobal("fetch",vi.fn(async(path:string,init?:RequestInit)=>{calls.push({path,init});return response([init?.method==="POST"?card({installed_digest:"a".repeat(64),installed_version:"1.0.0",enabled:true,readiness:"ready",revision:1}):card()]);}));
 render(<CommunitySkills accountId="a" onExpired={vi.fn()} back={vi.fn()} connect={vi.fn()} start={start}/>);
 fireEvent.click(await screen.findByRole("button",{name:"추가"}));
 await screen.findByRole("button",{name:"채팅에서 시작"});
 expect(calls.filter(c=>c.init?.method==="POST")).toHaveLength(1);
 expect(JSON.parse(String(calls[1].init?.body))).toEqual({action:"install",expected_revision:0,expected_digest:"a".repeat(64)});
 expect((calls[1].init?.headers as Record<string,string>)["X-Moosuri-Account"]).toBe("a");
 expect(start).not.toHaveBeenCalled();fireEvent.click(screen.getByRole("button",{name:"채팅에서 시작"}));expect(start).toHaveBeenCalledWith("Saved briefing");expect(calls).toHaveLength(2);
});
it("does not advertise missing connections as ready and persists hiding through stable package action",async()=>{
 const connect=vi.fn();const fetch=vi.fn(async(_path:string,init?:RequestInit)=>response([card({readiness:"connection_needed",missing:["mail_read"],hidden:init?.method==="POST",revision:init?.method==="POST"?1:0})]));vi.stubGlobal("fetch",fetch);
 render(<CommunitySkills accountId="a" onExpired={vi.fn()} back={vi.fn()} connect={connect} start={vi.fn()}/>);
 fireEvent.click(await screen.findByRole("button",{name:"Gmail 연결하기"}));expect(connect).toHaveBeenCalledOnce();expect(screen.queryByRole("button",{name:"채팅에서 시작"})).not.toBeInTheDocument();
 fireEvent.click(screen.getByRole("button",{name:"제안 숨기기"}));await waitFor(()=>expect(screen.queryByText("Saved briefing")).not.toBeInTheDocument());
 fireEvent.click(screen.getByRole("checkbox"));expect(screen.getByText("Saved briefing")).toBeInTheDocument();expect(JSON.parse(String(fetch.mock.calls[1][1]?.body)).action).toBe("hide");
});
it("fences late installation results and local forms when the account changes",async()=>{
 let resolve!: (value:Response)=>void;const late=new Promise<Response>(r=>resolve=r);
 const fetch=vi.fn(async(_path:string,init?:RequestInit)=>init?.method==="POST"?late:response((init?.headers as Record<string,string>)["X-Moosuri-Account"]==="a"?[card()]:[]));vi.stubGlobal("fetch",fetch);
 const props={onExpired:vi.fn(),back:vi.fn(),connect:vi.fn(),start:vi.fn()};const view=render(<CommunitySkills accountId="a" {...props}/>);
 fireEvent.click(await screen.findByRole("button",{name:"추가"}));view.rerender(<CommunitySkills accountId="b" {...props}/>);
 await waitFor(()=>expect(fetch).toHaveBeenCalledTimes(3));await act(async()=>resolve(response([card({name:"Private old account response",installed_digest:"a".repeat(64),enabled:true})])));
 expect(screen.queryByText("Private old account response")).not.toBeInTheDocument();expect(screen.queryByText("Saved briefing")).not.toBeInTheDocument();
});
