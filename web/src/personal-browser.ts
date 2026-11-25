import type { BrowserFrame } from "../shared/api-types";
import { browserLocation } from "./browser-frame";

export type PersonalBrowserState =
  | "starting" | "reading" | "ready" | "user_control" | "resuming" | "stopping"
  | "completed" | "closed" | "outcome_unknown";
export type PersonalBrowserTab = { id: string; url: string | null; title: string | null };
export type PersonalBrowserWorkspace = {
  id: string;
  request_id: string;
  run_id: string;
  thread_id: string;
  state: PersonalBrowserState;
  revision: number;
  url: string | null;
  title: string | null;
  tabs: PersonalBrowserTab[];
  active_tab_id: string | null;
  reason: string | null;
  links: { url: string; text: string }[];
};
export type PersonalBrowserStatus = { configured: boolean; workspace: PersonalBrowserWorkspace | null };
export type PersonalBrowserFrame = BrowserFrame & { tab_id: string };
export type PersonalBrowserControl = { workspace: PersonalBrowserWorkspace; control_token: string };
export type PersonalBrowserRequest =
  | { kind: "start"; url: string; wait_for_user: boolean }
  | { kind: "guide"; workspace_id: string }
  | { kind: "google_sign_in" };

export function browserEndpoint(thread: string) {
  return `/threads/${encodeURIComponent(thread)}/browser`;
}
/** The typed Google sign-in operation. Its body is only `{ request_id }`; there is no URL, email or text field. */
export function googleSignInEndpoint(thread: string) {
  return `${browserEndpoint(thread)}/google-sign-in`;
}

export function browserNeedsGuidance(workspace: PersonalBrowserWorkspace | null) {
  return !!workspace && ["starting", "reading", "ready", "user_control"].includes(workspace.state);
}

/** The server stops at sign-in, human-verification or blocked pages and reports one of these reasons. */
export function browserAuthStopped(workspace: PersonalBrowserWorkspace | null) {
  return !!workspace?.reason?.startsWith("browser_auth_");
}

export function browserCanWatch(workspace: PersonalBrowserWorkspace | null) {
  return !!workspace?.active_tab_id && !browserAuthStopped(workspace) && !browserGoogleSignIn(workspace) &&
    ["reading", "ready", "user_control", "completed"].includes(workspace.state);
}

export const officialGoogleAccountUrl = "https://accounts.google.com/";
export type AuthHandoff = { provider: "google" | "other"; officialUrl: string | null };

/** What the server recorded for one typed Google sign-in attempt. Only `ready` means signed in. */
export type GoogleSignInStatus = "ready" | "needs_human" | "blocked" | "unavailable" | "unknown";
const googleStatusReasons: Record<string, GoogleSignInStatus> = {
  browser_google_ready: "ready",
  browser_google_needs_human: "needs_human",
  browser_google_blocked: "blocked",
  browser_google_unavailable: "unavailable",
  browser_google_unknown: "unknown",
};
export function browserGoogleSignIn(workspace: PersonalBrowserWorkspace | null):
  { status: GoogleSignInStatus } | null {
  const status = workspace?.reason ? googleStatusReasons[workspace.reason] : undefined;
  return status ? { status } : null;
}
export const googleStatusLabel: Record<GoogleSignInStatus, string> = {
  ready: "Google 로그인 확인됨",
  needs_human: "Kiri에서 Google이 본인 확인을 요청함 · 로그인되지 않음",
  blocked: "Google 로그인 차단 · 로그인되지 않음",
  unavailable: "Google 로그인을 시도하지 못함 · 로그인되지 않음",
  unknown: "Google 로그인 결과 확인 불가 · 자동 재시도 없음",
};

const googleName = String.raw`(?:내\s*)?(?:구글|google|지메일|gmail)(?:\s*계정)?`;
// Whole-message, imperative requests only. Questions, documentation and other prose that merely mention
// logging in are never matched, so public pages about login remain ordinary browser tasks.
const googleKorean = new RegExp(
  String.raw`^\s*(?:브라우저에서\s*)?${googleName}(?:으로|에|에서)?\s*로그인\s*(?:좀\s*)?(?:해\s*줘|해\s*주세요|해줘요|해\s*줄래|시도해\s*줘|시도해\s*주세요)\s*[.!]?\s*$`, "iu");
const googleEnglish = /^\s*(?:please\s+)?(?:sign|log)[\s-]?(?:in|me\s+in)\s+(?:to|into|with)\s+(?:my\s+)?(?:google|gmail)(?:\s+account)?\s*(?:please)?\s*[.!]?\s*$/iu;
const otherKorean = /^\s*(?:이\s*)?(?:사이트|웹사이트|페이지)?(?:에)?\s*로그인\s*(?:좀\s*)?(?:해\s*줘|해\s*주세요|해줘요)\s*[.!]?\s*$/u;
const otherEnglish = /^\s*(?:please\s+)?(?:sign|log)\s*(?:me\s+)?in(?:\s+for\s+me)?\s*[.!]?\s*$/iu;

/** True only for an explicit request to sign in to the member's Google account. Any URL disqualifies it. */
export function explicitGoogleSignIn(text: string) {
  return !/https?:\/\//iu.test(text) && (googleKorean.test(text) || googleEnglish.test(text));
}

