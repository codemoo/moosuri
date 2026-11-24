import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expect, it, vi } from "vitest";
import { MemberMailWork, parsePrepared, type CitedRead } from "./MemberMailWork";

const read: CitedRead = { request_id: "r1", state: "completed", source: { mailbox: "a@example.test" }, result: { mailbox: "a@example.test", messages: [{ id: "m1", subject: "Application status", from: "Recruiter <r@example.test>", date: "Mon" }] } };
const source = { read_request_id: "r1", connection_id: "g1", epoch: 3, mailbox: "a@example.test", message_id: "m1", subject: "Application status", from: "Recruiter <r@example.test>", date: "Mon" };
const draft = { to: ["r@example.test"], subject: "Re: Application status", body: "Thank you." };
const preview = { action_id: "act", action_revision: 4, action_digest: "d".repeat(64), connection_id: "c1", connection_epoch: 2, from: "a@example.test", to: ["r@example.test"], subject: "Re: Application status", body: "Thank you.", expires_at: 9999999999, action_state: "pending_approval", threaded: false, can_save_draft: true, can_approve_send: true };
const work = (delivery: unknown, over: object = {}) => ({ id: "w1", thread_id: "t", revision: 1, digest: "x".repeat(64), state: "local_draft", source, draft, delivery, created_at: 1, updated_at: 1, ...over });
const props = (request: any, extra: object = {}) => ({ request, threadId: "t", read, fail: (e: unknown) => { throw e; }, disabled: false, newId: () => "fixed-id", ...extra });

it("readonly result is an actionable local draft: exact requirements, no write controls, replay-safe prepare", async () => {
  const needs = work({ state: "needs_connection", reason: "readonly_only", requirements: ["standard_gmail_connection_same_mailbox", "scope_gmail_send_to_approve_and_send", "scope_gmail_compose_to_save_gmail_draft"] });
  let attempts = 0;
  const request = vi.fn(async (path: string, _m?: string, body?: any) => {
    if (path.endsWith("/list")) return { items: [] };
    attempts += 1;
    if (attempts === 1) throw Error("network");
    expect(body).toMatchObject({ id: "fixed-id", thread_id: "t", read_request_id: "r1", message_id: "m1" });
    return { outcome: "draft", work: needs };
  });
  const failed = vi.fn(); const user = userEvent.setup();
  render(<MemberMailWork {...props(request, { fail: failed })} />);
  await user.type(screen.getByLabelText("회신 본문"), "Thank you.");
  await user.click(screen.getByRole("button", { name: "로컬 회신 초안 만들기" }));
  expect(await screen.findByText(/같은 요청을 다시 누르면 중복 없이 이어집니다/)).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "로컬 회신 초안 만들기" }));
  expect(await screen.findByText(/읽기 전용입니다/)).toBeInTheDocument();
  expect(failed).toHaveBeenCalledTimes(1);
  const ids = request.mock.calls.filter(c => c[0].endsWith("/prepare")).map(c => c[2].id);
  expect(ids).toEqual(["fixed-id", "fixed-id"]);
  expect(screen.getByText(/출처: 읽기 전용 Gmail a@example.test/)).toBeInTheDocument();
  expect(screen.getByText("Gmail에 저장하거나 보내려면:")).toBeInTheDocument();
  expect(screen.getByText("Gmail 초안 저장: gmail.compose 권한")).toBeInTheDocument();
  for (const name of [/Gmail 초안으로 저장/, /발송 승인/, /최종 확인본 만들기/]) expect(screen.queryByRole("button", { name })).toBeNull();
  expect(() => parsePrepared({ outcome: "draft", work: { ...needs, delivery: { state: "delivered" } } })).toThrow();
});

it("final preview acts only on the exact shown revision and digest; unknown never claims a receipt", async () => {
  const shown = work({ state: "final_preview", preview });
  const request = vi.fn(async (path: string, _m?: string, body?: any) => {
    if (path.endsWith("/list")) return { items: [shown] };
    if (path.endsWith("/save-draft")) return work({ state: "unknown", operation: "gmail_draft", preview: { ...preview, can_save_draft: false } });
    if (path.endsWith("/approve")) return work({ state: "approved", preview: { ...preview, action_state: "approved" }, gmail_draft_id: null });
    throw Error(path + JSON.stringify(body));
  });
  const user = userEvent.setup();
  render(<MemberMailWork {...props(request)} />);
  expect(await screen.findByText("최종 확인본입니다. 아직 저장되거나 발송되지 않았습니다.")).toBeInTheDocument();
  expect(screen.getByText("원본 메일의 스레드에 연결되지 않은 새 메일로 저장·발송됩니다.")).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "이 확인본을 Gmail 초안으로 저장" }));
  const saved = request.mock.calls.find(c => c[0].endsWith("/save-draft"))![2];
  expect(saved).toEqual({ id: "w1", revision: 1, action_revision: 4, action_digest: "d".repeat(64) });
  expect(await screen.findByRole("alert")).toHaveTextContent("Gmail 저장 결과를 확인하지 못했습니다");
  expect(screen.queryByText(/Gmail 초안에 저장됨/)).toBeNull();
  expect(screen.queryByText(/발송 확인됨/)).toBeNull();
  await user.click(screen.getByRole("button", { name: "Gmail 저장 결과 확인" }));
  expect(request.mock.calls.filter(c => c[0].endsWith("/save-draft"))).toHaveLength(2);
  expect(request.mock.calls.some(c => c[0].endsWith("/approve"))).toBe(false);
});
