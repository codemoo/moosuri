import { useEffect, useRef, useState } from "react";
import type { CommunitySkillCard, GitHubSkillLocation } from "../shared/api-types";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";
import "./community-skills.css";
import { GitHubSkillImport } from "./GitHubSkillImport";

type Props = {
  accountId: string;
  onExpired: () => void;
  back: () => void;
  connect: () => void;
  start: (name: string) => void;
};
export function CommunitySkills(props: Props) {
  return <Cards key={props.accountId} {...props} />;
}
function Cards({ accountId, onExpired, back, connect, start }: Props) {
  const scope = useScope(accountId);
  const [cards, setCards] = useState<CommunitySkillCard[] | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [showHidden, setShowHidden] = useState(false);
  const [importSource,setImportSource]=useState<GitHubSkillLocation | undefined>(undefined);
  const [importOpen,setImportOpen]=useState(false);
  const catalogGeneration = useRef(0);
  const listController = useRef<AbortController | null>(null);
  // The parent owns every full-catalog mutation, including importer activation.
  // A synchronous owner guard also fences two events before React rerenders.
  const mutationOwner = useRef<symbol | null>(null);
  function fail(e: unknown) {
    if (isAborted(e)) return;
    if (e instanceof ApiFailure && e.status === 401) onExpired();
    else setError("상태를 확인하지 못했습니다. 새로고침한 뒤 다시 시도하세요.");
  }
  function refresh() {
    if (mutationOwner.current) return;
    const generation = ++catalogGeneration.current;
    listController.current?.abort();
    const controller = new AbortController();
    listController.current = controller;
    setError("");
    void scope.request<CommunitySkillCard[]>(
      "/community-skills", "GET", undefined, controller.signal,
    ).then(snapshot => {
      if (generation === catalogGeneration.current && !controller.signal.aborted)
        setCards(snapshot);
    }).catch(error => {
      if (generation === catalogGeneration.current) fail(error);
    }).finally(() => {
      if (listController.current === controller) listController.current = null;
    });
  }
  useEffect(() => {
    refresh();
    return () => {
      catalogGeneration.current++;
      listController.current?.abort();
    };
  }, [scope]);
  async function action(card: CommunitySkillCard, action: string): Promise<boolean> {
    if (mutationOwner.current) return false;
    const owner = Symbol();
    mutationOwner.current = owner;
    const generation = ++catalogGeneration.current;
    // A list captured before this mutation must never replace its accepted state.
    listController.current?.abort();
    listController.current = null;
    setBusy(true);
    setError("");
    try {
      const snapshot = await scope.request<CommunitySkillCard[]>(
        `/community-skills/${encodeURIComponent(card.id)}`, "POST",
        { action, expected_revision: card.revision, expected_digest: card.digest },
      );
      if (generation !== catalogGeneration.current || mutationOwner.current !== owner)
        return false;
      setCards(snapshot);
      return true;
    } catch (error) {
      if (generation === catalogGeneration.current) fail(error);
      return false;
    } finally {
      if (mutationOwner.current === owner) {
        mutationOwner.current = null;
        setBusy(false);
      }
    }
  }
  return (
    <main className="admin-scroll community-skills">
      <header className="admin-intro">
        <button onClick={back}>채팅으로</button>
        <h2>제안 · 할 수 있는 일</h2>
        <p>공개 저장소에서 함께 만든 절차입니다. 추가한 절차는 새 대화 요청에서 사용할 수 있습니다.</p>
        <p>추가만으로 메시지를 보내거나 예약을 만들지 않습니다. 기존 연결과 도구 범위에서 작업합니다.</p>
      </header>
      {error && <p role="alert">{error}</p>}
      {!cards && !error && <p role="status">불러오는 중…</p>}
      <button disabled={busy} onClick={()=>{setImportSource(undefined);setImportOpen(!importOpen)}}>GitHub 원본에서 추가</button>
      <button disabled={busy} onClick={refresh}>목록 새로고침</button>
      {importOpen && <GitHubSkillImport accountId={accountId} onExpired={onExpired} connect={connect} source={importSource} mutationBusy={busy} install={card=>action(card,card.installed_digest ? "update" : "install")} />}
      <label className="community-hidden">
        <input type="checkbox" checked={showHidden} onChange={e => setShowHidden(e.target.checked)} />
        숨긴 제안 표시
      </label>
      {cards?.filter(c => showHidden || !c.hidden).map(card => (
        <article key={card.id} className="admin-card">
          <h3>{card.name}</h3>
          <p>{card.description}</p>
          <p>{card.readiness === "connection_needed" ? "필요한 연결을 확인해 주세요"
            : card.readiness === "ready" ? "사용 가능"
            : card.readiness === "disabled" ? "비활성화됨" : "추가 가능"}</p>
          {card.missing.some(c => c.startsWith("mail_")) && <button onClick={connect}>Gmail 연결하기</button>}
          {card.missing.includes("web_read") && <p>서버의 공개 웹 읽기 지원이 필요합니다.</p>}
          <div className="community-actions">
            {!card.installed_digest ? (
              <button disabled={busy} onClick={() => void action(card, "install")}>추가</button>
            ) : <>
              {card.enabled && card.readiness === "ready" && <button onClick={() => start(card.github ? `${card.name} (${card.github.repository})` : card.name)}>채팅에서 시작</button>}
              <button disabled={busy} onClick={() => void action(card, card.enabled ? "disable" : "enable")}>
                {card.enabled ? "비활성화" : "활성화"}
              </button>
              {card.github && <button disabled={busy} onClick={()=>{setImportSource(card.github);setImportOpen(true)}}>다른 버전 확인</button>}
              {card.digest !== card.installed_digest && <button disabled={busy} onClick={() => void action(card, "update")}>업데이트</button>}
              {card.can_rollback && <button disabled={busy} onClick={() => void action(card, "rollback")}>이전 버전으로</button>}
              <button disabled={busy} onClick={() => void action(card, "remove")}>삭제</button>
            </>}
            <button disabled={busy} onClick={() => void action(card, card.hidden ? "show" : "hide")}>
              {card.hidden ? "다시 표시" : "제안 숨기기"}
            </button>
          </div>
          <details>
            <summary>출처 · 버전 · 필요한 기능</summary>
            <p>제작자: {card.github ? card.github.repository : "moosuri contributors"} · {card.github ? "사용자가 선택한 외부 원본" : "curated 목록"} · 호환 형식 확인됨 · 실제 사용 성공을 보증하지 않습니다.</p>
            <a href={card.source} target="_blank" rel="noreferrer">공개 원본</a>
            <p>제공 버전 {card.version} · 설치 버전 {card.installed_version ?? "없음"} · {card.license}</p>
            <p>필요한 기능: {card.capabilities.join(", ")}</p>
            <p>비활성화·삭제하면 새 요청에서 이 절차를 제외합니다. 기본 도구 자체는 계속 사용할 수 있습니다.</p>
            <p>삭제하면 이 절차를 포함한 진행 중 요청과 재시도를 중단합니다. 완료한 외부 작업 기록은 보존합니다.</p>
          </details>
        </article>
      ))}
      <p>현재 목록은 앱에 포함한 검증된 공개 패키지입니다. 공개 GitHub 저장소의 네 파일 절차를 직접 추가할 수 있습니다. private 저장소, 개인 학습 공유, 자동 업데이트는 아직 지원하지 않습니다.</p>
    </main>
  );
}
