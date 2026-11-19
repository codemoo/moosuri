import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, expect, it, vi } from "vitest";
import { PersonalMemories } from "./PersonalMemories";
import { emptyMemory, initialKnowledgeDrafts } from "./KnowledgeState";
import type { MemoryDocument } from "../shared/api-types";

const memory: MemoryDocument = {
  id: "test-memory",
  revision: 1,
  status: "active",
  draft: { ...emptyMemory(), subject: "Morning tea", text: "Assam", speaker: "Synthetic author" },
  created_at: 100,
  updated_at: 100,
};
afterEach(() => vi.unstubAllGlobals());

it("searches by speaker, reads, edits, and restores focus to a reopened memory row", async () => {
  let current = memory;
  const fetch = vi.fn(async (url: string, init: RequestInit = {}) => {
    if (init.method === "PUT") {
      const body = JSON.parse(String(init.body));
      expect(body.expected_revision).toBe(1);
      current = { ...current, revision: 2, draft: body.draft };
    }
    return Response.json(url.endsWith("/memories") ? { items: [current], next_after: null } : current);
  });
  vi.stubGlobal("fetch", fetch);
  const user = userEvent.setup();
  render(<PersonalMemories accountId="test-member" onExpired={vi.fn()} />);
  await screen.findByRole("button", { name: /Morning tea/ });
  const filter = screen.getByRole("searchbox");
  await user.type(filter, "missing{enter}");
  expect(await screen.findByText("일치하는 기억이 없습니다.")).toBeVisible();
  await user.clear(filter);
  await user.type(filter, "synthetic AUTHOR{enter}");
  const filteredRow = await screen.findByRole("button", { name: /Morning tea/ });
  await user.click(filteredRow);
  const detail = screen.getByRole("dialog", { name: "기억 읽기" });
  await user.click(await within(detail).findByRole("button", { name: "기억 수정" }));
  const editor = screen.getByRole("dialog", { name: "기억 작성" });
  const title = within(editor).getByLabelText("기억 주제");
  expect(title).toHaveFocus();
  await user.type(title, " set");
  await user.click(within(editor).getByRole("button", { name: "기억 저장" }));
  const updated = await within(screen.getByRole("dialog", { name: "기억 읽기" })).findByRole("heading", { name: "Morning tea set" });
  expect(updated).toBeVisible();
  await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "닫기" }));
  expect(screen.getByRole("button", { name: /Morning tea set/ })).toHaveFocus();
  expect(fetch.mock.calls.every(([url]) => url.includes("/knowledge/memories"))).toBe(true);
});

it("searches other pages in bounded batches and clears back to the full list", async () => {
  const fetch = vi.fn(async (url: string) => {
    const page = Number(new URL(url, "https://example.test").searchParams.get("after"));
    return Response.json({
      items: [page === 10 ? { ...memory, id: "last", draft: { ...memory.draft, subject: "후속 페이지" } } : memory],
      next_after: page < 10 ? String(page + 1) : null,
    });
  });
  vi.stubGlobal("fetch", fetch);
  const user = userEvent.setup();
  render(<PersonalMemories accountId="test-member" onExpired={vi.fn()} />);
  await screen.findByRole("button", { name: /Morning tea/ });
  await user.type(screen.getByRole("searchbox"), "후속");
  expect(fetch).toHaveBeenCalledTimes(1);
  await user.click(screen.getByRole("button", { name: "검색" }));
  expect(await screen.findByText("이 구간에 일치하는 기억이 없습니다. 다음 구간을 검색할 수 있습니다.")).toBeVisible();
  expect(fetch).toHaveBeenCalledTimes(11);
  await user.click(screen.getByRole("button", { name: "검색 계속" }));
  expect(await screen.findByRole("button", { name: /후속 페이지/ })).toBeVisible();
  await user.click(screen.getByRole("button", { name: "검색 지우기" }));
  expect(await screen.findByRole("button", { name: /Morning tea/ })).toBeVisible();
  expect(screen.getByRole("searchbox")).toHaveFocus();
  expect(fetch.mock.calls.every(([url]) => url.includes("/knowledge/memories"))).toBe(true);
});

it("distinguishes a failed list read from an empty list and retries it", async () => {
  vi.stubGlobal("fetch", vi.fn()
    .mockResolvedValueOnce(new Response(null, { status: 503 }))
    .mockResolvedValueOnce(Response.json({ items: [], next_after: null })));
  const user = userEvent.setup();
  render(<PersonalMemories accountId="test-member" onExpired={vi.fn()} />);
  expect(await screen.findByRole("alert")).toHaveTextContent("기억 목록을 불러오지 못했습니다.");
  expect(screen.queryByText("아직 기록한 기억이 없습니다.")).not.toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "다시 시도" }));
  expect(await screen.findByText("아직 기록한 기억이 없습니다.")).toBeVisible();
});

