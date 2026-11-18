import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";
import "./provider-settings.css";

export type TelegramConfiguration = {
  bot: { revision: number; username: string | null; enabled: boolean; status: string; last_poll_at: number | null };
  binding?: { id: string; telegram_user_id: number; thread_id: string | null; notifications: boolean; timezone: string; last_delivery: string | null } | null;
};
type Pair = { url: string; code: string; expires_at: number };
type Props = { accountId: string; onExpired: () => void; admin?: boolean };
const states: Record<string, string> = {
  disabled: "봇이 꺼져 있습니다.", connected: "봇이 연결되었습니다.", polling: "새 메시지를 받고 있습니다.",
  error: "Telegram 연결에 문제가 있습니다. 상태를 다시 확인하세요.", conflict: "다른 수신기 또는 웹훅과 충돌했습니다. 관리자에게 확인하세요.",
  unauthorized: "봇 인증이 만료되거나 거부되었습니다. 관리자가 토큰을 다시 연결해야 합니다.", rate_limited: "Telegram 요청 제한으로 잠시 기다리고 있습니다.",
};
const deliveries: Record<string, string> = {
  accepted: "최근 메시지가 Telegram에 접수되었습니다. 읽음 여부는 확인하지 않습니다.",
  unknown: "최근 전달 여부를 확인할 수 없습니다. 같은 메시지를 자동으로 다시 보내지 않습니다.",
  rejected: "최근 메시지를 전달하지 못했습니다.", queued: "전달을 기다리는 메시지가 있습니다.",
};
function timestamp(value: number) { return new Date(value * 1000).toLocaleString(); }
function safePairUrl(value: string) {
  try { const url = new URL(value); return url.protocol === "https:" && url.hostname === "t.me" && !url.username && !url.password && !url.port ? url.href : null; }
  catch { return null; }
}

