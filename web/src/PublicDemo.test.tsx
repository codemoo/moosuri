import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import { PublicDemo } from "./PublicDemo";

afterEach(() => vi.unstubAllGlobals());

it("opens the public timetable example without requesting a member session or private data", () => {
  const fetcher = vi.fn();
  vi.stubGlobal("fetch", fetcher);
  render(<PublicDemo />);
  expect(screen.getByText("오후 3시 40분, 7번 승강장에서 출발해요.")).toBeVisible();
  expect(screen.getByRole("link", { name: "회원 공간 열기" })).toHaveAttribute("href", "/app");
  expect(fetcher).not.toHaveBeenCalled();
});

it("keeps a practice question out of persistent storage and clears it on leaving the public demo", async () => {
  const localWrite = vi.spyOn(Storage.prototype, "setItem");
  const fetcher = vi.fn();
  vi.stubGlobal("fetch", fetcher);
  const user = userEvent.setup();
  const { unmount } = render(<PublicDemo />);
  await user.type(screen.getByRole("textbox"), "example question");
  expect(screen.getByRole("textbox")).toHaveValue("example question");
  unmount();
  render(<PublicDemo />);
  expect(screen.getByRole("textbox")).toHaveValue("");
  expect(localWrite).not.toHaveBeenCalled();
  expect(fetcher).not.toHaveBeenCalled();
});

it("shows only the selected synthetic conversation and its fixed evidence in the public sample", async () => {
  const user = userEvent.setup();
  render(<PublicDemo />);
  await user.click(screen.getByRole("button", { name: "기억 정정" }));
  expect(screen.getByText("예시 기억 · 보리차 → 현미차")).toBeVisible();
  await user.click(screen.getByRole("button", { name: "맡긴 일" }));
  expect(screen.getByText("예시 작업 기록 · 요청 → 시간표 확인 → 완료")).toBeVisible();
  expect(screen.queryByText("예시 기억 · 보리차 → 현미차")).toBeNull();
  expect(screen.getByRole("button", { name: "맡긴 일" })).toHaveAttribute("aria-pressed", "true");
});
