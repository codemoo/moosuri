import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import { PersonalMemories } from "./PersonalMemories";
import { emptyMemory } from "./KnowledgeState";
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

it("filters, reads, edits, and restores focus to a reopened memory row", async () => {
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
  await user.type(filter, "missing");
  expect(screen.getByText("현재 목록에 일치하는 기억이 없습니다.")).toBeVisible();
  await user.clear(filter);
  await user.type(filter, "synthetic AUTHOR");
  const filteredRow = screen.getByRole("button", { name: /Morning tea/ });
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
  await user.click(screen.getByRole("button", { name: "기억 닫기" }));
  expect(screen.getByRole("button", { name: /Morning tea set/ })).toHaveFocus();
  expect(fetch.mock.calls.every(([url]) => url.includes("/knowledge/memories"))).toBe(true);
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
