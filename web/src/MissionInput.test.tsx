import {render,screen,fireEvent,cleanup} from "@testing-library/react";
import {it,expect,vi,afterEach} from "vitest";
import {Missions} from "./Missions";
import type {MissionView} from "../shared/api-types";
afterEach(()=>{cleanup();vi.unstubAllGlobals()});
it("keeps the typed question on the original card and retries an uncertain reply with identical identity",async()=>{
 const mission:MissionView={id:"original",revision:1,state:"waiting",request:{request_id:"original-request",task:"Read Gmail and prepare an email draft",constraints:"",checks:[],execution_mode:"durable"},steering:[],plan:["Read","Draft"],completed_steps:1,result:null,receipts:[],report:[],uncertain:false,expires_at_ms:2000000000000,input_wait:{request_id:"original-wait",field:"mail_to",question:"메일을 받을 주소는 무엇인가요?",state:"waiting",value:null,acceptance_revision:1,expires_at_ms:2000000000000}};
 const writes:unknown[]=[];let item=mission;
 vi.stubGlobal("fetch",vi.fn(async(url,init)=>{if(init?.method==="POST"){expect(String(url)).toBe("/api/v1/missions/original/input");writes.push(JSON.parse(String(init.body)));if(writes.length===1)throw new TypeError("synthetic lost ACK");item={...mission,input_wait:{...mission.input_wait!,state:"ready",value:"recipient@example.test"}};return new Response(JSON.stringify(item))}return new Response(JSON.stringify([item]))}));
 const page=render(<Missions accountId="owner" onExpired={vi.fn()} back={vi.fn()}/>);
 await screen.findByText("메일을 받을 주소는 무엇인가요?");page.unmount();render(<Missions accountId="owner" onExpired={vi.fn()} back={vi.fn()}/>);
 const address=await screen.findByLabelText("받는 사람 이메일");fireEvent.change(address,{target:{value:"recipient@example.test"}});fireEvent.click(screen.getByRole("button",{name:"답변하고 같은 일 이어서 진행"}));await screen.findByRole("alert");expect(address).toHaveValue("recipient@example.test");fireEvent.click(screen.getByRole("button",{name:"답변하고 같은 일 이어서 진행"}));await screen.findByText(/답변 저장됨/);expect(writes).toHaveLength(2);expect(writes[1]).toEqual(writes[0]);expect(writes[0]).toMatchObject({wait_request_id:"original-wait",expected_revision:1,value:"recipient@example.test"});expect(screen.queryByRole("button",{name:"답변하고 같은 일 이어서 진행"})).not.toBeInTheDocument();
});
