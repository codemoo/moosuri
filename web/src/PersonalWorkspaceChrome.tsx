import { Button } from "@astryxdesign/core/Button";
import { Dialog } from "@astryxdesign/core/Dialog";
import { Icon } from "@astryxdesign/core/Icon";
import { useId, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import type { ThreadSummary } from "../shared/api-types";
import { WorkspaceIcon } from "./WorkspaceNavigation";
import { useWorkspaceViewport } from "./workspace-viewport";
import "./personal-workspace.css";
import "./personal-conversations.css";

export type MemberView = "chat" | "memory" | "connections" | "admin";
export const threadDisplayTitle = (title: string) => title.trim() || "제목 없는 대화";
const threadMonth = new Intl.DateTimeFormat("ko-KR", { year: "numeric", month: "long" });
const threadDay = new Intl.DateTimeFormat("ko-KR", { month: "long", day: "numeric" });
function historyGroup(thread: ThreadSummary, today: Date, yesterday: Date) {
  const updated = new Date(thread.updated_at * 1000);
  if (!Number.isFinite(updated.getTime())) return "날짜 미상";
  if (updated >= today) return "오늘";
  if (updated >= yesterday) return "어제";
  return threadMonth.format(updated);
}
const destinations = {
  chat: { label: "대화", icon: <WorkspaceIcon name="bubble" /> },
  memory: { label: "기억", icon: <WorkspaceIcon name="notebook" /> },
  connections: { label: "연결", icon: <WorkspaceIcon name="link" /> },
  admin: { label: "관리", icon: <Icon icon="wrench" color="inherit" /> },
};

export function PersonalWorkspaceChrome({
  children, view, title, admin, threads, selected, loading, busy, user,
  drafts, inventoryError, chatActivity, onRefreshThreads, onNavigate, onThread, onNew, onDelete, onSignOut,
}: {
  children: ReactNode;
  view: MemberView;
  title: string;
  admin: boolean;
  threads: ThreadSummary[];
  selected: string | null;
  loading: boolean;
  busy: boolean;
  drafts: Readonly<Record<string, string>>;
  inventoryError: boolean;
  chatActivity?: string;
  onRefreshThreads: () => void;
  user: { name: string; email: string };
  onNavigate: (view: MemberView) => void;
  onThread: (id: string) => void;
  onNew: () => void;
  onDelete: (id: string) => Promise<boolean>;
  onSignOut: () => void;
}) {
  const workspace = useWorkspaceViewport();
  const drawerId = useId();
  const confirmationId = useId();
  const [drawer, setDrawer] = useState(false);
  const [query, setQuery] = useState("");
  const [deletion, setDeletion] = useState<{ thread: ThreadSummary; failed: boolean } | null>(null);
  const searchInput = useRef<HTMLInputElement>(null);
  const cancelButton = useRef<HTMLButtonElement>(null);
  const refocusDelete = useRef<HTMLElement | null>(null);
  const views = Object.keys(destinations) as MemberView[];
  const search = query.trim().normalize("NFKC").toLocaleLowerCase();
  const visibleThreads = threads.filter(thread => threadDisplayTitle(thread.title).normalize("NFKC").toLocaleLowerCase().includes(search));
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);
  const groups = new Map<string, ThreadSummary[]>();
  if (drawer) for (const thread of visibleThreads) {
    const label = search ? "검색 결과" : historyGroup(thread, today, yesterday);
    const group = groups.get(label);
    if (group) group.push(thread);
    else groups.set(label, [thread]);
  }
  function toggleDrawer(open: boolean) {
    setDrawer(open);
    if (!open) {
      setQuery("");
      setDeletion(null);
      refocusDelete.current = null;
    }
  }

  useLayoutEffect(() => {
    if (!drawer) return;
    if (deletion) cancelButton.current?.focus({ preventScroll: true });
    else if (refocusDelete.current) {
      const target = refocusDelete.current.isConnected ? refocusDelete.current : searchInput.current;
      target?.focus({ preventScroll: true });
      refocusDelete.current = null;
    }
  }, [drawer, deletion]);

  async function confirmDeletion() {
    if (!deletion || busy) return;
    const removed = await onDelete(deletion.thread.id);
    setDeletion(current => current !== deletion ? current : removed ? null : { ...current, failed: true });
  }

  function navigate(destination: MemberView) {
    toggleDrawer(false);
    onNavigate(destination);
  }
  function newChat() {
    toggleDrawer(false);
    onNew();
  }
  return (
    <div className="member-workspace" data-view={view} ref={workspace}>
      <div className="member-rail">
        <div className="member-rail-brand"><img src="/moosuri-icon-192.png" alt="" width="36" height="36" /><span>Moosuri</span></div>
        <nav className="member-navigation" aria-label="주 메뉴">
        {views.filter(destination => destination !== "admin" || admin).map(destination => (
          <button key={destination} type="button" aria-label={destinations[destination].label}
            title={destinations[destination].label} aria-current={view === destination ? "page" : undefined}
            onClick={() => navigate(destination)}>
            {destinations[destination].icon}
            <span className="member-nav-label" aria-hidden="true">{destinations[destination].label}</span>
          </button>
        ))}
        </nav>
        <div className="member-rail-account">
          <span className="member-account-avatar" aria-hidden="true">{Array.from(user.name.trim())[0] || "M"}</span>
          <div><strong>{user.name}</strong><small>{admin ? "설치 관리자" : "회원"}</small></div>
        </div>
      </div>
      <header className="member-topbar">
        <button type="button" className="member-switcher" aria-label="대화 목록"
          aria-haspopup="dialog" aria-expanded={drawer} aria-controls={drawer ? drawerId : undefined}
          onClick={event => { event.currentTarget.focus({ preventScroll: true }); setDrawer(true); }}>
          <Icon icon="menu" size="sm" color="inherit" /><span className="member-switcher-name">대화 목록</span>
        </button>
        {view === "chat" && <>
          <div className="member-chat-title">
            <h1 className="member-chat-heading" title={title}>
              <img className="member-chat-avatar" src="/moosuri-icon-192.png" width="28" height="28" alt="" />
              <span>{title}</span>
            </h1>
            {chatActivity && <p className="member-chat-activity" role="status">{chatActivity}</p>}
          </div>
          <button type="button" className="member-add-chat" aria-label="새 대화" title="새 대화"
            disabled={loading || busy} onClick={newChat}><WorkspaceIcon name="new" /></button>
        </>}
      </header>
      <main className="member-body">{children}</main>
      <Dialog isOpen={drawer} id={drawerId} aria-label="대화 목록" padding={0}
        className="member-drawer" width={360} maxHeight="100%"
        position={{start: 0, top: 0, bottom: 0}} onOpenChange={toggleDrawer}>
        <div className="member-drawer-layout">
          <header className="member-drawer-header">
            <h2>대화 기록</h2>
            <Button className="member-header-button" variant="ghost" label="닫기" isIconOnly
              icon={<Icon icon="close" color="inherit" />} onClick={() => toggleDrawer(false)} />
          </header>
          <div className="member-drawer-tools">
            <label className="member-thread-search"><WorkspaceIcon name="search" />
              <input ref={searchInput} type="search" disabled={!!deletion} value={query} onChange={event => setQuery(event.target.value)}
                placeholder="대화 제목 검색" aria-label="대화 검색"
                onKeyDown={(event) => {
                  if (event.key !== "Enter" || event.nativeEvent.isComposing || event.nativeEvent.keyCode === 229 ||
                      !search || visibleThreads.length !== 1) return;
                  event.preventDefault();
                  toggleDrawer(false);
                  onThread(visibleThreads[0].id);
                }} />
            </label>
            <Button className="member-drawer-new" label="새 대화" icon={<WorkspaceIcon name="new" />}
              variant="primary" width="100%" isDisabled={loading || busy || !!deletion} onClick={newChat} />
          </div>
          <div className="member-drawer-scroll">
            {deletion && (
              <section className="member-thread-confirmation" aria-labelledby={confirmationId}>
                <h3 id={confirmationId}>대화를 삭제할까요?</h3>
                <p>{threadDisplayTitle(deletion.thread.title)}</p>
                <p>이 대화의 메시지와 저장된 대화 기록을 지웁니다.</p>
                {deletion.failed && <p role="alert">대화 삭제 결과를 확인하지 못했습니다. 다시 시도해 주세요.</p>}
                {busy && <p role="status">대화 삭제 중…</p>}
                <div>
                  <button ref={cancelButton} type="button" disabled={busy}
                    onClick={() => setDeletion(null)}>취소</button>
                  <button type="button" className="member-thread-confirm-delete"
                    disabled={busy} onClick={() => void confirmDeletion()}>대화 삭제</button>
                </div>
              </section>
            )}
            <nav aria-label="대화 목록" className="member-thread-list" hidden={!!deletion}>
              {loading && <p role="status">대화 불러오는 중…</p>}
              {inventoryError && <div className="member-history-error">
                <p role="alert">대화 목록을 불러오지 못했습니다.</p>
                <Button label="다시 시도" variant="ghost" onClick={onRefreshThreads} />
              </div>}
              {!loading && !inventoryError && visibleThreads.length === 0 && <p role="status">
                {search ? "일치하는 대화가 없습니다." : "아직 대화가 없습니다."}
              </p>}
              {Array.from(groups, ([label, group]) => (
                <section key={label} className="member-history-group">
                  <h3 className="member-drawer-label">{label}</h3>
                  {group.map((thread) => {
                    const isCurrent = view === "chat" && selected === thread.id;
                    const title = threadDisplayTitle(thread.title);
                    const hasDraft = !!drafts[thread.id]?.trim();
                    const updated = new Date(thread.updated_at * 1000);
                    return (
                      <div className="member-thread-row" key={thread.id}>
                        <button type="button" className="member-thread-button" aria-label={title}
                          aria-description={hasDraft ? "작성 중인 메시지 있음" : undefined}
                          aria-current={isCurrent ? "page" : undefined}
                          onClick={() => { toggleDrawer(false); onThread(thread.id); }}>
                          <WorkspaceIcon name="bubble" />
                          <span className="member-history-title" title={title}>{title}</span>
                          <span className="member-history-meta" aria-hidden="true">
                            {hasDraft ? "초안" : search && Number.isFinite(updated.getTime()) ? threadDay.format(updated) : null}
                          </span>
                        </button>
                        <button type="button" className="member-thread-delete" aria-label={`${title} 삭제`}
                          disabled={loading || busy} onClick={event => {
                            refocusDelete.current = event.currentTarget;
                            setDeletion({ thread, failed: false });
                          }}>삭제</button>
                      </div>
                    );
                  })}
                </section>
              ))}
            </nav>
          </div>
          <footer className="member-drawer-account">
            <span className="member-account-avatar" aria-hidden="true">{Array.from(user.name.trim())[0] || "M"}</span>
            <div><strong>{user.name}</strong><small>{user.email}</small></div>
            <Button variant="ghost" label="로그아웃" onClick={onSignOut} />
          </footer>
        </div>
      </Dialog>
    </div>
  );
}
