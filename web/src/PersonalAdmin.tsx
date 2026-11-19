import { useEffect, useRef, useState } from "react";
import { ApiFailure, isAborted } from "./api";
import { useScope } from "./account-scope";
import "./personal-management.css";

export const adminBase = "/personal/admin";
export type Member = {
  account_id: string;
  email: string;
  generation: number;
  revision: number;
  admin: boolean;
  eligible: boolean;
  suspended: boolean;
  ready: boolean;
  invitation_request_id?: string | null;
  invitation_name?: string | null;
};
type Change = "promote" | "demote" | "suspend" | "resume";
type Pending = {
  request_id: string;
  account_id: string;
  generation: number;
  revision: number;
  action: Change;
};
export default function PersonalAdmin({
  account,
  unavailable,
  expired,
}: {
  account: string;
  unavailable: () => void;
  expired: () => void;
}) {
  const scope = useScope(account, "installation-admin");
  const [members, setMembers] = useState<Member[]>([]),
    [busy, setBusy] = useState(true),
    [error, setError] = useState("");
  const [pending, setPending] = useState<Pending | null>(null);
  const lock = useRef(false);
  const [name, setName] = useState(""),
    [email, setEmail] = useState(""),
    [receipt, setReceipt] = useState("");
  const [inviteId, setInviteId] = useState<string | null>(null);
  async function invite() {
    if (lock.current || busy || !name.trim() || !email.trim()) return;
    const id = inviteId ?? crypto.randomUUID();
    setInviteId(id);
    lock.current = true;
    setBusy(true);
    setError("");
    setReceipt("");
    try {
      const r = await scope.request<{ request_id: string; email: string }>(
        adminBase + "/invitations",
        "POST",
        { request_id: id, name: name.trim(), email: email.trim() },
      );
      setReceipt(
        r.email + " 계정 준비 요청을 등록했습니다. 요청 번호 " + r.request_id,
      );
      setInviteId(null);
      setName("");
      setEmail("");
      try {
        await load();
      } catch (e) {
        fail(e);
      }
    } catch (e) {
      if (e instanceof ApiFailure && [400, 409, 422].includes(e.status)) {
        setInviteId(null);
        setError("이름과 이메일, 기존 계정의 등록 상태를 확인해 주세요.");
      } else if (e instanceof ApiFailure && [401, 403].includes(e.status)) {
        fail(e);
      } else if (!isAborted(e)) {
        setError(
          "초대 접수 결과를 확인하지 못했습니다. 같은 요청으로 다시 확인해 주세요.",
        );
      }
    } finally {
      lock.current = false;
      setBusy(false);
    }
  }
  function fail(e: unknown) {
    if (isAborted(e)) return;
    if (e instanceof ApiFailure && e.status === 401) {
      expired();
      return;
    }
    if (e instanceof ApiFailure && e.status === 403) {
      unavailable();
      return;
    }
    setError("회원 상태를 불러오지 못했습니다.");
  }
  async function load() {
    const { members } = await scope.request<{ members: Member[] }>(
      adminBase + "/members",
    );
    setMembers(members);
  }
  useEffect(() => {
    let live = true;
    void load()
      .catch(fail)
      .finally(() => {
        if (live) setBusy(false);
      });
    return () => {
      live = false;
    };
  }, [scope]);
  async function submit(p: Pending) {
    if (lock.current) return;
    lock.current = true;
    setBusy(true);
    setError("");
    setPending(p);
    try {
      await scope.request(adminBase + "/members/change", "POST", p);
      setPending(null);
      try {
        await load();
      } catch (e) {
        fail(e);
      }
    } catch (e) {
      if (e instanceof ApiFailure && e.status === 409) {
        setPending(null);
        setError(
          "변경할 수 없습니다. 회원 상태와 사용 가능한 관리자를 확인해 주세요.",
        );
        try {
          await load();
        } catch (refreshError) {
          fail(refreshError);
        }
      } else if (
        e instanceof ApiFailure &&
        [400, 401, 403, 404, 422].includes(e.status)
      ) {
        setPending(null);
        fail(e);
      } else if (!isAborted(e)) {
        setError(
          "변경 결과를 확인하지 못했습니다. 같은 요청으로 결과를 확인할 수 있습니다.",
        );
      }
    } finally {
      lock.current = false;
      setBusy(false);
    }
  }
  const labels: Record<Change, string> = {
    promote: "관리자로 지정",
    demote: "일반 회원으로 변경",
    suspend: "계정 정지",
    resume: "정지 해제",
  };
  function change(m: Member, action: Change) {
    if (!window.confirm(m.email + " 계정을 " + labels[action] + "할까요?"))
      return;
    void submit({
      request_id: crypto.randomUUID(),
      account_id: m.account_id,
      generation: m.generation,
      revision: m.revision,
      action,
    });
  }
  return (
    <section aria-labelledby="member-management-title" className="member-page member-management">
      <header className="member-page-heading">
        <h1 id="member-management-title">회원 관리</h1>
      </header>
      {error && <p className="preview-error" role="status">{error}</p>}
      {pending && !busy && (
        <button className="member-page-action" onClick={() => void submit(pending)}>
          변경 결과 확인
        </button>
      )}
      {busy && <p className="management-note" role="status">확인 중…</p>}
      <section className="management-group" aria-labelledby="member-invite-title">
        <h2 id="member-invite-title">초대</h2>
        <form
          className="member-page-list management-list"
          onSubmit={(e) => {
            e.preventDefault();
            void invite();
          }}
        >
          <label className="management-row management-field">
            <span>이름</span>
            <input
              value={name}
              maxLength={200}
              autoComplete="off"
              required
              disabled={!!inviteId}
              onChange={(e) => setName(e.target.value)}
            />
          </label>
          <label className="management-row management-field">
            <span>이메일</span>
            <input
              type="email"
              value={email}
              maxLength={254}
              autoComplete="off"
              required
              disabled={!!inviteId}
              onChange={(e) => setEmail(e.target.value)}
            />
          </label>
          <div className="management-row management-footer">
            <button
              className="member-page-action management-primary"
              disabled={busy || !!pending || !name.trim() || !email.trim()}
            >
              {inviteId ? "초대 접수 확인" : "회원 초대"}
            </button>
          </div>
        </form>
        {receipt && <p className="management-note" role="status">{receipt}</p>}
      </section>
      <section className="management-group" aria-labelledby="member-list-title">
        <h2 id="member-list-title">회원 목록</h2>
        {members.length > 0 ? (
          <ul className="member-page-list management-list">
            {members.map((m) => (
              <li className="management-row management-person" key={m.account_id}>
                <div className="management-person-details">
                  <strong>{m.invitation_name || m.email}</strong>
                  {m.invitation_name && <span>{m.email}</span>}
                  <span>
                    {m.suspended
                      ? "정지"
                      : !m.ready
                        ? "준비 중"
                        : m.admin
                          ? "관리자"
                          : "회원"}
                    {m.account_id === account ? " · 나" : ""}
                  </span>
                  {m.invitation_request_id && !m.ready && (
                    <small>요청 번호 {m.invitation_request_id}</small>
                  )}
                </div>
                {m.suspended && (
                  <button
                    className="member-page-action"
                    disabled={busy || !!pending || !!inviteId}
                    onClick={() => change(m, "resume")}
                  >
                    {labels.resume}
                  </button>
                )}
                {!m.suspended && m.ready && (
                  <div className="management-person-actions">
                    {(m.admin || m.eligible) && (
                      <button
                        className="member-page-action"
                        disabled={busy || !!pending || !!inviteId}
                        onClick={() => change(m, m.admin ? "demote" : "promote")}
                      >
                        {m.admin ? labels.demote : labels.promote}
                      </button>
                    )}
                    <button
                      className="member-page-action management-danger"
                      disabled={busy || !!pending || !!inviteId}
                      onClick={() => change(m, "suspend")}
                    >
                      {labels.suspend}
                    </button>
                  </div>
                )}
              </li>
            ))}
          </ul>
        ) : !busy && !error && (
          <p className="member-page-list management-row management-note">표시할 회원이 없습니다.</p>
        )}
      </section>
    </section>
  );
}
