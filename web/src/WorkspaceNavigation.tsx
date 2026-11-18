import { useState } from "react";
import type { ThreadSummary } from "../shared/api-types";

type IconName = "chat" | "feed" | "idea" | "goal" | "library" | "search" | "new" | "menu";
const paths: Record<IconName, string> = {
  chat: "M4 5h16v11H9l-5 4z",
  feed: "M5 6h2m4 0h8M5 12h2m4 0h8M5 18h2m4 0h8",
  idea: "M9 21h6m-6-4v-2a6 6 0 1 1 6 0v2z",
  goal: "M12 3a9 9 0 1 0 9 9M12 7a5 5 0 1 0 5 5m-5 0 8-8m0 0v4m0-4h-4",
  library: "M3 6h7l2 2h9v11H3z",
  search: "M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zm5 12 4 4",
  new: "M12 5v14M5 12h14",
  menu: "M4 6h16M4 12h16M4 18h11",
};
export function WorkspaceIcon({name}: {name: IconName}) {
  return <svg aria-hidden="true" width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round"><path d={paths[name]}/></svg>;
}

export function ConversationSearch({threads, openThread, openSources}: {
  threads: ThreadSummary[];
  openThread: (id: string) => void;
  openSources: () => void;
}) {
  const [query, setQuery] = useState("");
  const normalized = query.trim().normalize("NFKC").toLocaleLowerCase();
  const results = threads.filter(thread => thread.title.normalize("NFKC").toLocaleLowerCase().includes(normalized));
  return <div className="workspace-search">
    <label>대화 제목<input autoFocus type="search" autoComplete="off" maxLength={200} placeholder="제목으로 찾기" value={query}
      onChange={event => setQuery(event.target.value)}
      onKeyDown={event => {
        if (event.key === "Enter" && !event.nativeEvent.isComposing && results.length === 1) openThread(results[0].id);
      }} /></label>
    <div role="status" className="workspace-search-status">{normalized ? `${results.length}개 대화` : "최근 대화"}</div>
    <ul>{results.slice(0, 30).map(thread => <li key={thread.id}>
      <button onClick={() => openThread(thread.id)}><WorkspaceIcon name="chat"/>{thread.title}</button>
    </li>)}</ul>
    {results.length === 0 && <p>일치하는 대화가 없습니다.</p>}
    {results.length > 30 && <p>제목을 더 입력해 범위를 좁혀 보세요.</p>}
    <button className="text-button" onClick={openSources}>저장한 자료 검색 열기</button>
  </div>;
}
