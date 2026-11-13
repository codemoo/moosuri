import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type FormEvent,
} from "react";
import type {
  MailAction,
  MailConnections as Connections,
  MailDraftContent,
} from "../shared/api-types";
import { useScope } from "./account-scope";
import { ApiFailure, errorText, isAborted } from "./api";
import { MailApproval } from "./MailApproval";
import "./mail.css";
type ReadMessage = {
  id: string;
  thread_id: string;
  mail: {
    subject: string;
    from: string[];
    to: string[];
    text: string;
    text_truncated: boolean;
    attachment_names: string[];
  };
};
function adopt(current: MailAction[], incoming: MailAction[]) {
  return incoming.map((next) => {
    const old = current.find((old) => old.id === next.id);
    return old && old.version > next.version ? old : next;
  });
}
const blank: MailDraftContent = {
  from: "",
  to: [],
  cc: [],
  bcc: [],
  subject: "",
  text: "",
  reply: null,
};
export function MailConnections({
  accountId,
  onExpired,
  back,
  originThreadId,
  openGoogleSetup,
}: {
  accountId: string;
  onExpired: () => void;
  back: () => void;
  originThreadId: string | null;
  openGoogleSetup?: () => void;
}) {
  const scope = useScope(accountId);
  const [connections, setConnections] = useState<Connections | null>(null);
  const [connection, setConnection] = useState("");
  const [actions, setActions] = useState<MailAction[]>([]);
  const [selected, setSelected] = useState<string | null>(null);
  const [draft, setDraft] = useState<MailDraftContent>(blank);
  const [editing, setEditing] = useState<MailAction | null>(null);
  const [recipients, setRecipients] = useState({ to: "", cc: "", bcc: "" });
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [query, setQuery] = useState("in:inbox");
  const [pageToken, setPageToken] = useState<string | null>(null);
  const [results, setResults] = useState<{ id: string; threadId: string }[]>(
    [],
  );
  const [message, setMessage] = useState<ReadMessage | null>(null);
  const [uncertain, setUncertain] = useState(false);
  const oauthStarted = useRef(false);
  function fail(e: unknown) {
    if (isAborted(e)) return;
    if (e instanceof ApiFailure && e.status === 401) onExpired();
    else setError(errorText(e));
  }
  async function reload() {
    const [c, a] = await Promise.all([
      scope.request<Connections>("/mail/connections"),
      scope.request<{ actions: MailAction[] }>("/mail/actions"),
    ]);
    setConnections(c);
    setActions((old) => adopt(old, a.actions));
    setConnection((old) =>
      c.connections.some((v) => v.id === old && v.status === "active")
        ? old
        : (c.connections.find((v) => v.status === "active")?.id ?? ""),
    );
    setUncertain(false);
  }
  useEffect(() => {
    void reload().catch(fail);
    const timer = window.setInterval(() => {
      void scope
        .request<{ actions: MailAction[] }>("/mail/actions")
        .then((a) => setActions((old) => adopt(old, a.actions)))
        .catch(fail);
    }, 1500);
    return () => window.clearInterval(timer);
  }, [scope]);
  useEffect(() => {
    const params = new URLSearchParams(window.location.search);
    const handle = params.get("oauth");
    const denied = params.get("oauth_error");
    if (handle || denied)
      window.history.replaceState(null, "", window.location.pathname);
    if (denied)
      setError("Google 연결을 허락하지 않았습니다. 원할 때 다시 연결하세요.");
    if (handle && !oauthStarted.current) {
      oauthStarted.current = true;
      setBusy(true);
      void scope
        .request("/mail/oauth/complete", "POST", { handle })
        .then(reload)
        .catch(fail)
        .finally(() => setBusy(false));
    }
  }, [scope]);
  useEffect(() => {
    const c = connections?.connections.find((c) => c.id === connection);
    if (c && !editing) setDraft((v) => ({ ...v, from: c.email }));
    setResults([]);
    setMessage(null);
    setPageToken(null);
  }, [connection, connections, editing]);
  // Incoming purge/revision changes invalidate editor copies as well as the
  // preview. Preserve unsaved edits only while their original snapshot remains
  // pending with the same revision/digest; clear before the next browser paint.
  useLayoutEffect(() => {
    if (!editing) return;
    const latest = actions.find((a) => a.id === editing.id);
    if (
      !latest ||
      !latest.content ||
      latest.state !== "pending_approval" ||
      latest.revision !== editing.revision ||
      latest.digest !== editing.digest ||
      latest.connection_id !== editing.connection_id
    ) {
      setEditing(null);
      setDraft({ ...blank });
      setRecipients({ to: "", cc: "", bcc: "" });
    }
  }, [actions, editing]);
  const action = actions.find((a) => a.id === selected) ?? null;
  async function connect() {
    setBusy(true);
    setError("");
    try {
      const r = await scope.request<{ authorization_url: string }>(
        "/mail/oauth/start",
        "POST",
        {},
      );
      const u = new URL(r.authorization_url);
      if (u.origin !== "https://accounts.google.com")
        throw new Error("Invalid OAuth destination");
      window.location.assign(u.href);
    } catch (e) {
      fail(e);
      setBusy(false);
    }
  }
  async function search(next = false) {
    setBusy(true);
    setError("");
    setMessage(null);
    try {
      const r = await scope.request<{
        messages?: { id: string; threadId: string }[];
        nextPageToken?: string;
      }>("/mail/search", "POST", {
        connection_id: connection,
        query,
        page_token: next ? pageToken : null,
      });
      setResults(r.messages ?? []);
      setPageToken(r.nextPageToken ?? null);
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  }
  async function read(id: string) {
    setBusy(true);
    try {
      setMessage(
        await scope.request<ReadMessage>(
          `/mail/messages/${encodeURIComponent(id)}?connection_id=${encodeURIComponent(connection)}`,
        ),
      );
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  }
  async function save(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    setError("");
    try {
      const split = (s: string) =>
        s
          .split(",")
          .map((s) => s.trim())
          .filter(Boolean);
      const content = {
        ...draft,
        to: split(recipients.to),
        cc: split(recipients.cc),
        bcc: split(recipients.bcc),
      };
      const input = {
        connection_id: connection,
        content,
        origin_thread_id: editing?.origin_thread_id ?? originThreadId,
      };
      const a = editing
        ? await scope.request<MailAction>(
            `/mail/actions/${editing.id}`,
            "PUT",
            { revision: editing.revision, input },
          )
        : await scope.request<MailAction>("/mail/actions", "POST", input);
      setActions((v) => adopt(v, [a, ...v.filter((x) => x.id !== a.id)]));
      setSelected(a.id);
      setEditing(null);
      setDraft({ ...blank, from: draft.from });
      setRecipients({ to: "", cc: "", bcc: "" });
    } catch (e) {
      fail(e);
    } finally {
      setBusy(false);
    }
  }
  async function decide(approve: boolean) {
    if (!action || uncertain || busy || editing) return;
    setError("");
    try {
      const a = await scope.request<MailAction>(
        `/mail/actions/${action.id}/${approve ? "approve" : "reject"}`,
        "POST",
        { revision: action.revision, digest: action.digest },
      );
      setActions((v) =>
        adopt(
          v,
          v.map((x) => (x.id === a.id ? a : x)),
        ),
      );
    } catch (e) {
      if (!isAborted(e)) {
        setUncertain(true);
        setError(
          "허락 처리 결과를 확인하지 못했습니다. 상태 새로고침으로 확인하세요. 자동으로 다시 보내지 않습니다.",
        );
      }
      fail(e);
    }
  }
  return (
    <main className="mail-scroll">
      <h2>내 Gmail과 메일 확인</h2>
      <p>
        메일을 읽고 무수리 로컬 초안을 준비합니다. 모든 발송은 전체 내용을
        확인한 뒤 허락해야 합니다.
      </p>
      <button className="text-button" onClick={back}>
        ← 대화로 돌아가기
      </button>
      {error && (
        <p role="alert" className="notice">
          {error}
        </p>
      )}
      <button onClick={() => void reload().catch(fail)}>상태 새로고침</button>
      {!connections ? (
        <p role="status">연결 상태 확인 중…</p>
      ) : (
        <section className="admin-card">
          <h3>개인 연결</h3>
          {!connections.configured && (
            <div><p>{openGoogleSetup ? "설치의 Google 앱 설정을 먼저 준비하세요. 설정 후 각 회원이 자신의 계정으로 연결합니다." : "설치자에게 Google 연결 설정을 요청하세요. 준비가 끝나면 여기서 본인 Google 계정으로 로그인할 수 있습니다."} 다른 기능은 계속 사용할 수 있습니다.</p>{openGoogleSetup && <button onClick={openGoogleSetup}>Google 설치 설정 열기</button>}</div>
          )}
          <p>
            이 연결 토큰은 서버에서 관리합니다. 연결 해제는 새 작업을 막지만
            이미 요청된 메일을 회수하지 않습니다.
          </p>
          <button
            disabled={busy || !connections.configured}
            onClick={() => void connect()}
          >
            Google 계정 연결
          </button>
          <ul>
            {connections.connections.map((c) => (
              <li key={c.id}>
                {c.email} · {c.status}
                <button
                  disabled={busy || c.status === "disconnected"}
                  onClick={async () => {
                    setBusy(true);
                    try {
                      await scope.request(
                        `/mail/connections/${c.id}`,
                        "DELETE",
                      );
                      await reload();
                    } catch (e) {
                      fail(e);
                    } finally {
                      setBusy(false);
                    }
                  }}
                >
                  연결 해제
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
      {connection && (
        <>
          <label>
            사용할 계정
            <select
              value={connection}
              disabled={busy || !!editing}
              onChange={(e) => setConnection(e.target.value)}
            >
              {connections?.connections
                .filter((c) => c.status === "active")
                .map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.email}
                  </option>
                ))}
            </select>
          </label>
          <section className="admin-card">
            <h3>실시간 메일 조회</h3>
            <form
              onSubmit={(e) => {
                e.preventDefault();
                void search();
              }}
            >
              <label>
                Gmail 검색
                <input
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  maxLength={2048}
                />
              </label>
              <button disabled={busy}>검색</button>
              {pageToken && (
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void search(true)}
                >
                  다음 검색 결과
                </button>
              )}
            </form>
            <ul>
              {results.map((r) => (
                <li key={r.id}>
                  <button disabled={busy} onClick={() => void read(r.id)}>
                    메일 {r.id} 읽기
                  </button>
                </li>
              ))}
            </ul>
            {message && (
              <article>
                <h4>{message.mail.subject}</h4>
                <p>
                  {message.mail.from.join(", ")} → {message.mail.to.join(", ")}
                </p>
                <pre className="mail-body">{message.mail.text}</pre>
                {message.mail.text_truncated && (
                  <p>표시 범위를 넘은 본문이 있습니다.</p>
                )}
                {message.mail.attachment_names.length > 0 && (
                  <p>
                    첨부: {message.mail.attachment_names.join(", ")} · 첨부 내용
                    조회는 아직 지원하지 않습니다.
                  </p>
                )}
              </article>
            )}
          </section>
          <section className="admin-card">
            <h3>
              {editing ? "초안 수정 · 다시 허락 필요" : "무수리 로컬 초안"}
            </h3>
            <p>
              Gmail 초안함에 저장되지 않습니다. 첨부·HTML·다른 보내는 주소는
              지원하지 않습니다.
            </p>
            <form className="mail-compose" onSubmit={save}>
              <label>
                From
                <input value={draft.from} readOnly />
              </label>
              {(["to", "cc", "bcc"] as const).map((field) => (
                <label key={field}>
                  {field.toUpperCase()}
                  <input
                    value={recipients[field]}
                    onChange={(e) =>
                      setRecipients((v) => ({ ...v, [field]: e.target.value }))
                    }
                    placeholder="name@example.com, other@example.com"
                  />
                </label>
              ))}
              <label>
                제목
                <input
                  value={draft.subject}
                  onChange={(e) =>
                    setDraft((v) => ({ ...v, subject: e.target.value }))
                  }
                  maxLength={1000}
                />
              </label>
              <label>
                전체 본문
                <textarea
                  rows={10}
                  value={draft.text}
                  onChange={(e) =>
                    setDraft((v) => ({ ...v, text: e.target.value }))
                  }
                  maxLength={8000}
                />
              </label>
              <button disabled={busy}>
                {editing
                  ? "수정본 저장하고 다시 확인"
                  : "초안 저장하고 발송 내용 확인"}
              </button>
              {editing && (
                <button
                  type="button"
                  onClick={() => {
                    setEditing(null);
                    setDraft({ ...blank, from: draft.from });
                  }}
                >
                  수정 취소
                </button>
              )}
            </form>
          </section>
        </>
      )}
      <section className="admin-card">
        <h3>발송 허락과 결과</h3>
        <ul>
          {actions.map((a) => (
            <li key={a.id}>
              <button
                onClick={() => {
                  setSelected(a.id);
                  setUncertain(false);
                }}
              >
                {a.content?.subject || "메일 작업"} · {a.state}
              </button>
            </li>
          ))}
        </ul>
        {action && (
          <MailApproval
            key={`${action.id}:${action.revision}:${uncertain}`}
            action={action}
            disabled={uncertain || busy || !!editing}
            decide={decide}
            checkSent={async () => {
              if (busy || action.state !== "outcome_unknown") return;
              setBusy(true);
              try {
                const checked = await scope.request<MailAction>(`/mail/actions/${encodeURIComponent(action.id)}/check-sent`, "POST", {});
                setActions((v) => adopt(v, v.map((x) => x.id === checked.id ? checked : x)));
              } catch (e) { fail(e); } finally { setBusy(false); }
            }}
            edit={() => {
              if (action.content) {
                setEditing(action);
                setDraft(action.content);
                setRecipients({
                  to: action.content.to.join(", "),
                  cc: action.content.cc.join(", "),
                  bcc: action.content.bcc.join(", "),
                });
                setConnection(action.connection_id);
              }
            }}
          />
        )}
        {uncertain && (
          <p role="status">
            결과 확인 전 추가 허락을 중단했습니다. 상태 새로고침을 눌러 주세요.
          </p>
        )}
      </section>
    </main>
  );
}
