import { useCallback, useEffect, useRef, useState } from "react";
import { ApiFailure } from "./api";

/** Reusable Google Calendar OAuth surface for the member Calendar vertical.
 * Own per-member consent (scope: calendar.events.owned; never a Drive/read
 * capability). The member callback returns `#google_service=calendar&google_handle=…&flow=…`;
 * the handle is only accepted for the flow this tab started, then completed
 * server-side. On success `onConnected` resumes the Calendar preview (MemberCalendarWork). */
const base = "/personal/google/calendar";
const storageKey = "moosuri.personal.google-calendar.v1";
const flowPattern = /^[a-f0-9]{64}$/;

export type CalendarConnection = { connection_id: string; epoch: number; mailbox: string };
type Scope = {
  request<T>(path: string, method?: string, body?: unknown, signal?: AbortSignal, options?: { timeoutMs?: number }): Promise<T>;
};
type Pending = { account: string; session: string; flow: string; expires: number; phase: "consent" | "completing"; thread?: string };

function readPending(account: string, session: string): Pending | null {
  try {
    const p: Pending | null = JSON.parse(sessionStorage.getItem(storageKey) ?? "null");
    if (p && p.account === account && p.session === session && p.expires > Date.now() && flowPattern.test(p.flow)) return p;
  } catch {
    /* Unavailable or malformed storage cannot authorize completion. */
  }
  return null;
}
export { clearPending as clearCalendarConsent };
function clearPending() {
  try {
    sessionStorage.removeItem(storageKey);
  } catch {
    /* Storage unavailable. */
  }
}
/** True when the URL fragment is a Calendar consent return for this app. */
export function isCalendarConsentReturn(hash = location.hash): boolean {
  const f = new URLSearchParams(hash.replace(/^#/, ""));
  return f.getAll("google_service").length === 1 && f.get("google_service") === "calendar" && (f.has("google_handle") || f.has("google_denied"));
}
/** The conversation that started this tab's consent, so the callback returns to it. Pure: it changes nothing. */
export function calendarReturnThread(account: string, session: string | null | undefined): string | null {
  if (!session || !isCalendarConsentReturn()) return null;
  const pending = readPending(account, session);
  return pending?.thread && /^[a-zA-Z0-9_-]{1,256}$/.test(pending.thread) ? pending.thread : null;
}
function stripFragment() {
  history.replaceState(null, "", location.pathname + location.search);
}

export function MemberCalendarConsent(props: {
  scope: Scope;
  /** Authenticated member id and draft/session scope, binding consent to this tab. */
  accountId: string;
  sessionScope: string | null | undefined;
  /** The conversation the request belongs to; consent returns to it. */
  threadId: string;
  /** Current Calendar connection (from `/personal/google/calendar/status`), if any. */
  connection: CalendarConnection | null;
  /** Called with the verified fresh connection; the caller resumes its preview. */
  onConnected: (connection: CalendarConnection) => void;
  onDisconnected?: () => void;
  onExpired?: () => void;
}) {
  const { scope, accountId, sessionScope } = props;
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState("");
  const lock = useRef(false);
  const handled = useRef(false);

  const authFailed = useCallback(
    (error: unknown) => {
      if (error instanceof ApiFailure && error.status === 401) {
        props.onExpired?.();
        return true;
      }
      return false;
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [props.onExpired],
  );

  async function readStatus(): Promise<CalendarConnection | null> {
    const s = await scope.request<{ connection: CalendarConnection | null }>(`${base}/status`);
    return s.connection;
  }

  // Resume: accept the callback exactly once, only for the flow this tab started.
  useEffect(() => {
    if (handled.current || !isCalendarConsentReturn() || !sessionScope) return;
    handled.current = true;
    const fields = new URLSearchParams(location.hash.replace(/^#/, ""));
    const pending = readPending(accountId, sessionScope);
    const handles = fields.getAll("google_handle");
    const denials = fields.getAll("google_denied");
    const flows = fields.getAll("flow");
    stripFragment();
    if (!pending || pending.phase !== "consent" || flows.length !== 1 || flows[0] !== pending.flow) {
      clearPending();
      setMessage("연결을 시작한 계정과 탭에서 다시 진행해 주세요.");
      return;
    }
    if (denials.length === 1 && handles.length === 0) {
      clearPending();
      setMessage("Google 캘린더 연결이 취소됐습니다.");
      return;
    }
    if (handles.length !== 1 || denials.length !== 0 || !flowPattern.test(handles[0])) {
      clearPending();
      setMessage("Google 연결 정보를 확인할 수 없습니다.");
      return;
    }
    lock.current = true;
    setBusy(true);
    void (async () => {
      try {
        sessionStorage.setItem(storageKey, JSON.stringify({ ...pending, phase: "completing" }));
        const grant = await scope.request<CalendarConnection & { flow_id: string }>(
          `${base}/complete`, "POST", { handle: handles[0] }, undefined, { timeoutMs: 75000 });
        if (grant.flow_id !== pending.flow) throw Error("flow mismatch");
        clearPending();
        props.onConnected({ connection_id: grant.connection_id, epoch: grant.epoch, mailbox: grant.mailbox });
      } catch (error) {
        clearPending();
        if (authFailed(error)) return;
        // Result unknown: trust only the server's current status.
        try {
          const current = await readStatus();
          if (current) props.onConnected(current);
          else setMessage("Google 연결 결과를 확인하지 못했습니다. 다시 연결해 주세요.");
        } catch {
          setMessage("Google 연결 결과를 확인하지 못했습니다. 현재 연결 상태를 다시 확인해 주세요.");
        }
      } finally {
        lock.current = false;
        setBusy(false);
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [accountId, sessionScope]);

  async function connect() {
    if (lock.current || !sessionScope) return;
    lock.current = true;
    setBusy(true);
    setMessage("");
    try {
      const started = await scope.request<{ flow_id: string; authorization_url: string }>(
        `${base}/start`, "POST", {}, undefined, { timeoutMs: 15000 });
      const url = new URL(started.authorization_url);
      if (url.origin !== "https://accounts.google.com" || url.pathname !== "/o/oauth2/v2/auth" ||
          url.username || url.password || url.hash || !flowPattern.test(started.flow_id)) {
        throw Error("invalid consent");
      }
      const pending: Pending = { account: accountId, session: sessionScope, flow: started.flow_id, expires: Date.now() + 600000, phase: "consent", thread: props.threadId };
      sessionStorage.setItem(storageKey, JSON.stringify(pending));
      location.assign(url.href);
    } catch (error) {
      if (!authFailed(error)) setMessage("Google 캘린더 연결을 시작할 수 없습니다.");
      lock.current = false;
      setBusy(false);
    }
  }
  async function disconnect() {
    const c = props.connection;
    if (!c || lock.current) return;
    lock.current = true;
    setBusy(true);
    setMessage("");
    try {
      await scope.request(`${base}/disconnect`, "POST", { connection_id: c.connection_id, epoch: c.epoch }, undefined, { timeoutMs: 15000 });
      props.onDisconnected?.();
    } catch (error) {
      if (!authFailed(error)) setMessage("연결 해제 결과를 확인하지 못했습니다. 상태를 다시 확인해 주세요.");
    } finally {
      lock.current = false;
      setBusy(false);
    }
  }

  return (
    <section className="member-calendar-consent" aria-label="Google 캘린더 연결" aria-busy={busy}>
      {props.connection ? (
        <>
          <p role="status">Google 캘린더 연결됨 · {props.connection.mailbox}</p>
          <button type="button" disabled={busy} onClick={() => void disconnect()}>연결 해제</button>
        </>
      ) : (
        <>
          <p>
            내 기본 캘린더의 일정을 보고, 내가 미리보기로 확인한 일정 하나만 만들거나 옮기는 권한을 요청합니다. 초대는 보내지 않으며,
            Drive나 다른 Google 서비스에는 사용되지 않습니다.
          </p>
          <button type="button" disabled={busy || !sessionScope} onClick={() => void connect()}>Google 캘린더 연결</button>
        </>
      )}
      {message && <p role="alert">{message}</p>}
    </section>
  );
}
