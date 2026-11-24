import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import { MemberMemoryReceipt, type MemberMemoryReceiptView } from "./MemberMemoryReceipt";

afterEach(cleanup);

it("shows the live text after a correction and says so when a later revision replaced the one acted on", () => {
  const view: MemberMemoryReceiptView = {
    kind: "remembered", operation: "create", memory_id: "m1", revision: 1, current_revision: 2,
    subject: "식사 취향", text: "갑각류만 안 된다", superseded: true,
  };
  render(<MemberMemoryReceipt view={view} />);
  expect(screen.getByText("갑각류만 안 된다")).toBeVisible();
  expect(screen.getByText("이후 더 최근에 고친 내용이에요.")).toBeVisible();
  expect(screen.queryByRole("button")).toBeNull();
});

it("shows no memory text after a forget and offers the list only when asked", async () => {
  const open = vi.fn();
  render(<MemberMemoryReceipt view={{ kind: "withdrawn", memory_id: "m1" }} onOpenMemories={open} />);
  expect(screen.getByText(/이 기억은 잊었어요/)).toBeVisible();
  expect(screen.queryByText("해산물 알레르기")).toBeNull();
  await userEvent.setup().click(screen.getByRole("button", { name: "기억 목록 보기" }));
  expect(open).toHaveBeenCalledOnce();
});

it("explains a refusal and lists ambiguous candidates by name only", () => {
  render(<MemberMemoryReceipt view={{
    kind: "no_change", reason: "ambiguous",
    candidates: [{ memory_id: "a", revision: 1, subject: "여행 숙소" }, { memory_id: "b", revision: 1, subject: "여행 교통" }],
  }} />);
  expect(screen.getByText(/하나로 정할 수 없어요/)).toBeVisible();
  expect(screen.getAllByRole("listitem").map(li => li.textContent)).toEqual(["여행 숙소", "여행 교통"]);
});

it("reports a single-trip exception as unchanged and an unknown reason without claiming a change", () => {
  const { rerender } = render(<MemberMemoryReceipt view={{ kind: "no_change", reason: "one_time", candidates: [] }} />);
  expect(screen.getByText(/이번 한 번만/)).toBeVisible();
  rerender(<MemberMemoryReceipt view={{ kind: "no_change", reason: "future_reason", candidates: [] }} />);
  expect(screen.getByText("아무것도 바꾸지 않았어요.")).toBeVisible();
  rerender(<MemberMemoryReceipt view={{ kind: "stale" }} />);
  expect(screen.getByText(/그 사이 기억이 바뀌었어요/)).toBeVisible();
});
