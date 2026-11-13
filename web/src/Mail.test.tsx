import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vitest";
import { MailConnections } from "./MailConnections";
import type { MailAction } from "../shared/api-types";
const fixture: MailAction = {
  id: "a1",
  version: 1,
  revision: 1,
  connection_id: "c1",
  content: {
    from: "sender@example.test",
    to: ["to@example.test"],
    cc: ["cc@example.test"],
    bcc: ["hidden@example.test"],
    subject: "검토할 메일",
    text: "전체 본문 첫 줄\n마지막 줄",
    reply: null,
  },
  digest: "immutable-digest",
  state: "pending_approval",
  expires_at: 9999999999,
  message_id: "stable@example.test",
  provider_message_id: null,
  provider_thread_id: null,
  origin_thread_id: null,
};
let action: MailAction;
let requests: {
  path: string;
  method: string;
  body: any;
  account: string | null;
}[];
let configured = true;
beforeEach(() => {
  configured = true;
  requests = [];
  action = structuredClone(fixture);
  window.history.replaceState(null, "", "/settings/mail");
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, init: RequestInit = {}) => {
      const method = init.method ?? "GET";
      const headers = new Headers(init.headers);
      const body = init.body ? JSON.parse(init.body as string) : null;
      requests.push({
        path,
        method,
        body,
        account: headers.get("X-Moosuri-Account"),
      });
      if (path.endsWith("/connections"))
        return Response.json({
          configured,
          connections: [
            {
              id: "c1",
              email: "sender@example.test",
              epoch: 1,
              status: "active",
            },
          ],
        });
      if (path.endsWith("/actions") && method === "GET")
        return Response.json({ actions: [action] });
      if (path.endsWith("/approve")) {
        expect(body).toEqual({ revision: 1, digest: "immutable-digest" });
        action = { ...action, version: action.version + 1, state: "approved" };
        return Response.json(action);
      }
      if (path.endsWith("/reject")) {
        action = {
          ...action,
          version: action.version + 1,
          state: "rejected",
          content: null,
        };
        return Response.json(action);
      }
      if (path.endsWith("/actions/a1") && method === "PUT") {
        action = {
          ...action,
          version: action.version + 1,
          revision: 2,
          digest: "new-digest",
          content: body.input.content,
          state: "pending_approval",
        };
        return Response.json(action);
      }
      throw new Error(`Unexpected fixture route ${method} ${path}`);
    }),
  );
});
it("unconfigured Gmail takes an installer to setup while members ask their installer", async () => {
  configured = false;
  const user = userEvent.setup();
  const openGoogleSetup = vi.fn();
  const view = render(<MailConnections accountId="owner" onExpired={vi.fn()} back={vi.fn()} originThreadId={null} openGoogleSetup={openGoogleSetup} />);
  await user.click(await screen.findByRole("button", { name: "Google 설치 설정 열기" }));
  expect(openGoogleSetup).toHaveBeenCalledOnce();
  view.unmount();
  render(<MailConnections accountId="member" onExpired={vi.fn()} back={vi.fn()} originThreadId={null} />);
  await screen.findByText(/설치자에게 Google 연결 설정을 요청하세요/);
  expect(screen.queryByRole("button", { name: "Google 설치 설정 열기" })).toBeNull();
  expect(screen.queryByText(/Google Cloud/)).toBeNull();
  expect(screen.getByRole("button", { name: "Google 계정 연결" })).toBeDisabled();
});
it("shows every recipient and entire body; only human click sends approval with immutable identity", async () => {
  const user = userEvent.setup();
  render(
    <MailConnections
      accountId="owner"
      onExpired={vi.fn()}
      back={vi.fn()}
      originThreadId={null}
    />,
  );
  await user.click(await screen.findByRole("button", { name: /검토할 메일/ }));
  const preview = screen.getByRole("article", { name: "메일 발송 확인" });
  expect(within(preview).getByText("hidden@example.test")).toBeVisible();
  expect(within(preview).getByText("cc@example.test")).toBeVisible();
  expect(preview.textContent).toContain("전체 본문 첫 줄\n마지막 줄");
  expect(requests.some((r) => r.path.endsWith("/approve"))).toBe(false);
  await user.dblClick(screen.getByRole("button", { name: "허락하고 보내기" }));
  await screen.findByText("허락됨 · 발송 대기");
  expect(requests.filter((r) => r.path.endsWith("/approve"))).toHaveLength(1);
  expect(requests.every((r) => r.account === "owner")).toBe(true);
});
it("rejects without approval and edits become a new unapproved revision", async () => {
  const user = userEvent.setup();
  render(
    <MailConnections
      accountId="owner"
      onExpired={vi.fn()}
      back={vi.fn()}
      originThreadId={null}
    />,
  );
  await user.click(await screen.findByRole("button", { name: /검토할 메일/ }));
  await user.click(screen.getByRole("button", { name: "내용 수정" }));
  const subject = screen.getByLabelText("제목");
  await user.clear(subject);
  await user.type(subject, "수정한 제목");
  await user.click(
    screen.getByRole("button", { name: "수정본 저장하고 다시 확인" }),
  );
  await screen.findByText("수정한 제목", { selector: "dd" });
  expect(requests.some((r) => r.path.endsWith("/approve"))).toBe(false);
  await user.click(screen.getByRole("button", { name: "거절" }));
  await screen.findByRole("heading", { name: "거절됨" });
  expect(requests.filter((r) => r.path.endsWith("/approve"))).toHaveLength(0);
});
it("account lifetime fences a late private response when workspace switches", async () => {
  let release: (value: Response) => void = () => {};
  const held = new Promise<Response>((r) => (release = r));
  vi.mocked(fetch).mockImplementation(async (path, init) =>
    String(path).endsWith("/connections")
      ? new Headers(init?.headers).get("X-Moosuri-Account") === "owner"
        ? held
        : Response.json({ configured: true, connections: [] })
      : Response.json({ actions: [] }),
  );
  const view = render(
    <MailConnections
      key="owner"
      accountId="owner"
      onExpired={vi.fn()}
      back={vi.fn()}
      originThreadId={null}
    />,
  );
  await waitFor(() => expect(fetch).toHaveBeenCalled());
  view.rerender(
    <MailConnections
      key="other"
      accountId="other"
      onExpired={vi.fn()}
      back={vi.fn()}
      originThreadId={null}
    />,
  );
  release(
    Response.json({
      configured: true,
      connections: [
        {
          id: "private",
          email: "previous-owner@example.test",
          epoch: 1,
          status: "active",
        },
      ],
    }),
  );
  await new Promise((r) => setTimeout(r, 20));
  expect(screen.queryByText(/previous-owner@example.test/)).toBeNull();
});

