import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { SelectedBrowserMission } from "./SelectedBrowserMission";
afterEach(()=>{cleanup();vi.unstubAllGlobals()});
it("links the selected page to the original task and rechecks the same lost acknowledgement",async()=>{
  const writes:unknown[]=[];
  vi.stubGlobal("fetch",vi.fn(async(_url,init)=>{
    if(init?.method==="POST"){
      writes.push(JSON.parse(String(init.body)));
      if(writes.length===1)throw new TypeError("synthetic lost ACK");
      return new Response(JSON.stringify({request_id:"r",mission_id:"original",source_id:"source",source_revision:1,url:"https://example.test/",title:"Synthetic",truncated:false,observed_at_ms:1}));
    }
    return new Response(JSON.stringify([{id:"original",revision:3,state:"paused",request:{task:"Compare synthetic offers"},expires_at_ms:Date.now()+60000,uncertain:false},{id:"expired",revision:1,state:"paused",request:{task:"Expired task"},expires_at_ms:1,uncertain:false}]));
  }));
  render(<SelectedBrowserMission accountId="owner" threadId="thread" workspaceId="workspace" revision={4} controlToken="synthetic-control" frameToken={"0".repeat(64)} disabled={false} onBusy={vi.fn()} onExpired={vi.fn()}/>);
  fireEvent.click(screen.getByRole("button",{name:"현재 페이지를 맡긴 일에 사용"}));
  await screen.findByRole("option",{name:"Compare synthetic offers"});
  expect(screen.queryByRole("option",{name:"Expired task"})).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button",{name:"이 페이지로 업무 이어가기"}));
  await screen.findByRole("alert");
  fireEvent.click(screen.getByRole("button",{name:"페이지 연결 결과 다시 확인"}));
  expect(await screen.findByRole("link",{name:"진행 보기"})).toHaveAttribute("href","/app/missions#mission-original");
  expect(writes).toHaveLength(2);expect(writes[0]).toEqual(writes[1]);
  expect(writes[0]).toMatchObject({mission_id:"original",mission_revision:3,workspace_id:"workspace",revision:4});
});
