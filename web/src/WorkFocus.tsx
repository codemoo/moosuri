import { eligibleActions, scopeName, taskStateLabel, type ConversationWorkGroup } from "./conversation-work";
import { DateChoices, GroupBody, ReportViewer, summaryStyle, type WorkApi } from "./ConversationWork";
import type { WorkFocusKind } from "./work-interpret";

export type WorkFocusState = { thread: string; groupId: string; focus: WorkFocusKind; taskId: string | null };

/** Shows what the server pointed at, using only the live group the member already reads
 * (summary, source count and stored report ids). Nothing here is copied into chat history. */
export function WorkFocus({ state, group, work, openRun, onClose }: {
  state: WorkFocusState; group: ConversationWorkGroup | undefined; work: WorkApi;
  openRun: (runId: string) => void; onClose: () => void;
}) {
  if (!group) return null;
  const tasks = state.taskId ? group.tasks.filter(t => t.id === state.taskId) : group.tasks;
  const close = <button type="button" onClick={onClose}>닫기</button>;
  let body;
  if (state.focus === "tasks") {
    body = <GroupBody group={group} work={work} openRun={openRun} />;
  } else if (state.focus === "status") {
    body = <ul>{tasks.map(t => <li key={t.id}>{t.title} — {taskStateLabel[t.state]}{t.state === "completed" ? ` (근거 ${t.source_count}건)` : ""}</li>)}</ul>;
  } else if (state.focus === "evidence") {
    body = <ul className="work-evidence">{tasks.map(t => <li key={t.id}>
      <strong>{t.title}</strong>
      <p className="work-meta">{scopeName[t.scope]} 검색 · “{t.query || "검색어 없음"}” · {taskStateLabel[t.state]}
        {t.state === "completed" ? ` · 근거 ${t.source_count}건 (기록 당시)` : ""}</p>
      {t.state === "completed" && t.summary && <p className="work-summary" style={summaryStyle(t.summary)}>{t.summary}</p>}
      <DateChoices group={group} task={t} work={work} />
      {t.state === "completed" && !t.summary && <p className="work-meta">지금은 요약을 읽을 수 없습니다. 상세 기록에서 확인하세요.</p>}
      {t.state !== "completed" && <p className="work-meta">아직 보여 줄 결과가 없습니다.</p>}
      {t.run_id && <button type="button" onClick={() => openRun(t.run_id!)}>상세·근거 보기</button>}
    </li>)}</ul>;
  } else {
    const task = state.taskId ? tasks[0] : undefined;
    const reportId = task ? task.report_id : group.group_report_id ?? null;
    body = reportId ? <ReportViewer key={reportId} id={reportId} work={work} label="보고서" />
      : !task && group.report_requested ? <p className="work-meta" role="status">묶음 보고서를 준비하는 중입니다.</p>
      : task && eligibleActions(task).includes("save_report")
        ? <p className="work-meta">이 검색의 보고서는 아직 없습니다. <button type="button" disabled={work.busy.has(task.id)}
            onClick={() => void work.act(group, task, "save_report")}>보고서 만들기</button></p>
      : <p className="work-meta" role="status">저장된 보고서가 아직 없습니다.</p>;
    if (task && work.notices[task.id]) body = <>{body}<p role="alert" className="work-notice">{work.notices[task.id]}</p></>;
  }
  return <div className="member-chat-notice work-focus" role="region" aria-label="요청한 작업 결과" data-focus={state.focus}>
    {body}
    <div className="member-retry-actions">{close}</div>
  </div>;
}
