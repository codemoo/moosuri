import { StrictMode } from "react";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import type { CommunitySkillCard } from "../shared/api-types";
import { CommunitySkills } from "./CommunitySkills";
const base: CommunitySkillCard = {
  id: "source-briefing", name: "Saved briefing", description: "Synthetic saved-source procedure",
  source: "https://github.com/codemoo/moosuri", source_commit: "1".repeat(40), version: "1.0.0",
  digest: "a".repeat(64), license: "Apache-2.0", capabilities: ["knowledge_search"],
  installed_version: "1.0.0", installed_digest: "a".repeat(64), enabled: true, hidden: false,
  revision: 1, can_rollback: false, readiness: "ready", missing: [],
};
const remote: CommunitySkillCard = {
  ...base, id: "github-" + "b".repeat(64), name: "External synthetic procedure",
  github: { repository: "https://github.com/example/recipes", directory: "recipe" },
  digest: "c".repeat(64), installed_digest: null, installed_version: null, enabled: false, revision: 0,
  readiness: "installable",
};
const installed = { ...remote, installed_digest: remote.digest, installed_version: remote.version, enabled: true, revision: 1, readiness: "ready" };
const props = () => ({ onExpired: vi.fn(), back: vi.fn(), connect: vi.fn(), start: vi.fn() });
const response = (value: unknown) => new Response(JSON.stringify(value), { status: 200, headers: { "Content-Type": "application/json" } });
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>(yes => { resolve = yes; }); return { promise, resolve }; }
async function preview() {
  fireEvent.click(screen.getByRole("button", { name: "GitHub 원본에서 추가" }));
  fireEvent.change(screen.getByLabelText("저장소 URL"), { target: { value: remote.github!.repository } });
  fireEvent.change(screen.getByLabelText("패키지 디렉터리"), { target: { value: remote.github!.directory } });
  fireEvent.click(screen.getByRole("button", { name: "원본 확인" }));
  return screen.findByRole("button", { name: "추가" });
}
function article(name: string) { return screen.getByRole("heading", { level: 3, name }).closest("article")!; }
beforeEach(() => vi.restoreAllMocks());
it("retains a successful imported card when the older initial catalog arrives afterward", async () => {
  const oldList = deferred<Response>(); const mutations: RequestInit[] = [];
  vi.stubGlobal("fetch", vi.fn(async (path: string, init?: RequestInit) => {
    if (init?.method === "GET") return oldList.promise;
    if (path.endsWith("/preview")) return response(remote);
    mutations.push(init!); return response([installed]);
  }));
  const callbacks = props(); render(<StrictMode><CommunitySkills accountId="a" {...callbacks} /></StrictMode>);
  fireEvent.click(await preview());
  await screen.findByRole("heading", { name: remote.name, level: 3 });
  await act(async () => oldList.resolve(response([])));
  expect(within(article(remote.name)).getByRole("button", { name: "삭제" })).toBeEnabled();
  expect(within(article(remote.name)).getByRole("button", { name: "비활성화" })).toBeEnabled();
  expect(mutations).toHaveLength(1);
  expect(JSON.parse(String(mutations[0].body))).toEqual({ action: "install", expected_revision: 0, expected_digest: remote.digest });
  expect(callbacks.start).not.toHaveBeenCalled();
});
it.each(["parent_first", "import_first"])("serializes full catalog lifecycle/import results: %s", async order => {
  const held = deferred<Response>(); let writes = 0;
  vi.stubGlobal("fetch", vi.fn(async (path: string, init?: RequestInit) => {
    if (init?.method === "GET") return response([base]);
    if (path.endsWith("/preview")) return response(remote);
    writes++; return writes === 1 ? held.promise : response([installed]);
  }));
  render(<CommunitySkills accountId="a" {...props()} />);
  await screen.findByRole("heading", { name: base.name, level: 3 });
  const add = await preview(); const remove = within(article(base.name)).getByRole("button", { name: "삭제" });
  act(() => {
    (order === "parent_first" ? remove : add).click();
    (order === "parent_first" ? add : remove).click();
  });
  expect(writes).toBe(1); expect(remove).toBeDisabled(); expect(add).toBeDisabled();
  expect(screen.getByRole("button", { name: "목록 새로고침" })).toBeDisabled();
  await act(async () => held.resolve(response(order === "parent_first" ? [] : [base, installed])));
  if (order === "parent_first") {
    await waitFor(() => expect(add).toBeEnabled()); fireEvent.click(add);
  } else {
    await waitFor(() => expect(remove).toBeEnabled()); fireEvent.click(remove);
  }
  await waitFor(() => expect(writes).toBe(2));
  await waitFor(() => expect(screen.queryByRole("heading", { name: base.name, level: 3 })).toBeNull());
  expect(within(article(remote.name)).getByRole("button", { name: "삭제" })).toBeEnabled();
});
it("does not resurrect a removed card from an older explicit refresh snapshot", async () => {
  const oldList = deferred<Response>(); let reads = 0;
  vi.stubGlobal("fetch", vi.fn(async (_path: string, init?: RequestInit) => {
    if (init?.method === "GET") return ++reads === 1 ? response([base]) : oldList.promise;
    return response([]);
  }));
  render(<CommunitySkills accountId="a" {...props()} />);
  await screen.findByRole("heading", { name: base.name, level: 3 });
  fireEvent.click(screen.getByRole("button", { name: "목록 새로고침" }));
  fireEvent.click(within(article(base.name)).getByRole("button", { name: "삭제" }));
  await waitFor(() => expect(screen.queryByRole("heading", { name: base.name, level: 3 })).toBeNull());
  await act(async () => oldList.resolve(response([base])));
  expect(screen.queryByRole("heading", { name: base.name, level: 3 })).toBeNull();
});
it("fences an importer mutation result after switching the actual parent account", async () => {
  const held = deferred<Response>();
  vi.stubGlobal("fetch", vi.fn(async (path: string, init?: RequestInit) => {
    if (init?.method === "GET") return response([]);
    if (path.endsWith("/preview")) return response(remote);
    return held.promise;
  }));
  const callbacks = props(); const view = render(<CommunitySkills accountId="a" {...callbacks} />);
  fireEvent.click(await preview()); view.rerender(<CommunitySkills accountId="b" {...callbacks} />);
  await act(async () => held.resolve(response([installed])));
  expect(screen.queryByRole("heading", { name: remote.name, level: 3 })).toBeNull();
  expect(screen.queryByLabelText("저장소 URL")).toBeNull(); expect(callbacks.start).not.toHaveBeenCalled();
});