// Opening the connector is navigation. It never grants a scope or starts a browser login.
export function googleConnectionRequest(text: string): { service?: string } | null {
  const name = "(gws|google workspace|구글 워크스페이스|gmail|지메일|google drive|구글 드라이브|google docs|구글 문서|google slides|구글 슬라이드|google sheets|구글 시트)";
  const message = text.trim().toLowerCase().replace(/\s+/gu, " ").replace(/[.!]$/u, "").trimEnd();
  const korean = new RegExp("^(?:(?:구글|google)(?: 계정)?(?:에|으로)? 로그인(?:하고|해서|한 뒤) )?" + name + "(?:까지|을|를|도)?\\s*(?:연결|연동|등록)(?:을|을 좀| 좀)?\\s*(?:해\\s*(?:줘|주세요|줘요)|하기)$", "u");
  const english = new RegExp("^(?:please )?(?:sign in to google and )?connect (?:my )?" + name + "$", "u");
  const match = korean.exec(message) ?? english.exec(message);
  if (!match) return null;
  const services: Record<string, string> = {
    gmail: "gmail", "지메일": "gmail", "google drive": "drive", "구글 드라이브": "drive",
    "google docs": "docs", "구글 문서": "docs", "google slides": "slides", "구글 슬라이드": "slides",
    "google sheets": "sheets", "구글 시트": "sheets",
  };
  return { service: services[match[1]] };
}

/**
 * A sign-in request is never a general browser task. Google gets the typed `google_sign_in` operation
 * (see `browserIntent`); other sites get a truthful handoff. URLs are stripped first for the handoff.
 */
export function browserAuthIntent(text: string): AuthHandoff | null {
  if (explicitGoogleSignIn(text)) return { provider: "google", officialUrl: officialGoogleAccountUrl };
  const prose = text.replace(/https?:\/\/[^\s<>"`]+/giu, " ");
  return otherKorean.test(prose) || otherEnglish.test(prose) ? { provider: "other", officialUrl: null } : null;
}

/** True for addresses that identify an authentication page; such addresses are never opened here. */
export function browserAuthAddress(href: string | null | undefined) {
  let url: URL;
  try { url = new URL(href ?? ""); } catch { return false; }
  if (url.hostname === "accounts.google.com") return true;
  const path = url.pathname.toLowerCase();
  return /(?:^|[^a-z0-9])(?:login|logon|signin|auth|oauth2?|sso|saml|password|authorize)(?:$|[^a-z0-9])/.test(path) ||
    path.includes("sign-in") || path.includes("log-in") ||
    ["client_id", "redirect_uri", "response_type", "access_token", "id_token", "login_hint"]
      .some((key) => url.searchParams.has(key));
}

export const authHandoffStatus =
  "로그인은 이 브라우저 작업공간에서 진행하거나 확인하지 않았습니다. 현재 로그인된 상태가 아닙니다.";

export type BrowserIntent = { url: string | null; wait: boolean; operation?: "google_sign_in" };

/** An explicit provider operation carries no URL and no secret; everything else is a public page read. */
export function browserIntent(text: string, selected = false): BrowserIntent | null {
  if (explicitGoogleSignIn(text)) return { url: null, wait: false, operation: "google_sign_in" };
  const fullAddress = text.match(/https?:\/\/[^\s<>"`]+/i)?.[0];
  const domain = text.match(/(?:^|[\s(\[])([a-z\d][a-z\d.-]*\.[a-z]{2,24}(?::\d{1,5})?(?:\/[^\s<>"`]*)?)(?=$|[\s),.!?\]])/i)?.[1];
  const address = (fullAddress ?? (domain ? `https://${domain}` : undefined))
    ?.replace(/[).,!?;，。！？\]}]+$/u, "");
  const browserTarget = /브라우저|웹사이트|웹페이지|사이트|\b(?:browser|website|webpage)\b/iu.test(text);
  const action = /열어|접속|들어가|탐색|읽어|찾아|확인해|조사해|살펴|요약해|\b(?:open|read|check|inspect|research|browse|visit|access|summarize|look at|navigate to|go to)\b/iu.test(text);
  const explicit = !!(address || browserTarget) && action;
  if (!selected && !explicit) return null;
  if (browserAuthIntent(text)) return null;
  if (browserAuthAddress(browserLocation(address)?.href)) return null;
  const manual = /직접.{0,12}(?:조작|입력|바꿀|선택|로그인|확인)|(?:로그인|조작|입력|선택).{0,8}(?:할게|하겠|하고 싶)|(?:직접 조작|조건을 바꾸|넘겨|기다려)|\b(?:take over|manual|wait for me|let me|i will|i'll)\b/iu.test(text);
  return { url: browserLocation(address)?.href ?? null, wait: manual };
}

export const browserStateLabel: Record<PersonalBrowserState, string> = {
  starting: "브라우저를 준비하는 중",
  reading: "브라우저에서 페이지를 읽는 중",
  ready: "브라우저에서 확인을 기다리는 중",
  user_control: "브라우저 직접 조작 중",
  resuming: "변경된 화면을 읽는 중",
  stopping: "브라우저 작업을 중지하는 중",
  completed: "브라우저 읽기 완료",
  closed: "브라우저 작업 중지됨",
  outcome_unknown: "브라우저 상태 확인 필요",
};
