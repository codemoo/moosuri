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
type ChangeRequest = {
  request_id: string;
  account_id: string;
  generation: number;
  revision: number;
  action: Change;
};
type InvitationReceipt = { request_id: string; email: string };
type MemberFilter = "all" | "ready" | "preparing" | "suspended" | "admin";
const memberFilters: { value: MemberFilter; label: string }[] = [
  { value: "all", label: "전체" },
  { value: "ready", label: "로그인 가능" },
  { value: "preparing", label: "계정 준비" },
  { value: "suspended", label: "정지" },
  { value: "admin", label: "관리자" },
];
const memberGuide = "https://github.com/codemoo/moosuri/blob/main/deployment/kiri/admin.md#초대받은-회원의-공간-준비";
const actions: Record<Change, { label: string; question: string; detail: string; result: string }> = {
  promote: {
    label: "관리자로 지정",
    question: "관리자 권한을 부여할까요?",
    detail: "이 회원은 다른 회원과 공용 엔진을 관리할 수 있습니다.",
    result: "관리자로 지정했습니다.",
  },
  demote: {
    label: "일반 회원으로 변경",
    question: "일반 회원으로 변경할까요?",
    detail: "이 회원은 설치 관리 권한을 잃습니다.",
    result: "일반 회원으로 변경했습니다.",
  },
  suspend: {
    label: "계정 정지",
    question: "계정 이용을 정지할까요?",
    detail: "이 회원의 로그인과 무수리 접속을 차단합니다.",
    result: "계정을 정지했습니다.",
  },
  resume: {
    label: "정지 해제",
    question: "계정 정지를 해제할까요?",
    detail: "이 회원은 다시 로그인하면 서비스를 이용할 수 있습니다.",
    result: "계정 정지를 해제했습니다. 회원은 다시 로그인해야 합니다.",
  },
};

function memberState(member: Member) {
  if (member.suspended) return "suspended";
  return member.ready ? "ready" : "preparing";
}

function matchesFilter(member: Member, filter: MemberFilter) {
  if (filter === "all") return true;
  if (filter === "admin") return member.admin;
  return memberState(member) === filter;
}

