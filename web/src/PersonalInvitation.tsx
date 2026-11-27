import { useEffect, useRef, useState, type FormEvent } from "react";
import { accountPasswordValid } from "./account-password";
import "./personal-preview.css";

type Invitation = { name: string; email: string; ready: boolean; password_set: boolean };
export function takeInvitationToken(location: Pick<Location, "hash">, history: Pick<History, "replaceState">): string {
  const token = location.hash.slice(1);
  history.replaceState(null, "", "/join");
  return /^[A-Za-z0-9_-]{43}$/.test(token) ? token : "";
}
class InvitationError extends Error { constructor(readonly status: number) { super("invitation_failed"); } }
async function request(token: string, password?: string, signal?: AbortSignal): Promise<Invitation> {
  const response = await fetch(`/api/v1/personal/invitations/${password === undefined ? "status" : "redeem"}`, {
    method: "POST", headers: { "Content-Type": "application/json" }, credentials: "omit",
    cache: "no-store", referrerPolicy: "no-referrer", signal,
    body: JSON.stringify({ token, ...(password === undefined ? {} : { password }) }),
  });
  if (!response.ok) throw new InvitationError(response.status);
  return response.json();
}
function errorText(error: unknown) {
  const status = error instanceof InvitationError ? error.status : 0;
  if (status === 401 || status === 403 || status === 404) return "이 초대 링크를 사용할 수 없습니다. 관리자에게 새 링크를 받아 주세요.";
  if (status === 409) return "계정 공간을 준비하고 있습니다. 잠시 뒤 다시 확인해 주세요. 계속 준비 중이면 관리자에게 알려 주세요.";
  if (status === 429) return "요청이 많습니다. 5분 뒤 다시 확인해 주세요.";
  return "결과를 확인하지 못했습니다. ‘상태 다시 확인’을 누르세요. 비밀번호가 이미 저장됐다면 처음 설정한 비밀번호로 로그인할 수 있습니다.";
}
export default function PersonalInvitation({ token }: { token: string }) {
  const [invitation, setInvitation] = useState<Invitation | null>(null);
  const [password, setPassword] = useState("");
  const [confirmation, setConfirmation] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [setHere, setSetHere] = useState(false);
  const lock = useRef(false);
  const active = useRef(true);
  useEffect(() => {
    active.current = true;
    const abort = new AbortController();
    if (token) {
      setBusy(true);
      void request(token, undefined, abort.signal).then(setInvitation).catch(e => {
        if (!abort.signal.aborted) setError(errorText(e));
      }).finally(() => { if (!abort.signal.aborted) setBusy(false); });
    }
    return () => { active.current = false; abort.abort(); };
  }, [token]);
  async function run(submit: boolean) {
    if (lock.current || busy || !token) return;
    lock.current = true; setBusy(true); setError("");
    const chosen = submit ? password : undefined;
    setPassword(""); setConfirmation("");
    try {
      const result = await request(token, chosen);
      if (active.current) { setInvitation(prior => ({ ...prior, ...result })); setSetHere(submit && result.password_set); }
    } catch (e) { if (active.current) setError(errorText(e)); }
    finally { lock.current = false; if (active.current) setBusy(false); }
  }
  function submit(event: FormEvent) {
    event.preventDefault();
    if (accountPasswordValid(password) && password === confirmation) void run(true);
  }
  return <main className="preview-gate">
    <h1>{invitation?.ready ? "무수리에 오신 것을 환영합니다" : "내 무수리 시작하기"}</h1>
    {!token ? <p role="alert">전달받은 초대 링크를 다시 열어 주세요. 새로고침하면 비밀번호와 링크 정보는 남지 않습니다.</p> : <>
      {invitation && <p>{invitation.name} · {invitation.email}</p>}
      {invitation?.ready ? <><p role="status">{setHere ? "비밀번호를 설정했습니다." : "가입이 완료됐습니다. 이미 설정한 비밀번호로 로그인하세요."}</p><a href="/app">로그인하기</a></> : <>
        <p>회원님이 사용할 비밀번호를 정해 주세요. 개인 볼트 비밀번호는 가입 후 따로 설정합니다.</p>
        {invitation && <form onSubmit={submit}>
          <label>새 비밀번호<input type="password" autoComplete="new-password" required disabled={busy} value={password} onChange={e => setPassword(e.target.value)} /></label>
          <label>비밀번호 확인<input type="password" autoComplete="new-password" required disabled={busy} value={confirmation} onChange={e => setConfirmation(e.target.value)} /></label>
          <p>8자 이상, 최대 256바이트로 입력해 주세요.</p>
          <button disabled={busy || !accountPasswordValid(password) || password !== confirmation}>{busy ? "준비 중…" : "가입 완료"}</button>
        </form>}
        <button type="button" disabled={busy} onClick={() => void run(false)}>상태 다시 확인</button>
      </>}
      {error && <p role="alert" className="preview-error">{error}</p>}
    </>}
  </main>;
}
