// @vitest-environment jsdom
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { AuthHandoffCard, PersonalBrowserCard } from "./PersonalBrowser";
import type { PersonalBrowser } from "./usePersonalBrowser";
import {
  browserAuthAddress, browserAuthIntent, browserAuthStopped, browserCanWatch, browserGoogleSignIn, browserIntent,
  googleSignInEndpoint, type PersonalBrowserWorkspace,
} from "./personal-browser";

const workspace = (reason: string | null): PersonalBrowserWorkspace => ({
  id: "w", request_id: "r", run_id: "u", thread_id: "t", state: "ready", revision: 1,
  url: "https://accounts.google.com", title: null, tabs: [], active_tab_id: "tab", reason, links: [],
});

describe("member browser sign-in handling", () => {
  it("keeps ordinary public requests and login documentation as browser tasks", () => {
    expect(browserAuthIntent("https://example.org/login 페이지 내용을 읽어줘")).toBeNull();
    expect(browserAuthIntent("로그인 없이 볼 수 있는 시간표를 읽어줘")).toBeNull();
    for (const prose of [
      "Google 로그인 방법을 설명하는 공개 문서를 요약해줘",
      "how do I sign in to Google with a security key?",
      "https://docs.example.org/guides/sign-in 읽어줘",
      "로그인 화면 디자인 가이드를 찾아줘",
    ]) {
      expect(browserAuthIntent(prose)).toBeNull();
      expect(browserIntent(prose)?.operation).toBeUndefined();
    }
    expect(browserIntent("https://example.org/timetable 열어서 요약해줘")?.url).toBe("https://example.org/timetable");
  });
  it("turns only an explicit Google request into the typed operation, never a URL read", () => {
    for (const ask of ["구글 로그인 해줘", "구글 계정으로 로그인해줘", "Please sign in to Google", "sign me in to my Gmail account"]) {
      expect(browserIntent(ask)).toEqual({ url: null, wait: false, operation: "google_sign_in" });
    }
    expect(browserAuthIntent("구글 로그인 해줘")).toEqual({ provider: "google", officialUrl: "https://accounts.google.com/" });
    expect(browserAuthIntent("사이트에 로그인해줘")?.provider).toBe("other");
    expect(browserIntent("사이트에 로그인해줘")).toBeNull();
    // Any URL, open/redirect wording or extra instruction means this is not the typed operation.
    expect(browserIntent("구글 로그인 해줘 https://example.org")?.operation).toBeUndefined();
    expect(browserIntent("구글 로그인 해줘 브라우저로 열어", true)?.operation).toBeUndefined();
    expect(browserIntent("https://accounts.google.com 열어줘")).toBeNull();
    expect(googleSignInEndpoint("t 1")).toBe("/threads/t%201/browser/google-sign-in");
  });
  it("shows only a typed ready as signed in, and an official link only for a human step", () => {
    expect(browserGoogleSignIn(workspace("browser_google_ready"))).toEqual({ status: "ready", officialUrl: null });
    expect(browserGoogleSignIn(workspace("browser_google_needs_human"))?.officialUrl).toBe("https://accounts.google.com/");
    expect(browserGoogleSignIn(workspace("browser_google_blocked"))?.officialUrl).toBe("https://accounts.google.com/");
    expect(browserGoogleSignIn(workspace("browser_google_unavailable"))?.officialUrl).toBeNull();
    expect(browserGoogleSignIn(workspace("browser_google_unknown"))?.status).toBe("unknown");
    expect(browserGoogleSignIn(workspace("browser_auth_google"))).toBeNull();
    expect(browserCanWatch(workspace("browser_google_ready"))).toBe(false);
    const { container } = render(<PersonalBrowserCard pending={false} onOpen={() => {}}
      browser={{ status: { configured: true, workspace: workspace("browser_google_needs_human") }, preview: null } as unknown as PersonalBrowser} />);
    expect(screen.getByRole("status").textContent).toContain("로그인되지 않음");
    expect(screen.getByRole("link").getAttribute("href")).toBe("https://accounts.google.com/");
    expect(container.querySelector("button, input, img")).toBeNull();
  });
  it("never opens authentication addresses", () => {
    expect(browserAuthAddress("https://accounts.google.com/v3/signin")).toBe(true);
    expect(browserAuthAddress("https://idp.example.net/start?client_id=a&redirect_uri=b")).toBe(true);
    expect(browserAuthAddress("https://example.org/members/login")).toBe(true);
    expect(browserAuthAddress("https://example.org/timetable")).toBe(false);
  });
  it("hides the screen and manual control once the server stopped at an auth page", () => {
    for (const reason of ["browser_auth_required", "browser_auth_google", "browser_auth_human_verification", "browser_auth_blocked"]) {
      expect(browserAuthStopped(workspace(reason))).toBe(true);
      expect(browserCanWatch(workspace(reason))).toBe(false);
    }
    expect(browserCanWatch(workspace(null))).toBe(true);
  });
  it("renders a truthful handoff with no credential entry", () => {
    const { container } = render(<AuthHandoffCard handoff={{ provider: "google", officialUrl: "https://accounts.google.com/" }} />);
    expect(screen.getByText(/로그인된 상태가 아닙니다/)).toBeTruthy();
    const link = screen.getByRole("link");
    expect(link.getAttribute("href")).toBe("https://accounts.google.com/");
    expect(link.getAttribute("rel")).toContain("noopener");
    expect(container.querySelector("input, textarea, form")).toBeNull();
    expect(screen.queryByRole("button")).toBeNull();
  });
  it("offers consent only when the member's own connector path is supplied", () => {
    render(<AuthHandoffCard handoff={{ provider: "other", officialUrl: null }} onConnect={() => {}} />);
    expect(screen.queryByRole("link")).toBeNull();
    expect(screen.getByRole("button").textContent).toContain("연결 동의");
  });
});
