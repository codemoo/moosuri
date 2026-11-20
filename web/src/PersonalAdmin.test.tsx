import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import PersonalAdmin from "./PersonalAdmin";
const id = "e8675fc3-aaf7-42d5-97e4-a88e9ef28679";
afterEach(() => {
  cleanup();
  sessionStorage.clear();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});
it("binds a management change to the displayed revision and reuses the request after a lost reply", async () => {
  const requests: unknown[] = [];
  let reads = 0;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, init: RequestInit) => {
      if (path.endsWith("/change")) {
        requests.push(JSON.parse(String(init.body)));
        if (requests.length === 1) throw new TypeError("lost response");
        return Response.json({});
      }
      reads++;
      return Response.json({
        members: [
          {
            account_id: id,
            email: "guest@example.test",
            generation: 1,
            revision: 4,
            admin: reads > 1,
            ready: true,
            eligible: true,
            suspended: false,
          },
        ],
      });
    }),
  );
  const user = userEvent.setup();
  render(
    <PersonalAdmin account="actor" expired={() => {}} unavailable={() => {}} />,
  );
  await user.click(await screen.findByText("계정 관리"));
  await user.click(
    await screen.findByRole("button", { name: "관리자로 지정" }),
  );
  const dialog = screen.getByRole("dialog", { name: "관리자 권한을 부여할까요?" });
  expect(within(dialog).getByRole("button", { name: "취소" })).toHaveFocus();
  expect(requests).toHaveLength(0);
  await user.click(within(dialog).getByRole("button", { name: "관리자로 지정" }));
  await user.click(
    await screen.findByRole("button", { name: "변경 결과 확인" }),
  );
  await screen.findByRole("button", { name: "일반 회원으로 변경" });
  expect(await screen.findByText("guest@example.test · 관리자로 지정했습니다.")).toBeInTheDocument();
  expect(requests).toHaveLength(2);
  expect(requests[1]).toEqual(requests[0]);
  expect(requests[0]).toMatchObject({
    account_id: id,
    revision: 4,
    generation: 1,
    action: "promote",
  });
});

it("recovers a lost invitation across remount, copies the host command, and restores the accepted receipt", async () => {
  const requests: any[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(async (path: string, init: RequestInit) => {
      if (path.endsWith("/invitations")) {
        const r = JSON.parse(String(init.body));
        requests.push(r);
        if (requests.length === 1) throw new TypeError("lost");
        return Response.json({ email: r.email, request_id: r.request_id });
      }
      return Response.json({ members: [] });
    }),
  );
  const user = userEvent.setup();
  const mount = () => render(<PersonalAdmin account="actor" recoveryScope={"a".repeat(64)} unavailable={vi.fn()} expired={vi.fn()} />);
  let page = mount();
  await user.click(screen.getByRole("button", { name: "회원 초대" }));
  expect(screen.getByRole("textbox", { name: "이름" })).toHaveFocus();
  await user.type(screen.getByRole("textbox", { name: "이름" }), "Guest");
  await user.type(
    screen.getByRole("textbox", { name: "이메일" }),
    "guest@example.test",
  );
  await user.click(screen.getByRole("button", { name: "초대 요청 등록" }));
  await screen.findByRole("alert");
  page.unmount();
  page = mount();
  expect(await screen.findByRole("textbox", { name: "이름" })).toHaveValue("Guest");
  expect(requests).toHaveLength(1);
  await user.click(
    await screen.findByRole("button", { name: "초대 접수 확인" }),
  );
  const receipt = await screen.findByRole("region", { name: "최근 초대" });
  expect(within(receipt).getByRole("status")).toHaveTextContent("guest@example.test 초대 요청을 등록했습니다.");
  await user.click(screen.getByRole("button", { name: "요청 번호 복사" }));
  expect(await navigator.clipboard.readText()).toBe(requests[0].request_id);
  await user.click(within(receipt).getByText("설치 서버에서 계정 준비하기"));
  await user.click(within(receipt).getByRole("button", { name: "계정 준비 명령 복사" }));
  expect(await navigator.clipboard.readText()).toContain(`--request-id ${requests[0].request_id} --uid "\${MEMBER_UID:?}"`);
  page.unmount();
  mount();
  expect(await screen.findByRole("region", { name: "최근 초대" })).toHaveTextContent(requests[0].request_id);
  expect(requests).toHaveLength(2);
  expect(requests[0]).toEqual(requests[1]);
  expect(Object.keys(requests[0]).sort()).toEqual([
    "email",
    "name",
    "request_id",
  ]);
});

it("protects the last available administrator and refreshes controls when another admin is ready", async () => {
  let alternateSuspended = true;
  const admin = { generation: 1, revision: 4, admin: true, eligible: true, ready: true };
  vi.stubGlobal("fetch", vi.fn(async () => Response.json({ members: [
    { ...admin, account_id: id, email: "admin@example.test", suspended: false },
    { ...admin, account_id: "alternate", email: "alternate@example.test", suspended: alternateSuspended },
  ] })));
  const user = userEvent.setup();
  render(<PersonalAdmin account={id} expired={vi.fn()} unavailable={vi.fn()} />);
  await screen.findByText(/현재 이용 가능한 유일한 관리자/);
  const actorRow = within(screen.getByText("admin@example.test").closest("li")!);
  const otherRow = within(screen.getByText("alternate@example.test").closest("li")!);
  await user.click(otherRow.getByText("계정 관리"));
  expect(otherRow.getByRole("button", { name: "정지 해제" })).toBeEnabled();
  await user.click(actorRow.getByText("계정 관리"));
  expect(actorRow.getByRole("button", { name: "일반 회원으로 변경" })).toBeDisabled();
  expect(actorRow.getByRole("button", { name: "계정 정지" })).toBeDisabled();

  alternateSuspended = false;
  await user.click(screen.getByRole("button", { name: "목록 새로고침" }));
  await otherRow.findByText("로그인 가능", { exact: true });
  expect(actorRow.getByRole("button", { name: "일반 회원으로 변경" })).toBeEnabled();
  expect(actorRow.getByRole("button", { name: "계정 정지" })).toBeEnabled();
});

