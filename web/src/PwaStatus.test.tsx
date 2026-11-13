import { act, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { PwaStatus } from "./PwaStatus";

it("announces connection loss without replacing mounted drafts and clears on reconnect", () => {
  const online = vi.spyOn(navigator, "onLine", "get").mockReturnValue(true);
  const view = render(<><textarea aria-label="draft" defaultValue="보내지 않은 개인 초안" /><PwaStatus /></>);
  expect(screen.queryByRole("status")).toBeNull();
  online.mockReturnValue(false);
  act(() => { window.dispatchEvent(new Event("offline")); });
  expect(screen.getByRole("status")).toHaveTextContent("인터넷 연결이 끊겼어요");
  expect(screen.getByRole("textbox", { name: "draft" })).toHaveValue("보내지 않은 개인 초안");
  online.mockReturnValue(true);
  act(() => { window.dispatchEvent(new Event("online")); });
  expect(screen.queryByRole("status")).toBeNull();
  expect(screen.getByRole("textbox", { name: "draft" })).toHaveValue("보내지 않은 개인 초안");
  view.unmount();
});
