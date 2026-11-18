import { StrictMode } from "react";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { MailConnections } from "./MailConnections";
import { GWS_CONNECTED } from "./gws-connection";
import { beginGoogleMailResume, pendingGoogleMailResume, resumeGoogleMail } from "./google-mail-connection";
import type { MailConnection } from "../shared/api-types";

const target = { threadId: "11111111-1111-4111-8111-111111111111", runId: "22222222-2222-4222-8222-222222222222" };
const flowId = "a".repeat(64);
const otherFlowId = "b".repeat(64);
const pinnedTarget = {...target, flowId};
const verified: MailConnection & {flow_id:string} = { id: "33333333-3333-4333-8333-333333333333", epoch: 7, email: "owner@example.test", status: "active", compose_enabled: false, flow_id: flowId };
let calls: { path: string; method: string; body: any; account: string | null }[];
let completeFailed = false;
let resumeFailed = false;
beforeEach(() => {
  sessionStorage.clear(); calls = []; completeFailed = false; resumeFailed = false;
  window.history.replaceState(null, "", `/settings/mail?oauth=synthetic-once&oauth_flow=${flowId}&keep=1`);
  vi.stubGlobal("fetch", vi.fn(async (url: string, init: RequestInit = {}) => {
    const path = url.replace("/api/v1", "");
    calls.push({ path, method: init.method ?? "GET", body: init.body ? JSON.parse(String(init.body)) : null, account: new Headers(init.headers).get("X-Moosuri-Account") });
    if (path === "/mail/connections") return Response.json({configured: true, connections: [verified]});
    if (path === "/mail/actions") return Response.json({actions: []});
    if (path === "/mail/gws-inbox") return Response.json({configured:false});
    if (path === "/mail/oauth/complete") return completeFailed ? Response.json({code:"google_oauth_exchange_failed"}, {status:400}) : Response.json(verified);
    if (path.endsWith("/resume-gmail")) return resumeFailed ? Response.json({code:"gmail_resume_unavailable"}, {status:409}) : Response.json({});
    throw new Error(`Unexpected ${path}`);
  }));
});
afterEach(() => { cleanup(); sessionStorage.clear(); vi.unstubAllGlobals(); });
function mount(accountId = "owner") {
  return render(<StrictMode><MailConnections accountId={accountId} onExpired={vi.fn()} back={vi.fn()} originThreadId={null} /></StrictMode>);
}
it("verified OAuth resumes exactly the eligible original read once using its connection epoch", async () => {
  beginGoogleMailResume("owner", target, flowId);
  const returned = vi.fn();
  window.addEventListener(GWS_CONNECTED, returned);
  const view = mount();
  await screen.findByText("Google 연결을 마쳤습니다. 원래 받은메일 요청을 이어갑니다.");
  expect(calls.filter(c => c.path === "/mail/oauth/complete")).toHaveLength(1);
  expect(calls.filter(c => c.method === "POST")).toEqual([
    { path:"/mail/oauth/complete", method:"POST", body:{handle:"synthetic-once"}, account:"owner" },
    { path:`/threads/${target.threadId}/runs/${target.runId}/resume-gmail`, method:"POST", body:{connection_id:verified.id,connection_epoch:7}, account:"owner" },
  ]);
  expect(pendingGoogleMailResume("owner")).toBeNull();
  expect(window.location.search).toBe("?keep=1");
  expect(returned).toHaveBeenCalledOnce();
  expect(screen.queryByText(/GWS|Mac|기기 Gmail 연결/)).toBeNull();
  view.unmount(); mount();
  await screen.findByText("내 Google 계정 연결");
  expect(calls.filter(c => c.path.endsWith("/resume-gmail"))).toHaveLength(1);
  window.removeEventListener(GWS_CONNECTED, returned);
});
it.each(["settings", "other-account", "expired"])("%s OAuth only connects without replaying chat or sending mail", async mode => {
  if (mode !== "settings") beginGoogleMailResume("owner", target, flowId);
  if (mode === "expired") sessionStorage.setItem("moosuri.google-mail-resume.v1.owner", JSON.stringify({...target,flowId,at:Date.now()-31*60*1000}));
  mount(mode === "other-account" ? "other" : "owner");
  await screen.findByText("Google 연결을 마쳤습니다. 연결된 계정을 확인한 뒤 대화로 돌아가세요.");
  expect(calls.filter(c => c.method === "POST").map(c => c.path)).toEqual(["/mail/oauth/complete"]);
});
it.each(["denied", "complete-failed", "resume-rejected"])("%s clears continuation and never retries or replays request text", async mode => {
  beginGoogleMailResume("owner", target, flowId);
  if (mode === "denied") window.history.replaceState(null,"",`/settings/mail?oauth_error=denied&oauth_flow=${flowId}&oauth=must-not-complete`);
  completeFailed = mode === "complete-failed"; resumeFailed = mode === "resume-rejected";
  const view = mount();
  await screen.findByRole("alert");
  expect(pendingGoogleMailResume("owner")).toBeNull();
  const postCount = calls.filter(c => c.method === "POST").length;
  view.unmount(); mount();
  await screen.findByText("내 Google 계정 연결");
  expect(calls.filter(c => c.method === "POST")).toHaveLength(postCount);
  expect(calls.some(c => c.path === "/turns" || c.path.includes("approve"))).toBe(false);
});
it("invalid verification cannot invoke the server resume endpoint", async () => {
  beginGoogleMailResume("owner", target, flowId);
  const request = vi.fn();
  await expect(resumeGoogleMail("owner", pinnedTarget, {...verified,epoch:0}, request)).rejects.toThrow();
  expect(request).not.toHaveBeenCalled();
  expect(pendingGoogleMailResume("owner")).toBeNull();
});
it("account switches fence a late OAuth completion and its old continuation", async () => {
  let finish!: (value: Response) => void;
  const originalFetch = globalThis.fetch;
  vi.stubGlobal("fetch", vi.fn((url:string, init?:RequestInit) => url.endsWith("/mail/oauth/complete") ? new Promise<Response>(resolve => {finish=resolve;}) : originalFetch(url,init)));
  beginGoogleMailResume("owner", target, flowId);
  const view = mount();
  await waitFor(() => expect(finish).toBeDefined());
  view.rerender(<StrictMode><MailConnections accountId="other" onExpired={vi.fn()} back={vi.fn()} originThreadId={null} /></StrictMode>);
  finish(Response.json(verified));
  await screen.findByText("내 Google 계정 연결");
  expect(calls.some(c => c.path.endsWith("/resume-gmail"))).toBe(false);
});