it("searches and filters the roster and keeps the server request ID available after an invitation", async () => {
  const baseline = { generation: 1, revision: 1, admin: false, eligible: true, ready: true, suspended: false };
  const requestId = "8bd0a15f-461b-43a2-abdc-1954e7bd38ae";
  const fetcher = vi.fn(async () => Response.json({ members: [
    { ...baseline, account_id: "ready", email: "user@example.test", display_name: "준비 회원" },
    { ...baseline, account_id: "pending", email: "guest@example.test", ready: false, eligible: false, invitation_name: "손님", invitation_request_id: requestId },
    { ...baseline, account_id: "suspended", email: "blocked@example.test", suspended: true },
  ] }));
  vi.stubGlobal("fetch", fetcher);
  render(<PersonalAdmin account="actor" expired={vi.fn()} unavailable={vi.fn()} />);
  const user = userEvent.setup();
  const search = await screen.findByRole("searchbox", { name: "회원 검색" });
  await user.type(search, "준비 회원");
  expect(screen.getAllByRole("listitem")).toHaveLength(1);
  expect(screen.getByText("user@example.test")).toBeInTheDocument();
  await user.clear(search);
  await user.type(search, "EXAMPLE");
  await user.click(screen.getByRole("button", { name: "계정 준비 1" }));
  expect(screen.getAllByRole("listitem")).toHaveLength(1);
  expect(screen.getByText("손님")).toBeInTheDocument();
  await user.click(screen.getByText("초대 확인"));
  await user.click(screen.getByRole("button", { name: "요청 번호 복사" }));
  expect(await navigator.clipboard.readText()).toBe(requestId);
  await user.type(search, "UNKNOWN");
  expect(screen.getByText("조건에 맞는 회원이 없습니다.")).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "검색과 필터 초기화" }));
  expect(screen.getAllByRole("listitem")).toHaveLength(3);
  expect(fetcher).toHaveBeenCalledTimes(1);
});

it.each([
  ["promote", "관리자로 지정", "관리자로 지정했습니다."],
  ["demote", "일반 회원으로 변경", "일반 회원으로 변경했습니다."],
  ["suspend", "계정 정지", "계정을 정지했습니다."],
  ["resume", "정지 해제", "계정 정지를 해제했습니다. 회원은 다시 로그인해야 합니다."],
])("reopens %s after the server applies it but the reply is lost", async (action, label, message) => {
  const changes: Record<string, unknown>[] = [];
  const common = { generation: 2, revision: 6, ready: true, eligible: true, suspended: false, admin: false };
  const target = { ...common, account_id: id, email: "member@example.test", admin: action === "demote", suspended: action === "resume" };
  vi.stubGlobal("fetch", vi.fn(async (url: string, init: RequestInit) => {
    if (url.endsWith("/change")) {
      changes.push(JSON.parse(String(init.body)));
      if (changes.length === 1) {
        if (action === "demote" || action === "promote") target.admin = action === "promote";
        else target.suspended = action === "suspend";
        target.revision++;
        throw new TypeError("lost response");
      }
      return Response.json(target);
    }
    return Response.json({ members: [{ ...common, account_id: "actor", email: "admin@example.test", admin: true }, target] });
  }));
  const mount = (scope: string) => render(<PersonalAdmin account="actor" recoveryScope={scope} unavailable={vi.fn()} expired={vi.fn()} />);
  const user = userEvent.setup();
  const first = mount("a".repeat(64));
  const row = within((await screen.findByText(target.email)).closest("li")!);
  await user.click(row.getByText("계정 관리"));
  await user.click(row.getByRole("button", { name: label }));
  await user.click(within(screen.getByRole("dialog")).getByRole("button", { name: label }));
  await screen.findByRole("alert");
  first.unmount();
  const second = mount("a".repeat(64));
  await screen.findByText("2명 표시");
  expect(changes).toHaveLength(1);
  await user.click(screen.getByRole("button", { name: "변경 결과 확인" }));
  expect(await screen.findByText(`${target.email} · ${message}`)).toBeInTheDocument();
  expect(changes[1]).toEqual(changes[0]);
  expect(changes[1]).toMatchObject({ revision: 6, generation: 2, action });
  second.unmount();
  const third = mount("a".repeat(64));
  expect(await screen.findByText(`${target.email} · ${message}`)).toBeInTheDocument();
  third.unmount();
  mount("b".repeat(64));
  await screen.findByText("2명 표시");
  expect(screen.queryByText(`${target.email} · ${message}`)).toBeNull();
});

it("opens the existing installation Google setup from the roster", async () => {
  vi.stubGlobal("fetch", vi.fn(async () => Response.json({ members: [] })));
  const open = vi.fn();
  render(<PersonalAdmin account="actor" openGoogleSetup={open} expired={vi.fn()} unavailable={vi.fn()} />);
  await userEvent.setup().click(await screen.findByRole("button", { name: "Google Workspace 설정 열기" }));
  expect(open).toHaveBeenCalledOnce();
});
