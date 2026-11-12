import type {WebEvidence as Observation} from "../shared/api-types";

/** Only server-recorded current-run observations. Public pages are untrusted. */
export function WebEvidence({observations}:{observations?:Observation[]}) {
  const safe=(observations??[]).filter((item)=>{
    try { const url=new URL(item.url);return (url.protocol==="https:"||url.protocol==="http:")&&!url.username&&!url.password; } catch {return false;}
  });
  if(!safe.length)return null;
  return <div className="chat-evidence" aria-label="이번 실행의 웹 관측 자료">
    <span>이번 실행에서 관측한 웹 자료 · 답변 전체의 검증을 뜻하지 않습니다.</span>
    {safe.map((item,index)=><a key={`${item.url}:${index}`} href={item.url} target="_blank" rel="noopener noreferrer" title={`${item.url} · ${new Date(item.retrieved_at_unix_ms).toLocaleString()}`}>
      {item.title||new URL(item.url).hostname} ↗ {item.kind==="search_hit"?"(검색 요약)":"(HTTP 페이지)"}{item.truncated?" · 일부만 확인":""}
    </a>)}
  </div>;
}
