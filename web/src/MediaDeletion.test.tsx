import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { MediaLibraryView } from "./Media";

const id = "11111111-1111-4111-8111-111111111111";
const item = { id, revision: 1, state: "ready", digest: "synthetic", width: 32, height: 32, bytes: 123, created_at: 1 };
async function library() {
  let deleted = false;
  const pending: { resolve: (response: Response) => void; signal: AbortSignal }[] = [];
  const created = vi.fn(() => `blob:synthetic-${Math.random()}`);
  const revoked = vi.fn();
  Object.defineProperty(URL, "createObjectURL", { configurable: true, value: created });
  Object.defineProperty(URL, "revokeObjectURL", { configurable: true, value: revoked });
  const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
  vi.stubGlobal("fetch", vi.fn((input: string, init?: RequestInit) => {
    // Deliberately settle even after AbortSignal: cancellation alone is insufficient.
    if (input.endsWith("/content")) return new Promise<Response>((resolve) => pending.push({ resolve, signal: init!.signal as AbortSignal }));
    if (init?.method === "DELETE") { deleted = true; return Promise.resolve(new Response(null, { status: 204 })); }
    if (input.endsWith("/thumbnail")) return Promise.resolve(new Response(new Blob(["thumbnail"])));
    return Promise.resolve(new Response(JSON.stringify({ items: deleted ? [] : [item], used_bytes: 123, quota_bytes: 1024 }), { headers: { "content-type": "application/json" } }));
  }));
  const view = render(<MediaLibraryView accountId="account-a" onSelect={() => {}} />);
  fireEvent.click(screen.getByText("새로고침"));
  await screen.findByRole("img", { name: "첨부 이미지" });
  return { ...view, pending, created, revoked, click };
}

for (const mode of ["delete-ack", "another-view", "unmount"] as const) {
  it(`fences all late image downloads after ${mode}`, async () => {
    const view = await library();
    fireEvent.click(screen.getByText("다운로드"));
    fireEvent.click(screen.getByText("다운로드"));
    await waitFor(() => expect(view.pending).toHaveLength(2));
    if (mode === "delete-ack") {
      fireEvent.click(screen.getByText("이미지 삭제"));
      await screen.findByText("보관한 첨부가 없습니다.");
    } else if (mode === "another-view") {
      act(() => window.dispatchEvent(new CustomEvent("moosuri-media-deleted", { detail: id })));
      await screen.findByText("보관한 첨부가 없습니다.");
    } else view.unmount();
    expect(view.pending.every((request) => request.signal.aborted)).toBe(true);
    const before = view.created.mock.calls.length;
    await act(async () => {
      for (const request of view.pending) request.resolve(new Response(new Blob(["private image"])));
    });
    expect(view.click).not.toHaveBeenCalled();
    expect(view.created).toHaveBeenCalledTimes(before);
    expect(view.revoked).toHaveBeenCalled();
    if (mode === "another-view") {
      fireEvent.click(screen.getByText("새로고침"));
      await act(async () => {});
      expect(screen.queryByText("다운로드")).not.toBeInTheDocument();
    }
    view.click.mockRestore();
  });
}

it("keeps successful image downloads and releases their URL on unmount", async () => {
  const view = await library();
  fireEvent.click(screen.getByText("다운로드"));
  await waitFor(() => expect(view.pending).toHaveLength(1));
  await act(async () => view.pending[0].resolve(new Response(new Blob(["private image"]))));
  expect(view.click).toHaveBeenCalledTimes(1);
  const url = view.created.mock.results.at(-1)!.value;
  view.unmount();
  expect(view.revoked).toHaveBeenCalledWith(url);
  view.click.mockRestore();
});
