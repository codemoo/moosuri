import {useState} from "react";
import {useScope} from "./account-scope";
import {ApiFailure,isAborted} from "./api";
import type {CommunitySkillCard} from "../shared/api-types";
export function CompoundRecipeEditor({accountId,card,onExpired,edit}:{accountId:string;card:CommunitySkillCard;onExpired:()=>void;edit:(calculation:unknown)=>Promise<boolean>}) {
 const scope=useScope(accountId,"compound-recipe-edit");const [text,setText]=useState<string|null>(null),[busy,setBusy]=useState(false),[message,setMessage]=useState("");
 function fail(e:unknown){if(isAborted(e))return;if(e instanceof ApiFailure&&e.status===401){onExpired();return}setMessage("절차를 저장하지 못했어요. 현재 버전과 입력 구조를 확인해 주세요.")}
 async function open(){setBusy(true);try{const v=await scope.request<{calculation:unknown}>(`/community-skills/${encodeURIComponent(card.id)}/recipe`);setText(JSON.stringify(v.calculation,null,2))}catch(e){fail(e)}finally{setBusy(false)}}
 async function save(){if(busy||text===null)return;setBusy(true);setMessage("");try{if(await edit(JSON.parse(text)))setText(null)}catch(e){fail(e)}finally{setBusy(false)}}
 return <div>{text===null?<button disabled={busy} onClick={()=>void open()}>계산 구조 편집</button>:<><label>계산 입력·참조 구조<textarea aria-label="계산 구조" rows={12} value={text} disabled={busy} onChange={e=>setText(e.target.value)}/></label><p>저장하면 새 버전이 돼요. 기존 증거를 지우고 사용한 결과와 관련 예약을 중단해요. 새 버전은 서로 다른 새 결과 두 개를 채택해 다시 검증해야 해요.</p><button disabled={busy} onClick={()=>void save()}>새 버전으로 저장</button><button disabled={busy} onClick={()=>setText(null)}>닫기</button></>}{message&&<p role="alert">{message}</p>}</div>
}
