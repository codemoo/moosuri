import { render, screen, within } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { PublicInformation, resolvePublicInformationPath } from "./PublicInformation";

afterEach(() => vi.unstubAllGlobals());

function stubNetwork() {
  const fetcher = vi.fn();
  vi.stubGlobal("fetch", fetcher);
  return fetcher;
}

it("opens the homepage anonymously, describes the product and links privacy, terms and the app", () => {
  const fetcher = stubNetwork();
  render(<PublicInformation page="about" />);
  expect(screen.getByRole("heading", { level: 1, name: "Moosuri 소개" })).toBeVisible();
  expect(screen.getByText(/직접 설치해 운영하는 개인 비서/)).toBeVisible();
  const main = screen.getByRole("main");
  expect(within(main).getByRole("link", { name: "개인정보 처리방침" })).toHaveAttribute("href", "/privacy");
  expect(within(main).getByRole("link", { name: "이용 약관" })).toHaveAttribute("href", "/terms");
  expect(screen.getByRole("link", { name: "앱 열기 · 로그인" })).toHaveAttribute("href", "/app");
  expect(fetcher).not.toHaveBeenCalled();
});

it("keeps the footer navigation on stable own-origin paths and marks the current page", () => {
  render(<PublicInformation page="privacy" />);
  const nav = screen.getByRole("navigation", { name: "공개 정보" });
  expect(within(nav).getByRole("link", { name: "소개" })).toHaveAttribute("href", "/about");
  expect(within(nav).getByRole("link", { name: "개인정보 처리방침" })).toHaveAttribute("aria-current", "page");
  expect(within(nav).getByRole("link", { name: "이용 약관" })).toHaveAttribute("href", "/terms");
});

it("states privacy facts and remote revocation without promising self-service purge", () => {
  const fetcher = stubNetwork();
  render(<PublicInformation page="privacy" />);
  for (const name of ["저장하는 정보", "Google 사용자 데이터", "볼트와 관리자", "모델 제공업체", "연결 해제와 삭제"]) {
    expect(screen.getByRole("heading", { level: 2, name })).toBeVisible();
  }
  expect(screen.getByText(/제목·보낸 사람·날짜만 조회/)).toBeVisible();
  expect(screen.getByText(/일정 생성·시간 변경에는 별도의 Calendar 권한과 회원의 최종 내용 승인/)).toBeVisible();
  expect(screen.getByText(/받은편지함 조회 동의만으로 메일을 보내지 않습니다/)).toBeVisible();
  expect(screen.getByText(/앱 관리자 권한만으로는 개인 볼트를 열 수 없고/)).toBeVisible();
  expect(screen.getByText(/저장된 받은편지함 조회 결과가 제거됩니다/)).toBeVisible();
  expect(screen.getByText(/백업 전체를 삭제하거나 Google 쪽 접근 권한을 철회하는 것은 아닙니다/)).toBeVisible();
  expect(screen.getByRole("link", { name: "Google 계정 연결 관리" })).toHaveAttribute("href", "https://myaccount.google.com/connections");
  expect(screen.queryByText(/모든 데이터는 서버를 떠나지 않/)).toBeNull();
  expect(fetcher).not.toHaveBeenCalled();
});

it("shows terms with authorized account use and operator responsibility", () => {
  const fetcher = stubNetwork();
  render(<PublicInformation page="terms" />);
  expect(screen.getByRole("heading", { level: 1, name: "이용 약관" })).toBeVisible();
  expect(screen.getByText(/본인의 계정과 이용 권한이 있는 자료/)).toBeVisible();
  expect(screen.getByRole("heading", { level: 2, name: "운영자의 책임" })).toBeVisible();
  expect(screen.getByRole("link", { name: "Google 계정 연결 관리" })).toBeVisible();
  expect(fetcher).not.toHaveBeenCalled();
});

it("resolves only the exact public paths so every other route stays the ordinary app", () => {
  expect(resolvePublicInformationPath("/about")).toBe("about");
  expect(resolvePublicInformationPath("/privacy")).toBe("privacy");
  expect(resolvePublicInformationPath("/terms")).toBe("terms");
  for (const path of ["/", "/app", "/login", "/about/", "/About", "/privacy/x", "/terms.html", "/toString", "/__proto__"]) {
    expect(resolvePublicInformationPath(path)).toBeNull();
  }
});