it("uncertain approval acknowledgement stops further approval and refresh shows unknown without retry", async () => {
  const base = vi.mocked(fetch).getMockImplementation()!;
  vi.mocked(fetch).mockImplementation(async (path, init) => {
    if (String(path).endsWith("/approve")) {
      requests.push({
        path: String(path),
        method: "POST",
        body: JSON.parse(init!.body as string),
        account: "owner",
      });
      action = { ...action, version: 2, state: "outcome_unknown" };
      throw new TypeError("synthetic lost acknowledgement");
    }
    return base(path, init);
  });
  const user = userEvent.setup();
  render(
    <MailConnections
      accountId="owner"
      onExpired={vi.fn()}
      back={vi.fn()}
      originThreadId={null}
    />,
  );
  await user.click(await screen.findByRole("button", { name: /검토할 메일/ }));
  await user.click(screen.getByRole("button", { name: "허락하고 보내기" }));
  await screen.findByText(/결과 확인 전 추가 허락을 중단/);
  expect(
    screen.getByRole("button", { name: "허락하고 보내기" }),
  ).toBeDisabled();
  await user.click(screen.getByRole("button", { name: "상태 새로고침" }));
  await screen.findByRole("heading", { name: /발송 결과 불명/ });
  expect(requests.filter((r) => r.path.endsWith("/approve"))).toHaveLength(1);
});

