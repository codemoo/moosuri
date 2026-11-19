import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, expect, it, vi } from "vitest";
import PersonalAdmin from "./PersonalAdmin";
const id = "e8675fc3-aaf7-42d5-97e4-a88e9ef28679";
afterEach(() => {
  cleanup();
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
  vi.spyOn(window, "confirm").mockReturnValue(true);
  const user = userEvent.setup();
  render(
    <PersonalAdmin account="actor" expired={() => {}} unavailable={() => {}} />,
  );
  await user.click(
    await screen.findByRole("button", { name: "관리자로 지정" }),
  );
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

it("submits only invitation metadata and retries a lost registration with its original id", async () => {
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
  render(
    <PersonalAdmin account="actor" unavailable={() => {}} expired={() => {}} />,
  );
  await user.type(screen.getByRole("textbox", { name: "이름" }), "Guest");
  await user.type(
    screen.getByRole("textbox", { name: "이메일" }),
    "guest@example.test",
  );
  await user.click(screen.getByRole("button", { name: "초대 요청 등록" }));
  await user.click(
    await screen.findByRole("button", { name: "초대 접수 확인" }),
  );
  await screen.findByText(/guest@example.test 초대 요청을 등록했습니다/);
  await user.click(screen.getByRole("button", { name: "요청 번호 복사" }));
  expect(await navigator.clipboard.readText()).toBe(requests[0].request_id);
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
  expect(actorRow.getByRole("button", { name: "일반 회원으로 변경" })).toBeDisabled();
  expect(actorRow.getByRole("button", { name: "계정 정지" })).toBeDisabled();
  expect(screen.getByRole("button", { name: "정지 해제" })).toBeEnabled();

  alternateSuspended = false;
  await user.click(screen.getByRole("button", { name: "목록 새로고침" }));
  await screen.findByText("관리자", { exact: true });
  expect(actorRow.getByRole("button", { name: "일반 회원으로 변경" })).toBeEnabled();
  expect(actorRow.getByRole("button", { name: "계정 정지" })).toBeEnabled();
});
