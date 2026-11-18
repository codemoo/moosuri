import { useEffect, useRef, useState, type FormEvent } from "react";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";

type Login = { attempt_id: string; status: string; authorization_url: string | null; expires_in_seconds: number };
type Props = { accountId: string; bindingId: string; revision: number; disabled: boolean; onExpired: () => void };
const active = (status?: string) => ["starting", "awaiting_code", "verifying"].includes(status ?? "");
function officialUrl(value: string | null) {
  if (!value) return undefined;
  try {
    const u = new URL(value);
    return u.origin === "https://claude.com" && u.pathname === "/cai/oauth/authorize"
      && !u.username && !u.password && !u.hash ? value : undefined;
  } catch { return undefined; }
}

export function ClaudeSubscriptionLogin({ accountId, bindingId, revision, disabled, onExpired }: Props) {
  const scope = useScope(accountId);
  const [login, setLogin] = useState<Login | null>(null);
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const pending = useRef(false);
  const generation = useRef(0);
  const base = `/settings/claude/${encodeURIComponent(bindingId)}/login`;
  const path = login ? `${base}/${encodeURIComponent(login.attempt_id)}` : base;
  const url = officialUrl(login?.authorization_url ?? null);

  function fail(e: unknown) {
    if (isAborted(e)) return;
    if (e instanceof ApiFailure && e.status === 401) onExpired();
    else setError("로그인 상태를 확인하지 못했습니다. 새로 시작하면 진행 중인 연결을 먼저 확인합니다.");
  }
  useEffect(() => {
    if (!login || !active(login.status)) return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      const requested = generation.current;
      try {
        const next = await scope.request<Login>(path);
        if (stopped || requested !== generation.current) return;
        setLogin(next); setError("");
        if (active(next.status) && !stopped) timer = setTimeout(() => void poll(), 1000);
      } catch (e) {
        if (!stopped && requested === generation.current) { fail(e); setLogin(current => current && { ...current, status: "unavailable", authorization_url: null }); }
      }
    }
    timer = setTimeout(() => void poll(), 500);
    return () => { stopped = true; clearTimeout(timer); };
  }, [scope, path, login?.attempt_id, login?.status]);

  async function action(kind: "start" | "code" | "cancel") {
    if (pending.current || disabled) return;
    pending.current = true; generation.current += 1; setBusy(true); setError("");
    const submitted = code.trim();
    setCode("");
    try {
      const next = kind === "start"
        ? await scope.request<Login>(base, "POST", { revision })
        : kind === "cancel" ? await scope.request<Login>(path, "DELETE")
          : await scope.request<Login>(path, "POST", { code: submitted });
      setLogin(next);
    } catch (e) {
      fail(e);
      if (!isAborted(e)) setLogin(current => current && { ...current, status: "unavailable", authorization_url: null });
    }
    finally { pending.current = false; setBusy(false); }
  }
  function submit(event: FormEvent) { event.preventDefault(); void action("code"); }
  return <div className="claude-subscription-login" role="group" aria-label="Claude 구독 로그인">
    {!active(login?.status) && <button className="primary" disabled={disabled || busy} onClick={() => void action("start")}>
      {login ? "구독 로그인 다시 시작" : "구독 로그인"}
    </button>}
    {login?.status === "starting" && <p role="status">Claude 공식 로그인 링크를 준비하고 있습니다…</p>}
    {login?.status === "awaiting_code" && <>
      <p>1. Claude에서 본인 구독 계정으로 로그인하세요.</p>
      {url && <a className="secondary" href={url} target="_blank" rel="noopener noreferrer" referrerPolicy="no-referrer">Claude 공식 로그인 열기</a>}
      <form onSubmit={submit}>
        <label>2. Claude에서 받은 로그인 코드<input type="password" autoComplete="off" autoCapitalize="none" spellCheck={false}
          value={code} onChange={e => setCode(e.target.value)} maxLength={4096} disabled={disabled || busy}
          placeholder="로그인 후 표시된 코드를 여기에 붙여넣으세요" /></label>
        <p className="settings-hint">5분 안에 입력하세요. 코드는 이 로그인 연결에 한 번만 전달됩니다.</p>
        <button className="primary" disabled={disabled || busy || code.trim().length < 8}>코드로 연결 완료</button>
      </form>
    </>}
    {login?.status === "verifying" && <p role="status">Claude가 로그인 코드를 확인하고 있습니다…</p>}
    {login?.status === "succeeded" && <p className="settings-success" role="status">구독 로그인을 완료했습니다. 아래에서 사용할 모델의 텍스트 연결을 확인하세요.</p>}
    {login?.status === "expired" && <p role="status">로그인 시간이 만료되었습니다. 새로 시작해 주세요.</p>}
    {login?.status === "cancelled" && <p role="status">로그인을 취소했습니다.</p>}
    {login?.status === "failed" && <p role="alert">Claude 로그인을 완료하지 못했습니다. 새 로그인 링크에서 다시 진행해 주세요.</p>}
    {error && <p role="alert">{error}</p>}
    {active(login?.status) && <button className="text-button" disabled={disabled || busy} onClick={() => void action("cancel")}>로그인 취소</button>}
  </div>;
}