it("blocks old-preview approval while editing and while a changed revision PUT is held", async () => {
  const base = vi.mocked(fetch).getMockImplementation()!;
  let release: () => void = () => {};
  const held = new Promise<void>((r) => (release = r));
  let putStarted = false;
  vi.mocked(fetch).mockImplementation(async (path, init) => {
    if (String(path).endsWith("/actions/a1") && init?.method === "PUT") {
      putStarted = true;
      await held;
    }
    return base(path, init);
  });
  const user = userEvent.setup();
  render(
    <MailConnections
      accountId="owner"
      onExpired={vi.fn()}
      back={vi.fn()}
      originThreadId={null}
    />,
  );
  await user.click(await screen.findByRole("button", { name: /검토할 메일/ }));
  await user.click(screen.getByRole("button", { name: "내용 수정" }));
  const approve = screen.getByRole("button", { name: "허락하고 보내기" });
  expect(approve).toBeDisabled();
  await user.click(approve);
  const subject = screen.getByLabelText("제목");
  await user.clear(subject);
  await user.type(subject, "수정한 새 미리보기");
  await user.click(
    screen.getByRole("button", { name: "수정본 저장하고 다시 확인" }),
  );
  await waitFor(() => expect(putStarted).toBe(true));
  expect(
    screen.getByRole("button", { name: "허락하고 보내기" }),
  ).toBeDisabled();
  await user.click(screen.getByRole("button", { name: "허락하고 보내기" }));
  expect(requests.filter((r) => r.path.endsWith("/approve"))).toHaveLength(0);
  release();
  await screen.findByText("수정한 새 미리보기", { selector: "dd" });
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "허락하고 보내기" }),
    ).toBeEnabled(),
  );
  expect(action.revision).toBe(2);
  expect(requests.filter((r) => r.path.endsWith("/approve"))).toHaveLength(0);
});

it("purged action from polling clears editor plaintext and prevents stale editing", async () => {
  const user = userEvent.setup();
  render(
    <MailConnections
      accountId="owner"
      onExpired={vi.fn()}
      back={vi.fn()}
      originThreadId={null}
    />,
  );
  await user.click(await screen.findByRole("button", { name: /검토할 메일/ }));
  await user.click(screen.getByRole("button", { name: "내용 수정" }));
  await user.type(screen.getByLabelText("제목"), " + private unsaved subject");
  await user.type(screen.getByLabelText("전체 본문"), " private unsaved body");
  // A same-revision snapshot must preserve local unsaved edits.
  await user.click(screen.getByRole("button", { name: "상태 새로고침" }));
  await waitFor(() =>
    expect(screen.getByLabelText("제목")).toHaveValue(
      "검토할 메일 + private unsaved subject",
    ),
  );
  action = { ...action, version: 2, state: "cancelled", content: null };
  await screen.findByRole("heading", { name: "취소됨" }, { timeout: 3000 });
  await waitFor(() => {
    expect(screen.getByLabelText("제목")).toHaveValue("");
    expect(screen.getByLabelText("전체 본문")).toHaveValue("");
    expect(screen.getByLabelText("TO")).toHaveValue("");
    expect(screen.getByLabelText("CC")).toHaveValue("");
    expect(screen.getByLabelText("BCC")).toHaveValue("");
  });
  expect(screen.queryByDisplayValue(/private unsaved/)).toBeNull();
  expect(
    screen.queryByRole("button", { name: "수정본 저장하고 다시 확인" }),
  ).toBeNull();
  expect(screen.queryByRole("button", { name: "허락하고 보내기" })).toBeNull();
});
