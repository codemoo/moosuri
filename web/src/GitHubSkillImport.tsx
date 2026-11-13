import { useEffect, useRef, useState, type FormEvent } from "react";
import type { CommunitySkillCard, GitHubSkillLocation, GitHubSkillPreviewInput } from "../shared/api-types";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";

type Props = {
  accountId: string;
  onExpired: () => void;
  install: (card: CommunitySkillCard) => Promise<boolean>;
  mutationBusy: boolean;
  connect: () => void;
  source?: GitHubSkillLocation;
};
export function GitHubSkillImport(props: Props) {
  return <Import key={`${props.accountId}:${props.source?.repository ?? ""}:${props.source?.directory ?? ""}`} {...props} />;
}
function Import({ accountId, onExpired, install: activate, mutationBusy, connect, source }: Props) {
  const scope = useScope(accountId);
  const [repository, setRepository] = useState(source?.repository ?? "");
  const [directory, setDirectory] = useState(source?.directory ?? "");
  const [reference, setReference] = useState("main");
  const [preview, setPreview] = useState<CommunitySkillCard | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const pending = useRef<AbortController | null>(null);
  const generation = useRef(0);
  useEffect(() => () => { generation.current++; pending.current?.abort(); }, []);
  function edit(change: () => void) {
    generation.current++;
    pending.current?.abort();
    pending.current = null;
    setBusy(false);
    setPreview(null);
    setError("");
    change();
  }
  function fail(e: unknown) {
    if (isAborted(e)) return;
    if (e instanceof ApiFailure && e.status === 401) onExpired();
    else if (e instanceof ApiFailure && e.status === 409) setError("설치 상태가 바뀌었습니다. 원본을 다시 확인해 주세요.");
    else if (e instanceof ApiFailure && e.status === 404) setError("공개 저장소·버전·패키지 경로를 찾을 수 없습니다.");
    else if (e instanceof ApiFailure && e.status === 400) setError("지원하는 네 파일과 형식·크기·라이선스를 확인해 주세요. 공개 GitHub 저장소만 지원합니다.");
    else setError("원본을 확인하지 못했습니다. GitHub 요청 제한이나 연결 상태를 확인한 뒤 다시 시도하세요.");
  }
  async function inspect(event: FormEvent) {
    event.preventDefault();
    const owner = ++generation.current;
    pending.current?.abort();
    const controller = new AbortController();
    pending.current = controller;
    setBusy(true); setPreview(null); setError("");
    try {
      const input: GitHubSkillPreviewInput = { repository, directory, reference };
      const card = await scope.request<CommunitySkillCard>("/community-skills/github/preview", "POST", input, controller.signal);
      if (generation.current === owner) setPreview(card);
    } catch (e) { if (generation.current === owner) fail(e); }
    finally { if (generation.current === owner) { setBusy(false); pending.current = null; } }
  }
  async function install() {
    if (!preview || mutationBusy || busy) return;
    const owner = ++generation.current;
    setBusy(true); setError("");
    try {
      const accepted = await activate(preview);
      if (generation.current === owner && accepted) setPreview(null);
    } catch (e) { if (generation.current === owner) fail(e); }
    finally { if (generation.current === owner) setBusy(false); }
  }
  return <section className="admin-card" aria-label="공개 GitHub 절차 추가">
    <h3>공개 GitHub 절차 추가</h3>
    <p>목록 밖의 공개 원본을 직접 선택합니다. 원본 확인은 설치나 작업 실행을 시작하지 않습니다.</p>
    <form onSubmit={event => void inspect(event)}>
      <label>저장소 URL
        <input required type="url" placeholder="https://github.com/author/repository" value={repository} onChange={e => edit(() => setRepository(e.target.value))} disabled={mutationBusy || (busy && !!preview)} />
      </label>
      <label>패키지 디렉터리
        <input required placeholder="community/skills/example" value={directory} onChange={e => edit(() => setDirectory(e.target.value))} disabled={mutationBusy || (busy && !!preview)} />
      </label>
      <label>브랜치 · 태그 · 전체 commit
        <input required value={reference} onChange={e => edit(() => setReference(e.target.value))} disabled={mutationBusy || (busy && !!preview)} />
      </label>
      <button disabled={busy || mutationBusy}>원본 확인</button>
    </form>
    {busy && <p role="status">공개 원본 확인 또는 설치 중…</p>}
    {error && <p role="alert">{error}</p>}
    {preview && <div aria-label="GitHub 절차 미리보기">
      <h4>{preview.name} · {preview.version}</h4>
      <p>{preview.description}</p>
      <a href={preview.source} target="_blank" rel="noreferrer">고정한 원본 보기</a>
      <p>제작 저장소: {preview.github?.repository} · {preview.license}</p>
      <p>앱의 curated 목록과 별개입니다. 형식과 호환성을 확인했으며 실제 사용 성공을 보증하지 않습니다.</p>
      {preview.missing.some(name => name.startsWith("mail_")) && <button type="button" onClick={connect}>Gmail 연결하기</button>}
      {preview.missing.includes("web_read") && <p>공개 웹 읽기는 서버 설정이 필요합니다.</p>}
      <details><summary>필요한 기능 · 정확한 원본</summary>
        <p>{preview.capabilities.join(", ")}</p>
        <p>commit {preview.source_commit}</p><p>SHA-256 {preview.digest}</p>
      </details>
      <button disabled={busy || mutationBusy} onClick={() => void install()}>{preview.installed_digest ? "이 버전으로 업데이트" : "추가"}</button>
      <p>추가해도 메시지 발송·예약·외부 작업을 실행하지 않습니다. 기존 도구·개인 연결 범위에서 새 요청에 사용할 수 있습니다.</p>
    </div>}
  </section>;
}
