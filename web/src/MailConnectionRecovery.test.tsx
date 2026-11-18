import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, it } from "vitest";
import { MailConnectionRecovery } from "./MailConnectionRecovery";
import type { MailConnectionRecovery as Recovery } from "../shared/api-types";
afterEach(cleanup);
it("uses only fixed server recovery actions and separates installer setup from owner consent", () => {
  const view = render(<MailConnectionRecovery action="admin_setup" />);
  expect(screen.getByRole("link", {name: "Google 설치 설정 열기 ↗"})).toHaveAttribute("href", "/admin/google");
  view.rerender(<MailConnectionRecovery action="connect" />);
  expect(screen.getByRole("link", {name: "내 Gmail 읽기 연결하기 ↗"})).toHaveAttribute("href", "/settings/mail");
  view.rerender(<MailConnectionRecovery action="member_setup" />);
  expect(screen.queryByRole("link", {name: "Google 설치 설정 열기 ↗"})).toBeNull();
  expect(screen.getByRole("link")).toHaveAttribute("href", "/settings/mail");
  view.rerender(<MailConnectionRecovery action={"https://untrusted.example" as Recovery} />);
  expect(screen.queryByRole("link")).toBeNull();
  view.rerender(<MailConnectionRecovery />);
  expect(screen.queryByRole("link")).toBeNull();
});
