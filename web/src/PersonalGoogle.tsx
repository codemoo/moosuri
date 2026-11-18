import { useEffect, useRef, useState } from "react";
import type { SessionView } from "../shared/api-types";
import { ApiFailure, isAborted } from "./api";
import { useScope } from "./account-scope";

const base = "/personal/google";
const flowKey = "moosuri.personal.google-read.v1";
type Grant = { connection_id: string; epoch: number; mailbox: string };
type Status = { connection: Grant | null };
type Inbox = {
  mailbox: string;
  messages: { id: string; subject: string; from: string; date: string }[];
  has_more: boolean;
};
type Pending = {
  flow: string;
  account: string;
  session: string;
  read: boolean;
  expires: number;
};
export function googleReturn() {
  const p = new URLSearchParams(location.hash.slice(1));
  return p.has("google_handle") || p.has("google_denied");
}
function stored(): Pending | null {
  try {
    return JSON.parse(sessionStorage.getItem(flowKey) ?? "null");
  } catch {
    return null;
  }
}
export function clearPersonalGoogle() {
  try {
    sessionStorage.removeItem(flowKey);
  } catch {
    /* storage unavailable */
  }
}
export default function PersonalGoogle({
  session,
  expired,
  readOnOpen = false,
  consumed,
}: {
  session: SessionView;
  expired: () => void;
  readOnOpen?: boolean;
  consumed?: () => void;
}) {
  const scope = useScope(session.user.id, "personal-google");
  const [connection, setConnection] = useState<Grant | null>(null),
    [inbox, setInbox] = useState<Inbox | null>(null);
  const [notice, setNotice] = useState(""),
    [enabled, setEnabled] = useState(false),
    [busy, setBusy] = useState(true);
  const lock = useRef(false);
  function fail(e: unknown) {
    if (isAborted(e)) return;
    if (e instanceof ApiFailure && e.status === 401) {
      expired();
      return;
    }
    if (e instanceof ApiFailure && e.status === 404) {
      setEnabled(false);
      setNotice("설치 관리자가 Google 연결을 활성화하면 사용할 수 있습니다.");
      return;
    }
    setNotice("연결 상태를 확인할 수 없습니다. 다시 확인해 주세요.");
  }
  async function read(g: Grant) {
    const contents = await scope.request<Inbox>(
      base + "/inbox",
      "POST",
      { connection_id: g.connection_id, epoch: g.epoch },
      undefined,
      { timeoutMs: 75000 },
    );
    setInbox(contents);
  }
  async function update(readConnected = false) {
    setBusy(true);
    setNotice("");
    try {
      const v = await scope.request<Status>(base);
      setConnection(v.connection);
      setEnabled(true);
      if (readConnected && v.connection) await read(v.connection);
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  }
  useEffect(() => {
    // StrictMode may mount twice. Start irreversible completion only on the live mount.
    const timer = window.setTimeout(
      () =>
        void (async () => {
          if (!googleReturn()) {
            if (readOnOpen) consumed?.();
            await update(readOnOpen);
            return;
          }
          const fields = new URLSearchParams(location.hash.slice(1));
          const flow = fields.get("flow") ?? fields.get("google_denied"),
            handle = fields.get("google_handle");
          const pending = stored();
          history.replaceState(null, "", location.pathname);
          const matches =
            pending &&
            pending.flow === flow &&
            pending.account === session.user.id &&
            !!session.chat_draft_scope &&
            pending.session === session.chat_draft_scope &&
            pending.expires > Date.now();
          if (!matches) {
            await update();
            setNotice("연결을 시작한 계정과 탭에서 다시 진행해 주세요.");
            return;
          }
          clearPersonalGoogle();
          if (!handle || !/^[a-f0-9]{64}$/.test(handle)) {
            await update();
            setNotice("Google 연결이 취소됐습니다.");
            return;
          }
          try {
            const completed = await scope.request<Grant & { flow_id: string }>(
              base + "/complete",
              "POST",
              { handle },
              undefined,
              { timeoutMs: 75000 },
            );
            if (completed.flow_id !== pending.flow)
              throw Error("flow mismatch");
            setConnection(completed);
            setEnabled(true);
            if (pending.read) await read(completed);
          } catch (e) {
            fail(e);
          } finally {
            setBusy(false);
          }
        })(),
      0,
    );
    return () => clearTimeout(timer);
  }, [scope, session.user.id, session.chat_draft_scope]);
  async function action(name: "connect" | "read" | "disconnect") {
    if (lock.current || busy) return;
    lock.current = true;
    setBusy(true);
    setNotice("");
    try {
      if (name === "disconnect" && connection) {
        await scope.request(base + "/disconnect", "POST", {
          connection_id: connection.connection_id,
          epoch: connection.epoch,
        });
        clearPersonalGoogle();
        setConnection(null);
        setInbox(null);
      } else if (name === "read" && connection) {
        await read(connection);
      } else {
        const started = await scope.request<{
          flow_id: string;
          authorization_url: string;
        }>(base + "/start", "POST", {});
        const url = new URL(started.authorization_url);
        if (
          url.origin !== "https://accounts.google.com" ||
          url.pathname !== "/o/oauth2/v2/auth" ||
          !session.chat_draft_scope
        )
          throw Error("invalid consent");
        sessionStorage.setItem(
          flowKey,
          JSON.stringify({
            flow: started.flow_id,
            account: session.user.id,
            session: session.chat_draft_scope,
            read: name === "read",
            expires: Date.now() + 600000,
          } satisfies Pending),
        );
        location.assign(url.href);
      }
    } catch (e) {
      fail(e);
    } finally {
      lock.current = false;
      setBusy(false);
    }
  }
  return (
    <section aria-label="받은편지함" className="preview-gate">
      <h1>받은편지함</h1>
      {connection && <p>{connection.mailbox}</p>}
      {notice && <p role="status">{notice}</p>}
      {busy && <p role="status">확인 중…</p>}
      {enabled && (
        <button disabled={busy} onClick={() => void action("read")}>
          {connection ? "메일 확인" : "Google로 연결하고 메일 확인"}
        </button>
      )}
      {connection && (
        <button disabled={busy} onClick={() => void action("disconnect")}>
          연결 해제
        </button>
      )}
      {!enabled && !busy && (
        <button onClick={() => void update()}>연결 상태 다시 확인</button>
      )}
      {inbox && (
        <div aria-label="메일 목록">
          {inbox.messages.length ? (
            <ul>
              {inbox.messages.map((m) => (
                <li key={m.id}>
                  <strong>{m.subject || "제목 없음"}</strong>
                  <div>{m.from}</div>
                  <small>{m.date}</small>
                </li>
              ))}
            </ul>
          ) : (
            <p>받은 메일이 없습니다.</p>
          )}
          {inbox.has_more && <p>받은편지함의 첫 다섯 건을 표시했습니다.</p>}
        </div>
      )}
    </section>
  );
}
