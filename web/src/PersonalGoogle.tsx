import { useEffect, useRef, useState } from "react";
import type { SessionView } from "../shared/api-types";
import { ApiFailure, isAborted } from "./api";
import { useScope } from "./account-scope";

const base = "/personal/google",
  flowKey = "moosuri.personal.google-read.v1";
type Grant = { connection_id: string; epoch: number; mailbox: string };
type Inbox = {
  mailbox: string;
  messages: { id: string; subject: string; from: string; date: string }[];
  has_more: boolean;
  scope: string;
};
export type GoogleReadIntent = { thread_id: string; request_id: string };
export type ThreadGoogleRead = {
  thread_id: string;
  request_id: string;
  state: string;
  created_at: number;
  source: (Grant & { app_revision: string }) | null;
  result: Inbox | null;
};
type Pending = {
  flow: string;
  account: string;
  session: string;
  read?: GoogleReadIntent;
  draft?: string;
  expires: number;
  phase: "consent" | "completing" | "reading" | "uncertain";
};
function stored(): Pending | null {
  try {
    return JSON.parse(sessionStorage.getItem(flowKey) ?? "null");
  } catch {
    return null;
  }
}
function belongs(p: Pending | null, s: SessionView): p is Pending {
  return (
    !!p &&
    p.account === s.user.id &&
    !!s.chat_draft_scope &&
    p.session === s.chat_draft_scope &&
    p.expires > Date.now()
  );
}
function intent(v: unknown): v is GoogleReadIntent {
  return (
    !!v &&
    typeof v === "object" &&
    typeof (v as GoogleReadIntent).thread_id === "string" &&
    typeof (v as GoogleReadIntent).request_id === "string"
  );
}
function same(a: GoogleReadIntent, b: GoogleReadIntent) {
  return a.thread_id === b.thread_id && a.request_id === b.request_id;
}
function save(p: Pending) {
  sessionStorage.setItem(flowKey, JSON.stringify(p));
}
function uncertain(p: Pending) {
  try {
    save(p);
  } catch {
    /* Server dispatch history remains authoritative. */
  }
}
export function googleReadDestination(s: SessionView): GoogleReadIntent | null {
  const p = stored();
  return belongs(p, s) && intent(p.read) ? p.read : null;
}
export function googleReturn() {
  const p = new URLSearchParams(location.hash.slice(1));
  return (
    p.has("google_handle") ||
    p.has("google_denied") ||
    ["completing", "reading", "uncertain"].includes(stored()?.phase ?? "")
  );
}
export function clearPersonalGoogle() {
  try {
    sessionStorage.removeItem(flowKey);
  } catch {
    /* storage unavailable */
  }
}
export function GoogleReadHistory({ reads }: { reads: ThreadGoogleRead[] }) {
  return (
    <div aria-label="이 대화의 메일 확인 기록">
      {reads.length > 0 && <h3>저장된 메일 확인 기록 · 최근 20개</h3>}
      {reads.map((r) => (
        <article key={r.request_id} className="preview-google-read">
          <strong>받은편지함 확인</strong>
          <p>
            조회 요청:{" "}
            <time dateTime={new Date(r.created_at * 1000).toISOString()}>
              {new Date(r.created_at * 1000).toLocaleString()}
            </time>
          </p>
          {r.result && r.source ? (
            <>
              <p>
                {r.source.mailbox} · 받은편지함 첫 페이지에서 최대 5건을
                확인했습니다. 전체 메일에서 가장 최근인 항목을 뜻하지 않습니다.
              </p>
              {r.result.messages.length ? (
                <ul>
                  {r.result.messages.map((m) => (
                    <li key={m.id}>
                      <strong>{m.subject || "제목 없음"}</strong>
                      <div>{m.from}</div>
                      <small>{m.date}</small>
                    </li>
                  ))}
                </ul>
              ) : (
                <p>확인한 페이지에 받은 메일이 없습니다.</p>
              )}
              {r.result.has_more && <p>다음 페이지에 메일이 더 있습니다.</p>}
            </>
          ) : (
            <p>
              {r.state === "withdrawn"
                ? "연결이 변경되거나 해제되어 이 결과를 제거했습니다."
                : r.state === "awaiting_consent"
                  ? "Google 연결을 기다리던 요청입니다. 자동으로 다시 연결하거나 읽지 않습니다."
                  : r.state === "ready"
                    ? "연결은 완료됐지만 읽기 결과가 없습니다. 자동으로 읽기를 재개하지 않습니다."
                    : "조회 결과를 확인하지 못했습니다. 자동으로 다시 읽지 않았습니다."}
            </p>
          )}
        </article>
      ))}
    </div>
  );
}
export default function PersonalGoogle({
  session,
  expired,
  readIntent,
  readDraft,
  returned,
}: {
  session: SessionView;
  expired: () => void;
  readIntent?: GoogleReadIntent | null;
  readDraft?: string;
  returned?: (
    intent: GoogleReadIntent,
    uncertain?: boolean,
    draft?: string,
  ) => void;
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
    setNotice("연결 상태를 확인할 수 없습니다. 자동으로 다시 읽지 않았습니다.");
  }
  function pending(
    read: GoogleReadIntent | undefined,
    phase: Pending["phase"],
    flow = "",
    draft = readDraft,
  ): Pending {
    return {
      flow,
      account: session.user.id,
      session: session.chat_draft_scope!,
      read,
      draft: read ? draft : undefined,
      phase,
      expires: Date.now() + 600000,
    };
  }
  function owns(destination: GoogleReadIntent) {
    const p = stored();
    return belongs(p, session) && intent(p.read) && same(p.read, destination);
  }
  async function read(
    g: Grant,
    destination?: GoogleReadIntent,
    draftText = readDraft,
  ) {
    if (destination) save(pending(destination, "reading", "", draftText));
    try {
      if (destination) {
        const result = await scope.request<ThreadGoogleRead>(
          base + "/inbox",
          "POST",
          { connection_id: g.connection_id, epoch: g.epoch, read: destination },
          undefined,
          { timeoutMs: 75000 },
        );
        if (!same(result, destination))
          throw Error("read destination mismatch");
        if (!owns(destination)) return;
        clearPersonalGoogle();
        returned?.(destination, result.state !== "completed", draftText);
      } else
        setInbox(
          await scope.request<Inbox>(
            base + "/inbox",
            "POST",
            { connection_id: g.connection_id, epoch: g.epoch },
            undefined,
            { timeoutMs: 75000 },
          ),
        );
    } catch (e) {
      if (isAborted(e)) return;
      if (destination && owns(destination)) {
        uncertain(pending(destination, "uncertain", "", draftText));
        returned?.(destination, true, draftText);
      }
      fail(e);
    }
  }
  async function update(destination?: GoogleReadIntent) {
    try {
      const v = await scope.request<{ connection: Grant | null }>(base);
      setConnection(v.connection);
      setEnabled(true);
      if (destination && v.connection) await read(v.connection, destination);
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  }
  useEffect(() => {
    const timer = window.setTimeout(
      () =>
        void (async () => {
          const fields = new URLSearchParams(location.hash.slice(1)),
            p = stored();
          if (!fields.has("google_handle") && !fields.has("google_denied")) {
            if (
              belongs(p, session) &&
              intent(p.read) &&
              ["completing", "reading", "uncertain"].includes(p.phase)
            ) {
              const destination = p.read;
              clearPersonalGoogle();
              returned?.(destination, true, p.draft);
              setBusy(false);
              return;
            }
            await update(readIntent ?? undefined);
            return;
          }
          const flow = fields.get("flow") ?? fields.get("google_denied"),
            handle = fields.get("google_handle");
          history.replaceState(null, "", location.pathname);
          if (
            !belongs(p, session) ||
            p.flow !== flow ||
            p.phase !== "consent"
          ) {
            await update();
            setNotice("연결을 시작한 계정과 탭에서 다시 진행해 주세요.");
            return;
          }
          if (!handle || !/^[a-f0-9]{64}$/.test(handle)) {
            clearPersonalGoogle();
            await update();
            setNotice("Google 연결이 취소됐습니다.");
            return;
          }
          try {
            save({ ...p, phase: "completing" });
            const completed = await scope.request<
              Grant & { flow_id: string; continuation?: GoogleReadIntent }
            >(base + "/complete", "POST", { handle }, undefined, {
              timeoutMs: 75000,
            });
            if (completed.flow_id !== p.flow) throw Error("flow mismatch");
            if (stored()?.flow !== p.flow) return;
            setConnection(completed);
            setEnabled(true);
            if (intent(p.read)) {
              if (
                !completed.continuation ||
                !same(p.read, completed.continuation)
              )
                throw Error("continuation mismatch");
              await read(completed, completed.continuation, p.draft);
            } else clearPersonalGoogle(); // Manual connection never resumes a read.
          } catch (e) {
            if (isAborted(e)) return;
            if (stored()?.flow !== p.flow) return;
            if (intent(p.read)) {
              uncertain({ ...p, phase: "uncertain" });
              returned?.(p.read, true, p.draft);
            } else clearPersonalGoogle();
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
      } else if (name === "read" && connection)
        await read(connection, readIntent ?? undefined);
      else {
        const destination =
          name === "read" ? (readIntent ?? undefined) : undefined;
        const started = await scope.request<{
          flow_id: string;
          authorization_url: string;
        }>(base + "/start", "POST", destination ? { read: destination } : {});
        const url = new URL(started.authorization_url);
        if (
          url.origin !== "https://accounts.google.com" ||
          url.pathname !== "/o/oauth2/v2/auth" ||
          !session.chat_draft_scope
        )
          throw Error("invalid consent");
        save(pending(destination, "consent", started.flow_id));
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
        <>
          <button
            disabled={busy}
            onClick={() =>
              void action(readIntent || connection ? "read" : "connect")
            }
          >
            {connection
              ? "메일 확인"
              : readIntent
                ? "Google로 연결하고 이 대화에서 메일 확인"
                : "Google 연결"}
          </button>
          {readIntent && !connection && (
            <button disabled={busy} onClick={() => void action("connect")}>
              Google 연결만
            </button>
          )}
        </>
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
        <>
          <p>
            받은편지함 첫 페이지에서 최대 5건을 확인했습니다. 전체 메일에서 가장
            최근인 항목을 뜻하지 않습니다.
          </p>
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
            <p>확인한 페이지에 받은 메일이 없습니다.</p>
          )}
          {inbox.has_more && <p>다음 페이지에 메일이 더 있습니다.</p>}
        </>
      )}
    </section>
  );
}
