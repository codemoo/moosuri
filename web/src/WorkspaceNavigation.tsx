import { Button } from "@astryxdesign/core/Button";
import { TextInput } from "@astryxdesign/core/TextInput";
import { useState } from "react";
import type { ThreadSummary } from "../shared/api-types";

type IconName = "bubble" | "notebook" | "mail" | "link" | "vault" | "calendar" | "drive" | "slack" | "back" | "chat" | "feed" | "idea" | "goal" | "library" | "search" | "new" | "mic" | "refresh";
const paths: Record<IconName, string> = {
  bubble: "M21 11.5a8.5 8.5 0 0 1-8.8 8.3 11 11 0 0 1-3.7-.7L4 21l1.2-4.4a8 8 0 0 1-2-5.1A8.6 8.6 0 0 1 12.2 3 8.6 8.6 0 0 1 21 11.5z",
  notebook: "M6 4h12a1 1 0 0 1 1 1v14a1 1 0 0 1-1 1H7a3 3 0 0 1-3-3V6a2 2 0 0 1 2-2zm2 0v16m3-11h5m-5 4h4",
  mail: "M3 6h18v12H3zm0 0 9 7 9-7",
  link: "M10 13a4 4 0 0 0 6 .5l4-4a4 4 0 0 0-5.7-5.7L12 6m2 5a4 4 0 0 0-6-.5l-4 4a4 4 0 0 0 5.7 5.7L12 18",
  vault: "M5 10h14v11H5zm3 0V7a4 4 0 0 1 8 0v3m-4 5v2",
  calendar: "M4 6h16v15H4zm0 5h16M8 3v5m8-5v5m-7 7h.01m5 0h.01",
  drive: "M8 4h8l6 11-4 5H6l-4-5zm0 0 10 16m-2-16L6 20m-4-5h20",
  slack: "M8 4v5H4m12-5v5h4M8 20v-5H4m12 5v-5h4M9 3v18m6-18v18M3 9h18M3 15h18",
  back: "m15 5-7 7 7 7",
  chat: "M4 5h16v11H9l-5 4z",
  feed: "M5 6h2m4 0h8M5 12h2m4 0h8M5 18h2m4 0h8",
  idea: "M9 21h6m-6-4v-2a6 6 0 1 1 6 0v2z",
  goal: "M12 3a9 9 0 1 0 9 9M12 7a5 5 0 1 0 5 5m-5 0 8-8m0 0v4m0-4h-4",
  library: "M3 6h7l2 2h9v11H3z",
  search: "M11 4a7 7 0 1 0 0 14 7 7 0 0 0 0-14zm5 12 4 4",
  new: "M12 5v14M5 12h14",
  mic: "M9 6a3 3 0 0 1 6 0v5a3 3 0 0 1-6 0zM6 11a6 6 0 0 0 12 0m-6 6v4m-3 0h6",
  refresh: "M19 8a8 8 0 1 0 .8 6M19 3v5h-5",
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
    <TextInput label="대화 제목" isLabelHidden type="search" placeholder="대화 제목 검색" size="lg" width="100%"
      value={query} hasAutoFocus startIcon={<WorkspaceIcon name="search"/>}
      onChange={value => setQuery(value.slice(0, 200))}
      onKeyDown={event => {
        if (event.key === "Enter" && !event.nativeEvent.isComposing && results.length === 1) openThread(results[0].id);
      }} />
    <div role="status" className="workspace-search-status">{normalized ? `${results.length}개 대화` : "최근 대화"}</div>
    <ul>{results.slice(0, 30).map(thread => <li key={thread.id}>
      <button onClick={() => openThread(thread.id)}><WorkspaceIcon name="chat"/>{thread.title}</button>
    </li>)}</ul>
    {results.length === 0 && <p>일치하는 대화가 없습니다.</p>}
    {results.length > 30 && <p>제목을 더 입력해 범위를 좁혀 보세요.</p>}
    <Button variant="ghost" label="자료 검색" onClick={openSources}/>
  </div>;
}
