import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import type { CommunitySkillCard } from "../shared/api-types";
import { GitHubSkillImport } from "./GitHubSkillImport";
const fixture = (extra: Partial<CommunitySkillCard> = {}): CommunitySkillCard => ({
  id: "github-" + "a".repeat(64), name: "source-briefing", description: "External synthetic instructions",
  source: "https://github.com/example/recipes/tree/" + "1".repeat(40) + "/skills/source-briefing",
  source_commit: "1".repeat(40), version: "1.0.0", digest: "b".repeat(64), license: "Apache-2.0",
  capabilities: ["mail_read"], installed_version: null, installed_digest: null, enabled: false, hidden: false,
  revision: 0, can_rollback: false, readiness: "connection_needed", missing: ["mail_read"],
  github: { repository: "https://github.com/example/recipes", directory: "skills/source-briefing" }, ...extra,
});
const response = (value: unknown) => new Response(JSON.stringify(value), { status: 200, headers: { "Content-Type": "application/json" } });
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(yes => { resolve = yes; }); return { promise, resolve }; }
function fill() {
  fireEvent.change(screen.getByLabelText("저장소 URL"), { target: { value: "https://github.com/example/recipes" } });
  fireEvent.change(screen.getByLabelText("패키지 디렉터리"), { target: { value: "skills/source-briefing" } });
}
beforeEach(() => vi.restoreAllMocks());
it("previews source and missing connections then delegates one explicit activation to the parent", async () => {
  const install = vi.fn(async () => true), connect = vi.fn();
  const fetcher = vi.fn(async (_path: string, _init?: RequestInit) => response(fixture()));
  vi.stubGlobal("fetch", fetcher);
  render(<GitHubSkillImport accountId="synthetic-a" onExpired={vi.fn()} install={install} mutationBusy={false} connect={connect} />);
  fill(); fireEvent.click(screen.getByRole("button", { name: "원본 확인" }));
  await screen.findByRole("link", { name: "고정한 원본 보기" });
  expect(fetcher).toHaveBeenCalledTimes(1); expect(install).not.toHaveBeenCalled();
  expect(JSON.parse(String(fetcher.mock.calls[0][1]?.body))).toEqual({ repository: "https://github.com/example/recipes", directory: "skills/source-briefing", reference: "main" });
  fireEvent.click(screen.getByRole("button", { name: "Gmail 연결하기" })); expect(connect).toHaveBeenCalledOnce();
  fireEvent.click(screen.getByRole("button", { name: "추가" }));
  await waitFor(() => expect(install).toHaveBeenCalledOnce()); expect(install).toHaveBeenCalledWith(fixture());
  // The importer never sends a competing catalog mutation or starts work itself.
  expect(fetcher).toHaveBeenCalledTimes(1);
});
it("discards a late preview after editing the selected repository", async () => {
  const late = deferred<Response>(); vi.stubGlobal("fetch", vi.fn(() => late.promise));
  render(<GitHubSkillImport accountId="a" onExpired={vi.fn()} install={vi.fn()} mutationBusy={false} connect={vi.fn()} />);
  fill(); fireEvent.click(screen.getByRole("button", { name: "원본 확인" }));
  fireEvent.change(screen.getByLabelText("저장소 URL"), { target: { value: "https://github.com/another/recipes" } });
  await act(async () => late.resolve(response(fixture())));
  expect(screen.queryByLabelText("GitHub 절차 미리보기")).toBeNull(); expect(screen.getByLabelText("저장소 URL")).toHaveValue("https://github.com/another/recipes");
});
it("account switch fences a held preview and clears source inputs", async () => {
  const late = deferred<Response>(), install = vi.fn(); vi.stubGlobal("fetch", vi.fn(() => late.promise));
  const props = { onExpired: vi.fn(), install, mutationBusy: false, connect: vi.fn() };
  const view = render(<GitHubSkillImport accountId="a" {...props} />); fill();
  fireEvent.click(screen.getByRole("button", { name: "원본 확인" })); view.rerender(<GitHubSkillImport accountId="b" {...props} />);
  await act(async () => late.resolve(response(fixture())));
  expect(install).not.toHaveBeenCalled(); expect(screen.getByLabelText("저장소 URL")).toHaveValue(""); expect(screen.queryByText("External synthetic instructions")).toBeNull();
});
it("retains exact update preview metadata and respects the shared parent mutation lock", async () => {
  const preview = fixture({ revision: 7, installed_digest: "c".repeat(64), installed_version: "0.9.0", enabled: false, hidden: true });
  vi.stubGlobal("fetch", vi.fn(async () => response(preview))); const install = vi.fn(async () => true);
  const props = { accountId: "a", onExpired: vi.fn(), install, connect: vi.fn() };
  const view = render(<GitHubSkillImport {...props} mutationBusy={false} />); fill();
  fireEvent.click(screen.getByRole("button", { name: "원본 확인" })); await screen.findByRole("button", { name: "이 버전으로 업데이트" });
  view.rerender(<GitHubSkillImport {...props} mutationBusy={true} />);
  expect(screen.getByRole("button", { name: "이 버전으로 업데이트" })).toBeDisabled();
  fireEvent.click(screen.getByRole("button", { name: "이 버전으로 업데이트" })); expect(install).not.toHaveBeenCalled();
  view.rerender(<GitHubSkillImport {...props} mutationBusy={false} />);
  fireEvent.click(screen.getByRole("button", { name: "이 버전으로 업데이트" })); await waitFor(() => expect(install).toHaveBeenCalledWith(preview));
});
