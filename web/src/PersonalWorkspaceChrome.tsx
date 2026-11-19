import { Button } from "@astryxdesign/core/Button";
import { Dialog } from "@astryxdesign/core/Dialog";
import { Icon } from "@astryxdesign/core/Icon";
import { useId, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import type { ThreadSummary } from "../shared/api-types";
import { WorkspaceIcon } from "./WorkspaceNavigation";
import { useWorkspaceViewport } from "./workspace-viewport";
import "./personal-workspace.css";

export type MemberView = "chat" | "memory" | "connections" | "admin";
export const threadDisplayTitle = (title: string) => title.trim() || "제목 없는 대화";
const destinations = {
  chat: { label: "대화", icon: <WorkspaceIcon name="bubble" /> },
  memory: { label: "기억", icon: <WorkspaceIcon name="notebook" /> },
  connections: { label: "연결", icon: <WorkspaceIcon name="link" /> },
  admin: { label: "관리", icon: <Icon icon="wrench" color="inherit" /> },
};

export function PersonalWorkspaceChrome({
  children, view, title, admin, threads, selected, loading, busy, user,
  onNavigate, onThread, onNew, onDelete, onSignOut,
}: {
  children: ReactNode;
  view: MemberView;
  title: string;
  admin: boolean;
  threads: ThreadSummary[];
  selected: string | null;
  loading: boolean;
  busy: boolean;
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
      <header className="member-topbar">
        <button type="button" className="member-switcher" aria-label="대화 목록"
          aria-haspopup="dialog" aria-expanded={drawer} aria-controls={drawer ? drawerId : undefined}
          onClick={event => { event.currentTarget.focus({ preventScroll: true }); setDrawer(true); }}>
          <Icon icon="menu" size="sm" color="inherit" /><span className="member-switcher-name">Moosuri</span>
        </button>
        {view === "chat" && <>
          <h1 className="member-chat-heading" title={title}>
            <img className="member-chat-avatar" src="/moosuri-icon-192.png" width="28" height="28" alt="" />
            <span>{title}</span>
          </h1>
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
            <h2>대화</h2>
            <Button className="member-header-button" variant="ghost" label="닫기" isIconOnly
              icon={<Icon icon="close" color="inherit" />} onClick={() => toggleDrawer(false)} />
          </header>
          <div className="member-drawer-tools">
            <label className="member-thread-search"><WorkspaceIcon name="search" />
              <input ref={searchInput} type="search" disabled={!!deletion} value={query} onChange={event => setQuery(event.target.value)}
                placeholder="대화 검색" aria-label="대화 검색" />
            </label>
            <Button className="member-drawer-new" label="새 대화" icon={<WorkspaceIcon name="new" />}
              variant="primary" width="100%" isDisabled={loading || busy || !!deletion} onClick={newChat} />
          </div>
          <div className="member-drawer-scroll">
            {deletion && (
              <section className="member-thread-confirmation" aria-labelledby={confirmationId}>
                <h3 id={confirmationId}>대화를 삭제할까요?</h3>
                <p>{threadDisplayTitle(deletion.thread.title)}</p>
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
            <h3 className="member-drawer-label" hidden={!!deletion}>{search ? "검색 결과" : "최근 대화"}</h3>
            <nav aria-label="대화 목록" className="member-thread-list" hidden={!!deletion}>
              {loading && <p role="status">대화 불러오는 중…</p>}
              {!loading && visibleThreads.length === 0 && <p role="status">{search ? "일치하는 대화가 없습니다." : "아직 대화가 없습니다."}</p>}
              {visibleThreads.map((thread) => {
                const isCurrent = view === "chat" && selected === thread.id;
                const title = threadDisplayTitle(thread.title);
                return (
                  <div className="member-thread-row" key={thread.id}>
                    <Button label={title} variant="ghost" width="100%"
                      className="member-thread-button" icon={<WorkspaceIcon name="bubble" />} aria-current={isCurrent ? "page" : undefined}
                      onClick={() => { toggleDrawer(false); onThread(thread.id); }} />
                    <button type="button" className="member-thread-delete" aria-label={`${title} 삭제`}
                      disabled={loading || busy} onClick={event => {
                        refocusDelete.current = event.currentTarget;
                        setDeletion({ thread, failed: false });
                      }}>삭제</button>
                  </div>
                );
              })}
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