export default function PersonalAdmin({ account, unavailable, expired }: {
  account: string;
  unavailable: () => void;
  expired: () => void;
}) {
  const scope = useScope(account, "installation-admin");
  const [members, setMembers] = useState<Member[] | null>(null);
  const [listCurrent, setListCurrent] = useState(false);
  const [busy, setBusy] = useState<"list" | "invite" | "change" | null>("list");
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [pending, setPending] = useState<{ request: ChangeRequest; email: string } | null>(null);
  const [confirm, setConfirm] = useState<{ member: Member; action: Change; trigger: HTMLButtonElement } | null>(null);
  const lock = useRef(false);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<MemberFilter>("all");
  const [inviteOpen, setInviteOpen] = useState(false);
  const inviteToggle = useRef<HTMLButtonElement>(null);
  const inviteName = useRef<HTMLInputElement>(null);
  const [name, setName] = useState("");
  const [email, setEmail] = useState("");
  const [receipt, setReceipt] = useState<InvitationReceipt | null>(null);
  const [inviteId, setInviteId] = useState<string | null>(null);

  useEffect(() => {
    if (inviteOpen) inviteName.current?.focus();
  }, [inviteOpen]);

  function fail(e: unknown, message: string) {
    if (isAborted(e)) return;
    if (e instanceof ApiFailure && e.status === 401) expired();
    else if (e instanceof ApiFailure && e.status === 403) unavailable();
    else setError(message);
  }
  async function load(signal?: AbortSignal) {
    try {
      const { members } = await scope.request<{ members: Member[] }>(
        adminBase + "/members", "GET", undefined, signal,
      );
      setMembers(members);
      setListCurrent(true);
    } catch (e) {
      if (!isAborted(e)) setListCurrent(false);
      throw e;
    }
  }
  useEffect(() => {
    const controller = new AbortController();
    void load(controller.signal)
      .catch(e => fail(e, "회원 목록을 불러오지 못했습니다. 목록을 새로고침해 주세요."))
      .finally(() => {
        if (!controller.signal.aborted) setBusy(null);
      });
    return () => controller.abort();
  }, [scope]);

  async function refresh() {
    if (lock.current || busy) return;
    lock.current = true;
    setBusy("list");
    setError("");
    try {
      await load();
    } catch (e) {
      fail(e, "회원 목록을 불러오지 못했습니다. 목록을 새로고침해 주세요.");
    } finally {
      lock.current = false;
      setBusy(null);
    }
  }
  async function invite() {
    if (lock.current || busy || pending || !name.trim() || !email.trim()) return;
    const id = inviteId ?? crypto.randomUUID();
    setInviteId(id);
    lock.current = true;
    setBusy("invite");
    setError("");
    setNotice("");
    setReceipt(null);
    try {
      const result = await scope.request<InvitationReceipt>(
        adminBase + "/invitations", "POST",
        { request_id: id, name: name.trim(), email: email.trim() }, undefined, { timeoutMs: 30_000 },
      );
      setReceipt(result);
      setInviteId(null);
      setName("");
      setEmail("");
      setInviteOpen(false);
      inviteToggle.current?.focus();
      try {
        await load();
      } catch (e) {
        fail(e, "초대 요청은 등록됐습니다. 최신 상태를 보려면 회원 목록을 새로고침해 주세요.");
      }
    } catch (e) {
      if (e instanceof ApiFailure && [400, 409, 422].includes(e.status)) {
        setInviteId(null);
        setError(e.status === 409
          ? "이 이메일로 이미 등록된 회원이나 초대 요청이 있는지 목록에서 확인해 주세요."
          : "이름과 이메일을 확인한 뒤 다시 등록해 주세요.");
      } else {
        fail(e, "초대 접수 결과를 받지 못했습니다. ‘초대 접수 확인’을 눌러 같은 요청의 결과를 확인하세요.");
      }
    } finally {
      lock.current = false;
      setBusy(null);
    }
  }
  async function submit(p: NonNullable<typeof pending>) {
    if (lock.current || busy) return;
    lock.current = true;
    setBusy("change");
    setError("");
    setNotice("");
    setPending(p);
    try {
      await scope.request(adminBase + "/members/change", "POST", p.request, undefined, { timeoutMs: 30_000 });
      setPending(null);
      setNotice(`${p.email} · ${actions[p.request.action].result}`);
      try {
        await load();
      } catch (e) {
        fail(e, "변경이 적용됐습니다. 최신 상태를 보려면 회원 목록을 새로고침해 주세요.");
      }
    } catch (e) {
      if (e instanceof ApiFailure && [400, 401, 403, 404, 409, 422].includes(e.status)) {
        setPending(null);
        fail(e, "회원 상태가 달라졌거나 이 변경을 적용할 수 없습니다. 최신 목록을 확인해 주세요.");
        if (e.status !== 401 && e.status !== 403) {
          try {
            await load();
          } catch (refreshError) {
            fail(refreshError, "변경을 적용하지 못했습니다. 회원 목록을 새로고침해 주세요.");
          }
        }
      } else {
        fail(e, "변경 결과를 받지 못했습니다. ‘변경 결과 확인’을 눌러 같은 요청의 결과를 확인하세요.");
      }
    } finally {
      lock.current = false;
      setBusy(null);
    }
  }
  function change(m: Member, action: Change) {
    setConfirm(null);
    void submit({
      email: m.email,
      request: {
        request_id: crypto.randomUUID(),
        account_id: m.account_id,
        generation: m.generation,
        revision: m.revision,
        action,
      },
    });
  }
  const administrators = members?.filter(m => m.admin && m.eligible && m.ready && !m.suspended).length;
  const controlsDisabled = !!busy || !!pending || !!inviteId || !listCurrent;
  const receiptMember = receipt && members?.find(m => m.email === receipt.email);
  const search = query.trim().toLocaleLowerCase();
  const visibleMembers = members?.filter(m => matchesFilter(m, filter) && (
    !search || m.email.toLocaleLowerCase().includes(search) || m.invitation_name?.toLocaleLowerCase().includes(search)
  ));

  return (
    <section aria-labelledby="member-management-title" className="member-page member-management">
      <header className="member-page-heading management-heading">
        <div>
          <h1 id="member-management-title">회원 관리</h1>
          <p className="management-note">계정을 초대하고 로그인 상태와 관리자 권한을 관리합니다.</p>
        </div>
        <button ref={inviteToggle} type="button" className="member-page-action management-primary"
          aria-expanded={inviteOpen} aria-controls="member-invite" aria-disabled={!!inviteId || !!pending}
          onClick={() => { if (!inviteId && !pending) setInviteOpen(!inviteOpen); }}>
          회원 초대
        </button>
      </header>
      {error && <p className="preview-error" role="alert">{error}</p>}
      {notice && <p className="management-feedback" role="status">{notice}</p>}
      {pending && (
        <div className="member-page-list management-row management-connection">
          <span>{pending.email} · {actions[pending.request.action].label}</span>
          <button className="member-page-action" disabled={!!busy} onClick={() => void submit(pending)}>
            {busy === "change" ? "변경 처리 중…" : "변경 결과 확인"}
          </button>
        </div>
      )}
      <section id="member-invite" className="member-page-list management-invitation" aria-labelledby="member-invite-title" hidden={!inviteOpen}>
        <h2 id="member-invite-title">초대 요청</h2>
        <p className="management-note" id="member-invite-help">요청을 등록하면 설치 서버에서 계정을 준비할 때 쓸 요청 번호를 받습니다.</p>
        <form className="management-invite-form" aria-labelledby="member-invite-title" aria-describedby="member-invite-help"
          onSubmit={e => { e.preventDefault(); void invite(); }}>
          <div className="management-form-fields">
            <label className="management-field">
              <span>이름</span>
              <input ref={inviteName} value={name} maxLength={200} autoComplete="off" required
                readOnly={!!inviteId} onChange={e => setName(e.target.value)} />
            </label>
            <label className="management-field">
              <span>이메일</span>
              <input type="email" value={email} maxLength={254} autoComplete="off" required
                readOnly={!!inviteId} onChange={e => setEmail(e.target.value)} />
            </label>
          </div>
          <div className="management-footer">
            <button type="button" className="member-page-action" disabled={!!inviteId} onClick={() => {
              setInviteOpen(false);
              inviteToggle.current?.focus();
            }}>닫기</button>
            <button className="member-page-action management-primary" disabled={!!busy || !!pending || !name.trim() || !email.trim()}>
              {busy === "invite" ? "초대 요청 처리 중…" : inviteId ? "초대 접수 확인" : "초대 요청 등록"}
            </button>
          </div>
        </form>
      </section>
      {receipt && (
        <section className="member-page-list management-invitation-receipt" aria-label="최근 초대">
          <div className="management-section-heading">
            <p role="status"><strong>{receipt.email}</strong> 초대 요청을 등록했습니다.</p>
            <button type="button" className="member-page-action" onClick={() => setReceipt(null)}>접수 내역 닫기</button>
          </div>
          {receiptMember?.ready ? (
            <p className="management-note">{receiptMember.suspended ? "계정이 정지되어 있습니다." : "계정이 준비되었습니다."}</p>
          ) : (
            <>
              <p className="management-note">설치 서버에서 계정을 준비한 뒤 임시 비밀번호를 회원에게 직접 전달하세요. <a href={memberGuide} target="_blank" rel="noreferrer">서버에서 계정 준비하기</a></p>
              <RequestNumber key={receipt.request_id} id={receipt.request_id} />
            </>
          )}
        </section>
      )}
      <section className="management-group" aria-labelledby="member-list-title">
        <div className="management-section-heading">
          <h2 id="member-list-title">회원 목록</h2>
          <button type="button" className="member-page-action" disabled={!!busy || !!pending || !!inviteId} onClick={() => void refresh()}>
            {busy === "list" ? "목록 확인 중…" : "목록 새로고침"}
          </button>
        </div>
        {busy === "list" && !members && <p className="management-row management-note" role="status">회원 목록을 불러오는 중…</p>}
        {members && !listCurrent && <p className="management-note" role="status">마지막으로 확인한 회원 목록입니다. 새로고침하면 권한과 상태를 변경할 수 있습니다.</p>}
        {members && members.length > 0 && (
          <div className="management-roster-controls">
            <label className="management-field">
              <span>회원 검색</span>
              <input type="search" placeholder="이름 또는 이메일" value={query} onChange={e => setQuery(e.target.value)} />
            </label>
            <div className="management-filters" role="group" aria-label="회원 상태 필터">
              {memberFilters.map(({ value, label }) => (
                <button type="button" key={value} className="member-page-action management-filter" aria-pressed={value === filter}
                  onClick={() => setFilter(value)}>
                  {label} <span>{members.filter(m => matchesFilter(m, value)).length}</span>
                </button>
              ))}
            </div>
            <p className="management-note" role="status">{visibleMembers?.length}명 표시</p>
          </div>
        )}
        {visibleMembers && visibleMembers.length > 0 ? (
          <ul className="member-page-list management-list">
            {visibleMembers.map(m => {
              const lastAdmin = m.admin && m.eligible && m.ready && !m.suspended && administrators === 1;
              const state = memberState(m);
              const roleChange = m.admin ? "demote" : "promote";
              return (
                <li className="management-person" key={m.account_id}>
                  <div className="management-person-header">
                    <div className="management-person-details">
                      <strong>{m.invitation_name || m.email}</strong>
                      {m.invitation_name && <span className="management-note">{m.email}</span>}
                      <span className="management-note">
                        {m.admin ? "관리자" : "일반 회원"}{m.account_id === account ? " · 내 계정" : ""}
                      </span>
                    </div>
                    <span className="management-status" data-state={state}>
                      {state === "suspended" ? "로그인 차단" : state === "ready" ? "로그인 가능"
                        : m.invitation_request_id ? "계정 준비 대기" : "계정 준비 확인 필요"}
                    </span>
                  </div>
                  {lastAdmin && <p className="management-note">현재 이용 가능한 유일한 관리자입니다.</p>}
                  {(m.ready || m.suspended || m.invitation_request_id) && (
                    <details className="management-person-disclosure" name="management-member-details">
                      <summary aria-label={`${m.email} ${m.ready || m.suspended ? "계정 관리" : "초대 확인"}`}>
                        {m.ready || m.suspended ? "계정 관리" : "초대 확인"}
                      </summary>
                      {m.invitation_request_id && !m.ready && (
                        <>
                          <RequestNumber key={m.invitation_request_id} id={m.invitation_request_id} />
                          <p className="management-note"><a href={memberGuide} target="_blank" rel="noreferrer">서버에서 계정 준비하기</a></p>
                        </>
                      )}
                      {m.ready && !m.suspended && !m.eligible && !m.admin && (
                        <p className="management-note">관리자로 지정하려면 회원이 먼저 로그인하고 임시 비밀번호를 변경해야 합니다.</p>
                      )}
                      {m.suspended ? (
                        <button className="member-page-action" disabled={controlsDisabled}
                          onClick={event => setConfirm({ member: m, action: "resume", trigger: event.currentTarget })}>
                          {actions.resume.label}
                        </button>
                      ) : m.ready && (
                        <div className="management-person-actions">
                          {(m.admin || m.eligible) && (
                            <button className="member-page-action" disabled={controlsDisabled || lastAdmin}
                              onClick={event => setConfirm({ member: m, action: roleChange, trigger: event.currentTarget })}>
                              {actions[roleChange].label}
                            </button>
                          )}
                          <button className="member-page-action management-danger" disabled={controlsDisabled || lastAdmin}
                            onClick={event => setConfirm({ member: m, action: "suspend", trigger: event.currentTarget })}>
                            {actions.suspend.label}
                          </button>
                        </div>
                      )}
                    </details>
                  )}
                </li>
              );
            })}
          </ul>
        ) : members && (
          <div className="member-page-list management-row management-empty">
            <p>{members.length ? "조건에 맞는 회원이 없습니다." : "등록된 회원이 없습니다."}</p>
            {members.length > 0 && <button type="button" className="member-page-action" onClick={() => { setQuery(""); setFilter("all"); }}>검색과 필터 초기화</button>}
          </div>
        )}
      </section>
      {confirm && <MemberChangeDialog {...confirm} onCancel={() => setConfirm(null)} onConfirm={() => change(confirm.member, confirm.action)} />}
    </section>
  );
}