it.each(["success", "denied", "failed"])("old OAuth callback %s cannot consume a newer same-tab flow", async mode => {
  beginGoogleMailResume("owner", target, otherFlowId);
  if (mode === "denied") window.history.replaceState(null,"",`/settings/mail?oauth_error=denied&oauth_flow=${flowId}`);
  completeFailed = mode === "failed";
  mount();
  if (mode === "success") await screen.findByText("Google 연결을 마쳤습니다. 다른 연결 흐름의 요청은 이어가지 않았습니다.");
  else await screen.findByRole("alert");
  expect(calls.some(c => c.path.endsWith("/resume-gmail"))).toBe(false);
  expect(pendingGoogleMailResume("owner")).toEqual({...target,flowId:otherFlowId});
});
it("a late completion cannot resume or erase a marker replaced during transport", async () => {
  let finish!: (value:Response) => void;
  const originalFetch=globalThis.fetch;
  vi.stubGlobal("fetch",vi.fn((url:string,init?:RequestInit)=>url.endsWith("/mail/oauth/complete") ? new Promise<Response>(resolve=>{finish=resolve;}) : originalFetch(url,init)));
  beginGoogleMailResume("owner",target,flowId);
  mount();
  await waitFor(()=>expect(finish).toBeDefined());
  const newer={...target,runId:"44444444-4444-4444-8444-444444444444"};
  beginGoogleMailResume("owner",newer,otherFlowId);
  finish(Response.json(verified));
  await screen.findByText("Google 연결을 마쳤습니다. 다른 연결 흐름의 요청은 이어가지 않았습니다.");
  expect(calls.some(c=>c.path.endsWith("/resume-gmail"))).toBe(false);
  expect(pendingGoogleMailResume("owner")).toEqual({...newer,flowId:otherFlowId});
});
it("missing or forged flow verification cannot authorize continuation", async () => {
  beginGoogleMailResume("owner",target,flowId);
  const request=vi.fn();
  expect(await resumeGoogleMail("owner",pinnedTarget,{...verified,flow_id:otherFlowId},request)).toBe(false);
  expect(await resumeGoogleMail("owner",pinnedTarget,{...verified,flow_id:""},request)).toBe(false);
  expect(request).not.toHaveBeenCalled();
  expect(pendingGoogleMailResume("owner")).toEqual(pinnedTarget);
});