it("reloads after a stale deletion and requires another confirmation for the new revision", async () => {
  const updated = { ...memory, revision: 2, draft: { ...memory.draft, text: "Ceylon" } };
  const deletedRevisions: number[] = [];
  let current: MemoryDocument | null = memory;
  vi.stubGlobal("fetch", vi.fn(async (url: string, init: RequestInit) => {
    if (init.method === "DELETE") {
      deletedRevisions.push(JSON.parse(String(init.body)).expected_revision);
      if (deletedRevisions.length === 1) {
        current = updated;
        return new Response(null, { status: 409 });
      }
      current = null;
      return new Response(null, { status: 204 });
    }
    return Response.json(url.endsWith("/memories")
      ? { items: current ? [current] : [], next_after: null }
      : current);
  }));
  const user = userEvent.setup();
  render(<PersonalMemories accountId="test-member" onExpired={vi.fn()} />);
  async function confirmForget(text: string) {
    await user.click(await screen.findByRole("button", { name: new RegExp(text) }));
    const reader = screen.getByRole("dialog", { name: "기억 읽기" });
    await user.click(await within(reader).findByRole("button", { name: "기억 잊기" }));
    const confirm = screen.getByRole("dialog", { name: "기억 잊기" });
    await user.click(within(confirm).getByRole("button", { name: "기억과 이력 지우기" }));
  }
  await confirmForget("Assam");
  expect(await screen.findByText("기억이 다른 곳에서 변경되었습니다. 목록에서 최신 내용을 확인해 주세요.")).toBeVisible();
  expect(deletedRevisions).toEqual([1]);
  await confirmForget("Ceylon");
  expect(await screen.findByText("아직 기록한 기억이 없습니다.")).toBeVisible();
  expect(deletedRevisions).toEqual([1, 2]);
});

it("keeps an uncertain save request across views and retries with the same request ID", async () => {
  const writes: string[] = [];
  let saved = false;
  vi.stubGlobal("fetch", vi.fn(async (url: string, init: RequestInit) => {
    if (init.method === "POST") {
      writes.push(String(init.body));
      if (writes.length === 1) return new Response(null, { status: 503 });
      saved = true;
      return Response.json(memory);
    }
    return Response.json(url.endsWith("/memories")
      ? { items: saved ? [memory] : [], next_after: null }
      : memory);
  }));
  function Workspace() {
    const [showMemory, setShowMemory] = useState(true);
    const [drafts, updateDrafts] = useState(initialKnowledgeDrafts);
    return <>
      <button onClick={() => setShowMemory((value) => !value)}>화면 바꾸기</button>
      {showMemory && <PersonalMemories accountId="test-member" onExpired={vi.fn()} drafts={drafts} updateDrafts={updateDrafts} />}
    </>;
  }
  const user = userEvent.setup();
  render(<Workspace />);
  await screen.findByText("아직 기록한 기억이 없습니다.");
  await user.click(screen.getByRole("button", { name: "기억 추가" }));
  await user.type(screen.getByLabelText("기억 주제"), memory.draft.subject);
  await user.type(screen.getByLabelText("기억 내용"), memory.draft.text);
  await user.click(screen.getByRole("button", { name: "기억 저장" }));
  await screen.findByRole("button", { name: "같은 기억 요청 다시 확인" });
  await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: "닫기" }));
  const navigation = screen.getByRole("button", { name: "화면 바꾸기" });
  await user.click(navigation);
  await user.click(navigation);
  await user.click(screen.getByRole("button", { name: "기억 작성 이어가기" }));
  expect(screen.getByLabelText("기억 주제")).toHaveValue(memory.draft.subject);
  await user.click(screen.getByRole("button", { name: "같은 기억 요청 다시 확인" }));
  await waitFor(() => expect(writes).toHaveLength(2));
  expect(writes[0]).toBe(writes[1]);
  expect(await screen.findByRole("heading", { name: memory.draft.subject })).toBeVisible();
});

it("resumes the add drawer after Escape without losing its draft", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => Response.json({ items: [], next_after: null })));
  const user = userEvent.setup();
  render(<PersonalMemories accountId="test-member" onExpired={vi.fn()} />);
  await screen.findByText("아직 기록한 기억이 없습니다.");
  await user.click(screen.getByRole("button", { name: "기억 추가" }));
  const drawer = screen.getByRole("dialog", { name: "기억 작성" });
  await user.type(within(drawer).getByLabelText("기억 주제"), "A new note");
  fireEvent(drawer, new Event("cancel", { bubbles: true, cancelable: true }));
  expect(screen.queryByRole("dialog")).toBeNull();
  const resume = screen.getByRole("button", { name: "기억 작성 이어가기" });
  expect(resume).toHaveFocus();
  await user.click(resume);
  expect(screen.getByLabelText("기억 주제")).toHaveValue("A new note");
});