export function TelegramSettings(props: Props) {
  return <Contents key={`${props.accountId}:${!!props.admin}`} {...props} />;
}
function Contents({ accountId, admin = false, onExpired }: Props) {
  const scope = useScope(accountId, admin ? "telegram-admin" : "telegram-settings");
  const path = admin ? "/admin/telegram" : "/settings/telegram";
  const [configuration, setConfiguration] = useState<TelegramConfiguration | null>(null);
  const [pair, setPair] = useState<Pair | null>(null);
  const [notifications, setNotifications] = useState(false);
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const live = useRef(false), locked = useRef(false), generation = useRef(0);
  const tokenInput = useRef<HTMLInputElement>(null);
  useLayoutEffect(() => {
    live.current = true;
    return () => { live.current = false; generation.current++; if (tokenInput.current) tokenInput.current.value = ""; };
  }, []);
  function current(version: number) { return live.current && version === generation.current; }
  function fail(e: unknown, version: number) {
    if (!current(version) || isAborted(e)) return;
    if (e instanceof ApiFailure && e.status === 401) onExpired();
    else setError(e instanceof ApiFailure && e.status === 409
      ? "설정이 변경되었거나 다른 연결과 충돌했습니다. 상태를 새로고침하고 다시 시도하세요."
      : "Telegram 설정을 확인하지 못했습니다. 상태를 새로고침한 뒤 다시 시도하세요.");
  }
  async function load(version: number, signal?: AbortSignal) {
    const value = await scope.request<TelegramConfiguration>(path, "GET", undefined, signal);
    if (!current(version)) return;
    setConfiguration(value);
    if (value.binding) { setPair(null); setNotifications(value.binding.notifications); }
  }
  useEffect(() => {
    const controller = new AbortController(), version = ++generation.current;
    void load(version, controller.signal).catch(e => fail(e, version));
    return () => controller.abort();
  }, [scope, path]);
  useEffect(() => {
    if (!pair || admin) return;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout>;
    async function poll() {
      if (controller.signal.aborted || !pair) return;
      if (Date.now() >= pair.expires_at * 1000) {
        setPair(null); setNotice("연결 코드가 만료되었습니다. 새 연결 링크를 만드세요."); return;
      }
      const version = generation.current;
      if (!locked.current) {
        try { await load(version, controller.signal); }
        catch (e) { fail(e, version); }
      }
      if (!controller.signal.aborted) timer = setTimeout(() => void poll(), 3000);
    }
    timer = setTimeout(() => void poll(), 3000);
    return () => { controller.abort(); clearTimeout(timer); };
  }, [pair, admin, scope]);
  async function mutate(method: string, body?: unknown, pairing = false) {
    if (locked.current || !configuration) return;
    locked.current = true; setBusy(true); setError(""); setNotice("");
    const version = ++generation.current;
    // Credentials leave the input as soon as the explicit save starts, including on failure.
    setToken("");
    try {
      const result = await scope.request<Pair | undefined>(pairing ? "/settings/telegram/pair" : path, method, body);
      if (!current(version)) return;
      if (pairing) {
        if (!result || !safePairUrl(result.url)) throw new Error("Invalid pairing link");
        setPair(result);
      } else {
        if (method === "DELETE") setPair(null);
        await load(version);
        if (current(version)) setNotice(method === "DELETE" ? (admin ? "봇을 끄고 모든 연결을 해제했습니다." : "내 Telegram 연결과 대기 중인 코드를 해제했습니다.") : "Telegram 설정을 저장했습니다.");
      }
    } catch (e) { fail(e, version); }
    finally { if (current(version)) { locked.current = false; setBusy(false); } }
  }
  async function refresh() {
    if (locked.current) return;
    locked.current = true; setBusy(true); setError("");
    const version = ++generation.current;
    try { await load(version); } catch (e) { fail(e, version); }
    finally { if (current(version)) { locked.current = false; setBusy(false); } }
  }
  async function copy(value: string) {
    const version = generation.current;
    try { await navigator.clipboard.writeText(value); if (current(version)) setNotice("복사했습니다."); }
    catch { if (current(version)) setError("복사하지 못했습니다. 아래 값을 직접 선택해 복사하세요."); }
  }
  const binding = configuration?.binding;
  return <main className="admin-scroll model-settings" aria-labelledby="telegram-title">
    <div className="section-heading"><h2 id="telegram-title">{admin ? "설치의 Telegram 봇" : "내 Telegram"}</h2></div>
    <p className="fine">대화는 Telegram 서버를 거쳐 무수리 웹과 동기화됩니다.</p>
    {error && <p className="notice" role="alert">{error}</p>}
    {notice && <p role="status">{notice}</p>}
    {!configuration ? <p role="status">Telegram 설정을 불러오고 있습니다…</p> : <>
      <p role="status">{states[configuration.bot.status] ?? "봇 상태를 확인해야 합니다."}{configuration.bot.username && ` @${configuration.bot.username}`}</p>
      {configuration.bot.last_poll_at !== null && <p className="fine">마지막 수신 확인: {timestamp(configuration.bot.last_poll_at)}</p>}
      {admin ? <div className="admin-card">
        <p><a href="https://t.me/BotFather" target="_blank" rel="noopener noreferrer">BotFather에서 봇 만들기</a> 후 토큰을 붙여 넣으세요. 기존 웹훅이 있으면 연결할 수 없습니다.</p>
        <form onSubmit={event => { event.preventDefault(); if (token.trim()) void mutate("PUT", { expected_revision: configuration.bot.revision, token: token.trim() }); }}>
          <label>BotFather 토큰 <input ref={tokenInput} type="password" autoComplete="off" spellCheck={false} value={token} onChange={event => setToken(event.target.value)} disabled={busy} /></label>
          <div className="settings-form-actions"><button className="primary" disabled={busy || !token.trim()}>봇 확인하고 저장</button></div>
        </form>
        <p className="fine">토큰은 조회 화면에 표시되지 않습니다. 봇을 바꾸거나 끄면 기존 개인 연결을 다시 설정해야 합니다.</p>
        <button type="button" disabled={busy || !configuration.bot.enabled} onClick={() => void mutate("DELETE", { expected_revision: configuration.bot.revision })}>봇 끄고 모든 연결 해제</button>
      </div> : <div className="admin-card">
        {binding ? <>
          <h3>내 계정 연결됨</h3><p>Telegram 사용자: {binding.telegram_user_id}</p>
          {binding.last_delivery && <p>{deliveries[binding.last_delivery] ?? "최근 전달 상태를 확인해야 합니다."}</p>}
          <label className="check-label"><input type="checkbox" checked={binding.notifications} disabled={busy} onChange={event => void mutate("PUT", { binding_id: binding.id, notifications: event.target.checked })} />Telegram으로 새 소식 알림 받기</label>
        </> : <>
          <h3>내 계정 연결</h3>
          <label className="check-label"><input type="checkbox" checked={notifications} disabled={busy || !!pair} onChange={event => setNotifications(event.target.checked)} />Telegram으로 새 소식 알림 받기</label>
          {!pair && <button type="button" className="primary" disabled={busy || !configuration.bot.enabled} onClick={() => void mutate("POST", { timezone: Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC", notifications }, true)}>연결 링크 만들기</button>}
          {!configuration.bot.enabled && <p>관리자가 먼저 설치의 Telegram 봇을 연결해야 합니다.</p>}
        </>}
        <p className="fine">알림은 개인 내용 없이 웹으로 가는 링크만 보냅니다. 방해 금지 시간, 음소거하거나 이미 읽은 소식은 알리지 않습니다.</p>
        {pair && <div>
          <p>10분 동안 한 번만 사용할 수 있는 내 계정 연결 링크입니다. 다른 사람에게 공유하지 마세요. Telegram에서 링크를 열고 시작을 누르세요.</p>
          <p>만료: {timestamp(pair.expires_at)}</p>
          <p><a href={safePairUrl(pair.url)!} target="_blank" rel="noopener noreferrer">Telegram에서 연결하기</a></p>
          <label>연결 링크 <input readOnly value={pair.url} /></label><button type="button" onClick={() => void copy(pair.url)}>연결 링크 복사</button>
          <label>연결 코드 <input readOnly value={pair.code} /></label><button type="button" onClick={() => void copy(pair.code)}>연결 코드 복사</button>
        </div>}
        <div className="settings-form-actions"><button type="button" disabled={busy} onClick={() => void mutate("DELETE")}>내 연결·대기 코드 해제</button></div>
        <p>명령: /new 새 대화 · /stop 실행 취소 · /status 상태 · /help 도움말 · /disconnect 연결 해제</p>
        <p>개인 텍스트 대화를 지원합니다. 이미지·음성·파일·그룹 대화는 아직 지원하지 않습니다. 메일 발송은 웹에서 전체 최종 미리보기를 확인하고 승인해야 합니다.</p>
      </div>}
    </>}
    <div className="settings-form-actions"><button type="button" disabled={busy} onClick={() => void refresh()}>상태 새로고침</button></div>
    {busy && <p role="status">설정을 확인하고 있습니다…</p>}
  </main>;
}
