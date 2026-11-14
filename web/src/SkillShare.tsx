import { useEffect, useState } from "react";
import type { CommunitySkillCard, SkillSharePreview } from "../shared/api-types";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";

export function SkillShare({ accountId, card, close, onExpired }: {
  accountId: string; card: CommunitySkillCard; close: () => void; onExpired: () => void;
}) {
  const scope = useScope(accountId, `${card.id}:${card.revision}`);
  const [preview, setPreview] = useState<SkillSharePreview | null>(null);
  const [error, setError] = useState("");
  useEffect(() => {
    const controller = new AbortController();
    setPreview(null); setError("");
    void scope.request<SkillSharePreview>(
      `/community-skills/${encodeURIComponent(card.id)}/share-preview`, "POST",
      { expected_revision: card.revision }, controller.signal,
    ).then(setPreview).catch(e => {
      if (isAborted(e)) return;
      if (e instanceof ApiFailure && e.status === 401) onExpired();
      else setError("공유할 절차를 준비하지 못했습니다. 목록을 새로고침해 주세요.");
    });
    return () => controller.abort();
  }, [scope, card.id, card.revision]);
  function download() {
    if (!preview) return;
    try {
      const bytes = Uint8Array.from(atob(preview.archive_base64), c => c.charCodeAt(0));
      const url = scope.localUrl(new Blob([bytes], { type: "application/x-tar" }));
      const link = document.createElement("a");
      link.href = url; link.download = `${preview.slug}.tar`; link.click();
      setTimeout(() => scope.releaseUrl(url), 3000);
    } catch { setError("파일을 내려받지 못했습니다. 다시 시도해 주세요."); }
  }
  return <section className="skill-share" aria-label="공유할 절차 미리보기">
    <h4>이 절차 공유하기</h4>
    <p>절차와 새 예제만 담습니다. 내 대화·자료·계정 정보·사용 이력은 포함하지 않습니다.</p>
    {error && <p role="alert">{error}</p>}
    {!preview && !error && <p role="status">공유 파일을 준비하고 있습니다…</p>}
    {preview && <>
      {Object.entries(preview.files).map(([name, content]) => <details key={name}>
        <summary>{name}</summary><pre>{content}</pre>
      </details>)}
      <button onClick={download}>공유 파일 내려받기</button>
      <p>압축 파일을 풀어 Git 저장소에 올리면 다른 사람이 기존 GitHub 추가 화면에서 설치할 수 있습니다.
        설치한 사람의 연결과 도구를 사용하며 개인 학습 이력은 전달하지 않습니다.</p>
    </>}
    <button onClick={close}>미리보기 닫기</button>
  </section>;
}