function MemberChangeDialog({ member, action, trigger, onCancel, onConfirm }: {
  member: Member;
  action: Change;
  trigger: HTMLButtonElement;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  const policy = actions[action];
  useEffect(() => {
    const dialog = ref.current;
    dialog?.showModal();
    cancel.current?.focus();
    return () => {
      dialog?.close();
      if (trigger.disabled) trigger.closest("details")?.querySelector("summary")?.focus();
      else trigger.focus();
    };
  }, [trigger]);
  return (
    <dialog ref={ref} className="member-page-list management-dialog" aria-labelledby="member-change-title"
      aria-describedby="member-change-details" onCancel={event => { event.preventDefault(); onCancel(); }}>
      <h2 id="member-change-title">{policy.question}</h2>
      <p id="member-change-details"><strong>{member.invitation_name || member.email}</strong>{member.invitation_name && <small>{member.email}</small>}<span>{policy.detail}</span></p>
      <div className="management-footer">
        <button ref={cancel} type="button" className="member-page-action" onClick={onCancel}>취소</button>
        <button type="button" className="member-page-action management-primary" onClick={onConfirm}>{policy.label}</button>
      </div>
    </dialog>
  );
}

function RequestNumber({ id }: { id: string }) {
  const [message, setMessage] = useState("");
  async function copy() {
    try {
      await navigator.clipboard.writeText(id);
      setMessage("요청 번호를 복사했습니다.");
    } catch {
      setMessage("요청 번호를 선택해 직접 복사해 주세요.");
    }
  }
  return (
    <div className="management-request">
      <span className="management-note">서버에서 사용할 요청 번호</span>
      <code>{id}</code>
      <button className="member-page-action" type="button" onClick={() => void copy()}>요청 번호 복사</button>
      {message && <p role="status" className="management-note">{message}</p>}
    </div>
  );
}
