import {render,screen,fireEvent,cleanup,waitFor} from "@testing-library/react";
import {it,expect,vi,afterEach} from "vitest";
import {NativeMailWait} from "./NativeMailWait";
import type {MissionWaitView} from "../shared/api-types";
const wait:MissionWaitView={request_id:"original-wait",slot_id:"native-mail-slot-v1",run_id:"original-run",step:2,acceptance_revision:1,cause:"account_selection",state:"waiting",expires_at_ms:2000000000000,choices:[{binding_id:"opaque-device:opaque-binding",device_id:"opaque-device",operation:"google.mail.model_read.v1",resource:"mailbox",schema_version:1,state:"ready",expires_at_ms:2000000000000,alias:"연구용 Gmail"}]};
afterEach(()=>{cleanup();vi.unstubAllGlobals()});
it("retries the original precise native account choice after a lost acknowledgement",async()=>{
 const writes:any[]=[];vi.stubGlobal("fetch",vi.fn(async(url,init)=>{expect(String(url)).toBe("/api/v1/native-mail/original-run/connection");writes.push(JSON.parse(String(init.body)));if(writes.length===1)throw new TypeError("lost acknowledgement");return new Response(null,{status:204})}));
 render(<NativeMailWait accountId="owner" runId="original-run" revision={7} wait={wait} openSettings={vi.fn()} onExpired={vi.fn()}/>);
 fireEvent.click(screen.getByRole("button",{name:"연구용 Gmail"}));await screen.findByRole("alert");fireEvent.click(screen.getByRole("button",{name:"연구용 Gmail"}));await waitFor(()=>expect(writes).toHaveLength(2));expect(writes[1]).toEqual(writes[0]);expect(writes[0]).toMatchObject({wait_request_id:"original-wait",expected_revision:7,slot_id:"native-mail-slot-v1",binding_id:"opaque-device:opaque-binding"});
});
it("keeps an unknown dispatched read out of routine unlock and account actions",()=>{
 render(<NativeMailWait accountId="owner" runId="original-run" revision={7} wait={{...wait,state:"unknown",choices:[]}} openSettings={vi.fn()} onExpired={vi.fn()}/>);expect(screen.getByText("기기에서 시작한 읽기 결과 확인 필요")).toBeInTheDocument();expect(screen.queryByRole("button")).not.toBeInTheDocument();
});
