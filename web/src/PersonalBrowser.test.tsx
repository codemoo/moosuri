import { fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { PersonalBrowserCard, PersonalBrowserViewer } from "./PersonalBrowser";
import type { PersonalBrowserWorkspace } from "./personal-browser";
import type { PersonalBrowser } from "./usePersonalBrowser";

function workspace(over: Partial<PersonalBrowserWorkspace> = {}): PersonalBrowserWorkspace {
  return {
    id: "w1", request_id: "r1", run_id: "run1", thread_id: "t1", state: "ready", revision: 4,
    url: "https://example.org/b", title: "Timetable", tabs: [], active_tab_id: "tab1",
    reason: "browser_no_progress", links: [{ url: "https://example.org/c", text: "Platform" }],
    ...over,
  };
}
function fake(w: PersonalBrowserWorkspace | null, mutate = vi.fn()): PersonalBrowser {
  return {
    status: { configured: true, workspace: w }, frame: null, preview: null, controls: false,
    canUseFrame: false, busy: null, error: "", canRetryResume: false, refresh: vi.fn(), mutate,
    publish: vi.fn(),
  } as unknown as PersonalBrowser;
}

it("explains a no-progress pause and offers only the existing read-only ways to continue", () => {
  const mutate = vi.fn();
  render(<PersonalBrowserViewer browser={fake(workspace(), mutate)} onClose={() => {}} />);
  expect(document.querySelector("[data-reason='browser_no_progress']")?.textContent)
    .toContain("일시 중지");
  // The generic "start control" hint is replaced; control, links and refresh remain.
  expect(screen.queryByText("조건을 바꾸거나 입력하려면 직접 조작을 시작하세요.")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "직접 조작으로 이어가기" }));
  expect(mutate).toHaveBeenLastCalledWith({ kind: "takeover" });
  fireEvent.click(screen.getByRole("button", { name: "Platform" }));
  expect(mutate).toHaveBeenLastCalledWith({ kind: "tabs", action: "follow", url: "https://example.org/c" });
  expect(screen.getByRole("button", { name: "화면 새로고침" })).toBeTruthy();
});

it("shows the pause on the card only while the task is paused and not for other reasons", () => {
  const { rerender } = render(
    <PersonalBrowserCard browser={fake(workspace())} pending={false} onOpen={() => {}} />,
  );
  expect(screen.getByText("진행이 없어 일시 중지됨")).toBeTruthy();
  rerender(<PersonalBrowserCard browser={fake(workspace({ reason: "new_guidance" }))} pending={false} onOpen={() => {}} />);
  expect(screen.queryByText("진행이 없어 일시 중지됨")).toBeNull();
  rerender(<PersonalBrowserCard browser={fake(workspace({ state: "user_control" }))} pending={false} onOpen={() => {}} />);
  expect(screen.queryByText("진행이 없어 일시 중지됨")).toBeNull();
});

it("does not keep a stale pause message once the task is running or controlled again", () => {
  render(<PersonalBrowserViewer browser={fake(workspace({ state: "resuming" }))} onClose={() => {}} />);
  expect(document.querySelector("[data-reason='browser_no_progress']")).toBeNull();
});
