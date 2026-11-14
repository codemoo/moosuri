import { describe, expect, it } from "vitest";
import { parseEvent, parsePage, validRun } from "./chat-state";
const receipt={action_id:"synthetic-action",operation:"delete",state:"observed_absent",event_id:"event1",provider_etag:null,message:"Current absence, causality unknown."};
const run={id:"run",request_id:"request",thread_id:"thread",attempt:1,state:"completed",version:2,model:{provider_id:"p",provider_revision:1,provider_name:"fixture",kind:"compatible",model:"fixture",effort:null,function_tools:true,verified_at:1},error:null,usage:{input_tokens:null,output_tokens:null,cache_read_tokens:null,cache_write_tokens:null},evidence:[],can_retry:false,can_cancel:false,created_at:1,calendar_receipt:receipt};
describe("Calendar receipt boundary",()=>{
 it("accepts a bounded trusted receipt and keeps older snapshots compatible",()=>{expect(validRun(run)).toBe(true);expect(parseEvent({sequence:2,change:{kind:"state",run}},"2").sequence).toBe(2);const {calendar_receipt:_,...old}=run;expect(validRun(old)).toBe(true)});
 it.each([{}, {...receipt,state:"success invented"},{...receipt,message:{private:"invalid"}},{...receipt,message:"x".repeat(1025)},{...receipt,provider_etag:"x".repeat(513)}])("rejects malformed receipt in snapshots and events",calendar_receipt=>{
   const malformed={...run,calendar_receipt};expect(validRun(malformed)).toBe(false);
   expect(()=>parsePage({thread_id:"thread",cursor:1,next_before:null,turns:[{sequence:1,user_text:"Synthetic",assistant:[],run:malformed}]},"thread")).toThrow();
   expect(()=>parseEvent({sequence:2,change:{kind:"state",run:malformed}},"2")).toThrow();
 });
});


it("restores the server's no-connection personal Calendar receipt in snapshots and SSE",()=>{
  const missing={action_id:null,operation:"create",state:"connection_needed",event_id:null,provider_etag:null,message:"Calendar 연결이 필요해요. /settings/calendar 에서 내 Google Calendar를 연결한 뒤 새 요청을 보내 주세요. 아직 변경하지 않았어요. / Connect your Calendar in settings and send a new request. No change was made."};
  const personal={...run,model:{...run.model,source:"personal_chat_gpt"},calendar_receipt:missing};
  const turn={sequence:1,user_text:"Create Synthetic meeting on 2026-12-01 from 10:00 to 11:00",assistant:[{id:"calendar-receipt",text:missing.message,phase:"final_answer",refusal:false}],run:personal};
  const saved={thread_id:"thread",cursor:2,next_before:null,turns:[turn]};
  const restored=parsePage(saved,"thread");
  expect(restored.turns[0].run.calendar_receipt?.state).toBe("connection_needed");
  expect(restored.turns[0].assistant[0].text).toContain("/settings/calendar");
  expect(restored.turns[0].run.calendar_receipt?.action_id).toBeNull();
  expect(parseEvent({sequence:2,change:{kind:"state",run:personal}},"2").change.kind).toBe("state");
  expect(parseEvent({sequence:3,change:{kind:"turn",turn}},"3").change.kind).toBe("turn");
});
