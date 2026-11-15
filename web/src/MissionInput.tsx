import {useRef,useState,type FormEvent} from "react";
import type {MissionInputReply,MissionView} from "../shared/api-types";
import {ApiFailure} from "./api";
import {useScope} from "./account-scope";
export function MissionInput({mission,scope,changed,failure}:{mission:MissionView;scope:ReturnType<typeof useScope>;changed:()=>Promise<unknown>;failure:(e:unknown)=>void}){
 const wait=mission.input_wait!;const [value,setValue]=useState("");const [busy,setBusy]=useState(false);const pending=useRef<MissionInputReply|null>(null);
 const fields={mail_to:{label:"받는 사람 이메일",type:"email"},calendar_date:{label:"일정 날짜",type:"date"},calendar_start:{label:"일정 시작 시각",type:"time"},calendar_duration:{label:"일정 길이 (분)",type:"number"}};
 const field=fields[wait.field];
 async function submit(e:FormEvent){e.preventDefault();if(busy||!value.trim())return;setBusy(true);pending.current??={request_id:crypto.randomUUID(),wait_request_id:wait.request_id,expected_revision:mission.revision,value};try{await scope.request(`/missions/${mission.id}/input`,"POST",pending.current);pending.current=null;await changed()}catch(e){if(e instanceof ApiFailure&&e.status>=400&&e.status<500)pending.current=null;failure(e)}finally{setBusy(false)}}
 return <section aria-label="업무에 필요한 값"><p role="status">{wait.state==="waiting"?"답변이 필요해요":wait.state==="ready"?"답변 저장됨 · 앞선 업무가 끝나면 같은 일을 이어서 진행해요":wait.state==="expired"?"답변 대기 유효 기간이 끝났어요":"답변 대기가 종료됐어요"}</p><p>{wait.question}</p>{wait.state==="waiting"&&<form onSubmit={submit}><label>{field.label}<input required disabled={busy} type={field.type} min={wait.field==="calendar_duration"?1:undefined} max={wait.field==="calendar_duration"?1440:undefined} maxLength={254} value={value} onChange={e=>{setValue(e.target.value);pending.current=null}}/></label><button disabled={busy||!value.trim()} type="submit">답변하고 같은 일 이어서 진행</button><p>처음 맡긴 작업에 이 값만 채워요. 메일 발송은 최종 확인 화면에서 허락해야 해요.</p></form>}<p>원래 종료 시각: {new Date(wait.expires_at_ms).toLocaleString()}</p></section>
}
