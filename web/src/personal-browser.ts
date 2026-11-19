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
export type PersonalBrowserFrame = BrowserFrame & { tab_id: string; proof_valid_for_ms?: number };
export type PersonalBrowserControl = { workspace: PersonalBrowserWorkspace; control_token: string };
export type PersonalBrowserRequest =
  | { kind: "start"; url: string; wait_for_user: boolean }
  | { kind: "guide"; workspace_id: string };

export function browserEndpoint(thread: string) {
  return `/threads/${encodeURIComponent(thread)}/browser`;
}

export function browserNeedsGuidance(workspace: PersonalBrowserWorkspace | null) {
  return !!workspace && ["starting", "reading", "ready", "user_control"].includes(workspace.state);
}

export function browserCanWatch(workspace: PersonalBrowserWorkspace | null) {
  return !!workspace?.active_tab_id && ["reading", "ready", "user_control", "completed"].includes(workspace.state);
}

export function browserIntent(text: string, selected = false): { url: string | null; wait: boolean } | null {
  const fullAddress = text.match(/https?:\/\/[^\s<>"`]+/i)?.[0];
  const domain = text.match(/(?:^|[\s(\[])([a-z\d][a-z\d.-]*\.[a-z]{2,24}(?::\d{1,5})?(?:\/[^\s<>"`]*)?)(?=$|[\s),.!?\]])/i)?.[1];
  const address = (fullAddress ?? (domain ? `https://${domain}` : undefined))
    ?.replace(/[).,!?;，。！？\]}]+$/u, "");
  const browserTarget = /브라우저|웹사이트|웹페이지|사이트|\b(?:browser|website|webpage)\b/iu.test(text);
  const action = /열어|접속|들어가|탐색|읽어|찾아|확인해|조사해|살펴|요약해|\b(?:open|read|check|inspect|research|browse|visit|access|summarize|look at|navigate to|go to)\b/iu.test(text);
  const explicit = !!(address || browserTarget) && action;
  if (!selected && !explicit) return null;
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
