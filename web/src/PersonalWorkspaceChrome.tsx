import { AppShell } from "@astryxdesign/core/AppShell";
import { Button } from "@astryxdesign/core/Button";
import { Dialog } from "@astryxdesign/core/Dialog";
import { Icon } from "@astryxdesign/core/Icon";
import { SideNav, SideNavItem } from "@astryxdesign/core/SideNav";
import { TopNav } from "@astryxdesign/core/TopNav";
import { useId, useState, type ReactNode } from "react";
import type { ThreadSummary } from "../shared/api-types";
import { WorkspaceIcon } from "./WorkspaceNavigation";
import { useWorkspaceViewport } from "./workspace-viewport";
import "./personal-workspace.css";

export type MemberView = "chat" | "memory" | "mail" | "admin";
const destinations = {
  chat: { label: "대화", icon: <WorkspaceIcon name="chat" /> },
  memory: { label: "기억", icon: <Icon icon="viewColumns" color="inherit" /> },
  mail: { label: "메일", icon: <WorkspaceIcon name="feed" /> },
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
  onDelete: () => void;
  onSignOut: () => void;
}) {
  const workspace = useWorkspaceViewport();
  const drawerId = useId();
  const [drawer, setDrawer] = useState(false);
  const views = Object.keys(destinations) as MemberView[];

  function navigate(destination: MemberView) {
    setDrawer(false);
    onNavigate(destination);
  }
  function newChat() {
    setDrawer(false);
    onNew();
  }
  function navigation() {
    return views.filter((destination) => destination !== "admin" || admin).map((destination) => (
      <SideNavItem key={destination} className="workspace-rail-item"
        label={destinations[destination].label}
        icon={destinations[destination].icon}
        isSelected={view === destination}
        onClick={() => navigate(destination)} />
    ));
  }

  return (
    <div className="workspace member-workspace" ref={workspace}>
      <AppShell className="workspace-shell" variant="wash" height="fill" contentPadding={0}
        mobileNav={{hasToggle: false, isOpen: false}}
        sideNav={
          <SideNav className="workspace-rail" aria-label="주 메뉴">
            <div className="workspace-rail-items">{navigation()}</div>
          </SideNav>
        }
        topNav={
          <TopNav className="work-header" label="작업 공간"
            heading={
              <div className="work-title">
                <Button variant="ghost" className="workspace-account-toggle"
                  label="대화 및 설정" isIconOnly aria-haspopup="dialog" aria-expanded={drawer}
                  aria-controls={drawer ? drawerId : undefined}
                  icon={<img src="/moosuri-icon-192.png" width="32" height="32" alt="" />}
                  onClick={(event) => {
                    event.currentTarget.focus({ preventScroll: true });
                    setDrawer(true);
                  }} />
                <h1 title={title}>{title}</h1>
              </div>
            }
            endContent={
              <div className="work-header-actions">
                {view === "chat" && (
                  <Button variant="ghost" className="member-header-button" label="새 대화" isIconOnly
                    tooltip="새 대화" icon={<WorkspaceIcon name="new" />}
                    isDisabled={loading || busy} onClick={newChat} />
                )}
              </div>
            }
          />
        }
      >
        {children}
      </AppShell>
      <Dialog isOpen={drawer} id={drawerId} aria-label="대화 및 설정" padding={0}
        className="member-drawer" width={340} maxHeight="100%"
        position={{start: 0, top: 0, bottom: 0}} onOpenChange={setDrawer}>
        <div className="member-drawer-layout">
          <header className="member-drawer-header">
            <h2><img src="/moosuri-icon-192.png" width="32" height="32" alt="" />Moosuri</h2>
            <Button className="member-header-button" variant="ghost" label="닫기" isIconOnly
              icon={<Icon icon="close" color="inherit" />} onClick={() => setDrawer(false)} />
          </header>
          <div className="member-drawer-scroll">
            <Button className="member-drawer-new" label="새 대화" icon={<WorkspaceIcon name="new" />}
              variant="primary" width="100%" isDisabled={loading || busy} onClick={newChat} />
            <SideNav className="member-mobile-nav" aria-label="주 메뉴">{navigation()}</SideNav>
            <h3 className="member-drawer-label">최근 대화</h3>
            <nav aria-label="대화 목록" className="member-thread-list">
              {loading && <p role="status">대화 불러오는 중…</p>}
              {!loading && threads.length === 0 && <p>아직 대화가 없습니다.</p>}
              {threads.map((thread) => {
                const isCurrent = view === "chat" && selected === thread.id;
                return (
                  <div className="member-thread-row" key={thread.id}>
                    <Button label={thread.title} variant="ghost" width="100%"
                      className="member-thread-button" aria-current={isCurrent ? "page" : undefined}
                      onClick={() => { setDrawer(false); onThread(thread.id); }} />
                    {isCurrent && (
                      <Button variant="ghost" className="member-header-button" label="대화 삭제" isIconOnly
                        tooltip="대화 삭제" icon={<Icon icon="close" color="inherit" />}
                        isDisabled={busy} onClick={() => { setDrawer(false); onDelete(); }} />
                    )}
                  </div>
                );
              })}
            </nav>
          </div>
          <footer className="member-drawer-account">
            <div><strong>{user.name}</strong><small>{user.email}</small></div>
            <Button variant="ghost" label="로그아웃" onClick={onSignOut} />
          </footer>
        </div>
      </Dialog>
    </div>
  );
}
