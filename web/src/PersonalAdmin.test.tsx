import { cleanup, render, screen } from "@testing-library/react";
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
  await user.click(screen.getByRole("button", { name: "회원 초대" }));
  await user.click(
    await screen.findByRole("button", { name: "초대 접수 확인" }),
  );
  await screen.findByText(/guest@example.test 계정 준비 요청/);
  expect(requests).toHaveLength(2);
  expect(requests[0]).toEqual(requests[1]);
  expect(Object.keys(requests[0]).sort()).toEqual([
    "email",
    "name",
    "request_id",
  ]);
});
