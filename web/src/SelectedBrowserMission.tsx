import { useRef, useState } from "react";
import type { BrowserMissionReceipt, BrowserMissionSelection, MissionView } from "../shared/api-types";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";

/** Human-only explicit page selection. No page text, cookies or frames in this DTO. */
export function SelectedBrowserMission(props: {
  accountId: string; threadId: string; workspaceId: string; revision: number;
  controlToken: string; frameToken: string; disabled: boolean; onExpired: () => void;
  onBusy: (busy: boolean) => void; onChanged?: () => void;
}) {
  const scope=useScope(props.accountId,`${props.threadId}:${props.workspaceId}:${props.revision}`);
  const [choices,setChoices]=useState<MissionView[]|null>(null);
  const [selected,setSelected]=useState("");
  const [busy,setBusy]=useState(false);
  const [error,setError]=useState("");
  const [receipt,setReceipt]=useState<BrowserMissionReceipt|null>(null);
  const pending=useRef<BrowserMissionSelection|null>(null);
  function failure(e: unknown) {
    if(isAborted(e))return;
    if(e instanceof ApiFailure && e.status===401){props.onExpired();return}
    if(e instanceof ApiFailure && [400,403,404,409].includes(e.status)) {
      pending.current=null;setChoices(null);setSelected("");
      setError("화면이나 업무 상태가 바뀌었어요. 현재 페이지와 업무를 다시 선택해 주세요.");return;
    }
    setError("페이지 연결 결과를 확인하지 못했어요. 같은 요청으로 다시 확인할 수 있어요.");
  }
  async function load() {
    setBusy(true);props.onBusy(true);setError("");
    try {
      const missions=await scope.request<MissionView[]>("/missions");
      const eligible=missions.filter(m=>["paused","queued"].includes(m.state)&&!m.uncertain&&m.expires_at_ms>Date.now());
      setChoices(eligible);setSelected(eligible[0]?.id??"");
    } catch(e){failure(e)} finally {setBusy(false);props.onBusy(false)}
  }
  async function attach() {
    const mission=choices?.find(m=>m.id===selected);
    if(!mission&&!pending.current)return;
    pending.current??={request_id:crypto.randomUUID(),frame_token:props.frameToken,workspace_id:props.workspaceId,revision:props.revision,control_token:props.controlToken,mission_id:mission!.id,mission_revision:mission!.revision};
    setBusy(true);props.onBusy(true);setError("");
    try {
      const result=await scope.request<BrowserMissionReceipt>(`/threads/${encodeURIComponent(props.threadId)}/browser/mission`,"POST",pending.current);
      setReceipt(result);pending.current=null;props.onChanged?.();
    } catch(e){failure(e)} finally {setBusy(false);props.onBusy(false)}
  }
  if(receipt)return <p role="status">페이지를 원래 업무에 연결했어요. <a href={`/app/missions#mission-${encodeURIComponent(receipt.mission_id)}`}>진행 보기</a></p>;
  return <div className="browser-mission-selection">
    {choices===null?<button className="secondary" disabled={props.disabled||busy} onClick={()=>void load()}>현재 페이지를 맡긴 일에 사용</button>:<>
      <p>현재 페이지의 읽을 수 있는 본문을 내 자료로 저장하고 선택한 업무를 이어가요. 이 브라우저는 서버에서 실행됩니다.</p>
      {choices.length===0?<p>연결할 대기·정지 업무가 없어요. 진행 중인 업무는 먼저 잠시 멈춰 주세요.</p>:<>
        <label>이어서 처리할 업무<select value={selected} disabled={busy||!!pending.current} onChange={e=>setSelected(e.target.value)}>{choices.map(m=><option key={m.id} value={m.id}>{m.request.task}</option>)}</select></label>
        <button disabled={busy||props.disabled||!selected} onClick={()=>void attach()}>{pending.current?"페이지 연결 결과 다시 확인":"이 페이지로 업무 이어가기"}</button>
      </>}
    </>}
    {error&&<p role="alert">{error}</p>}
  </div>;
}
