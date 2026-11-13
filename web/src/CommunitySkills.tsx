import { useEffect, useState } from "react";
import type { CommunitySkillCard } from "../shared/api-types";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";
import "./community-skills.css";

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
  function fail(e: unknown) {
    if (isAborted(e)) return;
    if (e instanceof ApiFailure && e.status === 401) onExpired();
    else setError("상태를 확인하지 못했습니다. 새로고침한 뒤 다시 시도하세요.");
  }
  useEffect(() => {
    const controller = new AbortController();
    scope.request<CommunitySkillCard[]>(
      "/community-skills", "GET", undefined, controller.signal,
    ).then(setCards).catch(fail);
    return () => controller.abort();
  }, [scope]);
  async function action(card: CommunitySkillCard, action: string) {
    setBusy(true);
    setError("");
    try {
      setCards(await scope.request<CommunitySkillCard[]>(
        `/community-skills/${encodeURIComponent(card.id)}`, "POST",
        { action, expected_revision: card.revision, expected_digest: card.digest },
      ));
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
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
              {card.enabled && card.readiness === "ready" && <button onClick={() => start(card.name)}>채팅에서 시작</button>}
              <button disabled={busy} onClick={() => void action(card, card.enabled ? "disable" : "enable")}>
                {card.enabled ? "비활성화" : "활성화"}
              </button>
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
            <p>제작자: moosuri contributors · 호환 형식 확인됨 · 실제 사용 성공을 보증하지 않습니다.</p>
            <a href={card.source} target="_blank" rel="noreferrer">공개 원본</a>
            <p>제공 버전 {card.version} · 설치 버전 {card.installed_version ?? "없음"} · {card.license}</p>
            <p>필요한 기능: {card.capabilities.join(", ")}</p>
            <p>비활성화·삭제하면 새 요청에서 이 절차를 제외합니다. 기본 도구 자체는 계속 사용할 수 있습니다.</p>
            <p>삭제하면 이 절차를 포함한 진행 중 요청과 재시도를 중단합니다. 완료한 외부 작업 기록은 보존합니다.</p>
          </details>
        </article>
      ))}
      <p>현재 목록은 앱에 포함한 검증된 공개 패키지입니다. 외부 Git 주소 추가, 개인 학습 공유, 자동 업데이트는 아직 지원하지 않습니다.</p>
    </main>
  );
}
