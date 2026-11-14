import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { CommunitySkills } from "./CommunitySkills";
import type { CommunitySkillCard, SkillSharePreview } from "../shared/api-types";
const card: CommunitySkillCard = { id:"private:test",name:"정확한 합계 계산",description:"새 입력으로 계산",source:"private",source_commit:"",version:"1",digest:"a".repeat(64),license:"private",capabilities:["decimal_calculate"],installed_version:"1",installed_digest:"a".repeat(64),enabled:true,hidden:false,revision:2,can_rollback:false,readiness:"ready",missing:[] };
const preview: SkillSharePreview = { slug:"exact-decimal-sum",files:{"SKILL.md":"Original synthetic sharing instructions","examples.json":"Synthetic example only","moosuri.toml":"mode = instructions","LICENSE":"Apache-2.0"},archive_base64:btoa("synthetic archive bytes") };
const props = { onExpired:vi.fn(),back:vi.fn(),connect:vi.fn(),start:vi.fn() };
const status = {configured:false,source:null,state:"disabled",verified_at:null,checked_at:null,stale:false};

it("previews on request and downloads only the reconstructed archive after the user's click", async () => {
  const create = vi.fn(() => "blob:synthetic-export"); vi.stubGlobal("URL", class extends URL {static createObjectURL=create;static revokeObjectURL=vi.fn();});
  const click=vi.spyOn(HTMLAnchorElement.prototype,"click").mockImplementation(()=>{});
  const fetch=vi.fn(async(path:string,_init?:RequestInit)=>Response.json(path.endsWith("/share-preview")?preview:path.endsWith("/catalog/status")?status:[card]));
  vi.stubGlobal("fetch",fetch);
  render(<CommunitySkills accountId="owner-a" {...props}/>);
  fireEvent.click(await screen.findByRole("button",{name:"절차 공유하기"}));
  await screen.findByText("Original synthetic sharing instructions");
  expect(create).not.toHaveBeenCalled();expect(click).not.toHaveBeenCalled();
  const request=fetch.mock.calls.find(([path])=>path.endsWith("/share-preview"))!;
  expect(JSON.parse(String(request[1]?.body))).toEqual({expected_revision:2});
  expect((request[1]?.headers as Record<string,string>)["X-Moosuri-Account"]).toBe("owner-a");
  fireEvent.click(screen.getByRole("button",{name:"공유 파일 내려받기"}));
  expect(create).toHaveBeenCalledOnce();expect(click).toHaveBeenCalledOnce();
  expect((click.mock.instances[0] as HTMLAnchorElement).download).toBe("exact-decimal-sum.tar");
  expect(fetch.mock.calls.filter(([,init])=>init?.method==="POST")).toHaveLength(1);
  click.mockRestore();
});

it("does not display an old account's delayed sharing response", async () => {
  let release!:(value:Response)=>void;
  const fetch=vi.fn(async(path:string,init?:RequestInit)=>path.endsWith("/share-preview")?await new Promise<Response>(r=>release=r):Response.json(path.endsWith("/catalog/status")?status:(init?.headers as Record<string,string>)["X-Moosuri-Account"]==="owner-a"?[card]:[]));
  vi.stubGlobal("fetch",fetch);
  const view=render(<CommunitySkills accountId="owner-a" {...props}/>);
  fireEvent.click(await screen.findByRole("button",{name:"절차 공유하기"}));
  await waitFor(()=>expect(release).toBeDefined());
  view.rerender(<CommunitySkills accountId="owner-b" {...props}/>);
  await act(async()=>release(Response.json(preview)));
  expect(screen.queryByText("Original synthetic sharing instructions")).not.toBeInTheDocument();
  expect(screen.queryByRole("button",{name:"공유 파일 내려받기"})).not.toBeInTheDocument();
});
