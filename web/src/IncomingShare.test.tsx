import { beforeEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import { IDBFactory } from "fake-indexeddb";
import { IncomingShare, shareFragment } from "./IncomingShare";
import { bindShareSession, fenceIncomingShares, readShareRow, stageShare } from "./share-state";
const scope = "a".repeat(64);
const owner = { account: "synthetic-a", scope };
const network = vi.fn(async (...args: unknown[]) => args[0] === "/api/v1/shares/origin" ? new Response(null, { status: 204 }) : new Response(JSON.stringify({ user: { id: owner.account }, chat_draft_scope: scope })));
beforeEach(async () => { vi.stubGlobal("indexedDB", new IDBFactory()); vi.stubGlobal("fetch", network); network.mockReset(); network.mockImplementation(async (...args: unknown[]) => args[0] === "/api/v1/shares/origin" ? new Response(null, { status: 204 }) : new Response(JSON.stringify({ user: { id: owner.account }, chat_draft_scope: scope }))); await fenceIncomingShares(); });
async function staged() {
  const row = await bindShareSession(await readShareRow(), owner);
  const id = await stageShare(row, { title: "Synthetic title", text: "<script>inert()</script>", url: "https://example.invalid/no-fetch" });
  window.history.replaceState(null, "", `/app#share=${id}`);
  return id;
}
function Fixture({ slot = "new", moveSlot, sessionScope = scope }: { slot?: string; sessionScope?: string; moveSlot?: { current: ((from: string, to: string) => void) | null } }) {
  const [draft, setDraft] = useState("existing draft");
  return <><textarea aria-label="Ordinary draft" value={draft} onChange={e => setDraft(e.target.value)}/>
    <IncomingShare accountId={owner.account} sessionScope={sessionScope} slot={slot} moveSlot={moveSlot} draft={() => draft} insert={(_, text) => setDraft(text)}/></>;
}
it("explicit import reveals inert editable text, explicit insertion appends latest draft once and never sends", async () => {
  await staged(); render(<Fixture/>);
  expect(screen.queryByRole("textbox", { name: "공유 글 편집" })).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: "이 계정의 초안으로 가져오기" }));
  const shared = await screen.findByRole("textbox", { name: "공유 글 편집" });
  expect((shared as HTMLTextAreaElement).value).toContain("<script>inert()</script>");
  expect(document.querySelectorAll("script")).toHaveLength(0);
  expect(screen.queryByRole("link")).not.toBeInTheDocument();
  fireEvent.change(shared, { target: { value: "edited share" } });
  fireEvent.change(screen.getByRole("textbox", { name: "Ordinary draft" }), { target: { value: "typed meanwhile" } });
  fireEvent.keyDown(shared, { key: "Enter" });
  fireEvent.click(screen.getByRole("button", { name: "초안에 넣기" }));
  await waitFor(() => expect(screen.getByRole("textbox", { name: "Ordinary draft" })).toHaveValue("typed meanwhile\n\nedited share"));
  expect((await readShareRow()).pending).not.toHaveProperty("payload");
  expect(network.mock.calls.every(call => ["/api/v1/auth/session", "/api/v1/shares/origin"].includes(String(call[0])))).toBe(true);
  expect(network.mock.calls.filter(call => call[0] === "/api/v1/shares/origin")).toHaveLength(3);
  expect(window.location.hash).toBe("");
});
it("changing threads prevents insertion until the original slot is explicitly selected", async () => {
  await staged(); const view = render(<Fixture/>);
  fireEvent.click(screen.getByRole("button", { name: "이 계정의 초안으로 가져오기" }));
  await screen.findByRole("textbox", { name: "공유 글 편집" });
  view.rerender(<Fixture slot="other-thread"/>);
  expect(screen.getByRole("button", { name: "초안에 넣기" })).toBeDisabled();
  expect(screen.getByRole("textbox", { name: "Ordinary draft" })).toHaveValue("existing draft");
  view.rerender(<Fixture/>);
  expect(screen.getByRole("button", { name: "초안에 넣기" })).toBeEnabled();
});
it("reload/back cannot transfer a claim to a fresh window or recover plaintext", async () => {
  await staged(); const first = render(<Fixture/>);
  fireEvent.click(screen.getByRole("button", { name: "이 계정의 초안으로 가져오기" }));
  await screen.findByRole("textbox", { name: "공유 글 편집" });
  first.unmount(); render(<Fixture/>);
  fireEvent.click(screen.getByRole("button", { name: "이 계정의 초안으로 가져오기" }));
  await screen.findByText("공유를 가져올 수 없습니다. 로그인 상태를 확인한 뒤 다시 공유해 주세요.");
  expect(screen.queryByRole("textbox", { name: "공유 글 편집" })).not.toBeInTheDocument();
});
it("fresh auth failure reveals no payload and has no draft write", async () => {
  await staged(); network.mockRejectedValueOnce(new Error("offline")); render(<Fixture/>);
  fireEvent.click(screen.getByRole("button", { name: "이 계정의 초안으로 가져오기" }));
  await screen.findByText("공유를 가져올 수 없습니다. 로그인 상태를 확인한 뒤 다시 공유해 주세요.");
  expect(screen.queryByRole("textbox", { name: "공유 글 편집" })).not.toBeInTheDocument();
  expect(screen.getByRole("textbox", { name: "Ordinary draft" })).toHaveValue("existing draft");
});
it("UTF-8 overflow remains editable and cannot overwrite or submit the normal draft", async () => {
  await staged(); render(<Fixture/>);
  fireEvent.click(screen.getByRole("button", { name: "이 계정의 초안으로 가져오기" }));
  const shared = await screen.findByRole("textbox", { name: "공유 글 편집" });
  fireEvent.change(shared, { target: { value: "한".repeat(2731) } });
  expect(screen.getByRole("button", { name: "초안에 넣기" })).toBeDisabled();
  expect(shared).toBeEnabled();
  expect(screen.getByRole("textbox", { name: "Ordinary draft" })).toHaveValue("existing draft");
});
it("rejects payload-bearing, malformed and extra fragment parameters", () => {
  expect(shareFragment("#share=not-an-id")).toBeNull();
  expect(shareFragment("#share=10000000-0000-4000-8000-000000000001&text=private")).toBeNull();
});

it("new-slot review follows the explicit thread creation migration", async () => {
  await staged();
  const moveSlot = { current: null as ((from: string, to: string) => void) | null };
  const view = render(<Fixture moveSlot={moveSlot}/>);
  fireEvent.click(screen.getByRole("button", { name: "이 계정의 초안으로 가져오기" }));
  await screen.findByRole("textbox", { name: "공유 글 편집" });
  moveSlot.current!("new", "created-thread");
  view.rerender(<Fixture slot="created-thread" moveSlot={moveSlot}/>);
  expect(screen.getByRole("button", { name: "초안에 넣기" })).toBeEnabled();
});

function independentContext() {
  const module = { exports: {} as typeof import("./share-state") };
  const code = ts.transpileModule(readFileSync("src/share-state.ts", "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText;
  runInNewContext(code, { module, exports: module.exports, indexedDB, crypto, TextEncoder, structuredClone });
  return module.exports;
}
/** Defer the actual claim/consume transaction's completion after its row commits.
 * Other transactions use the same fake IDB. No BroadcastChannel hint is delivered. */
function delayCommit(state: "claimed" | "consumed") {
  const actual = indexedDB;
  let release!: () => void, announce!: () => void;
  const committed = new Promise<void>(resolve => { announce = resolve; });
  let once = false;
  vi.stubGlobal("indexedDB", { open(...args: Parameters<IDBFactory["open"]>) {
    const request = actual.open(...args);
    return new Proxy(request, {
      get(target, key) {
        if (key === "result") return new Proxy(target.result, {
          get(database, property) {
            if (property === "transaction") return (...args: Parameters<IDBDatabase["transaction"]>) => {
              const tx = database.transaction(...args);
              let completion: ((event: Event) => unknown) | null = null;
              let held = false;
              return new Proxy(tx, {
                get(transaction, property) {
                  if (property === "objectStore") return (name: string) => new Proxy(transaction.objectStore(name), {
                    get(store, property) {
                      if (property === "put") return (row: { pending?: { state: string } }, key: IDBValidKey) => {
                        if (!once && row.pending?.state === state) { once = true; held = true; }
                        return store.put(row, key);
                      };
                      const value = Reflect.get(store, property, store); return typeof value === "function" ? value.bind(store) : value;
                    },
                  });
                  const value = Reflect.get(transaction, property, transaction); return typeof value === "function" ? value.bind(transaction) : value;
                },
                set(transaction, property, value) {
                  if (property === "oncomplete") {
                    completion = value;
                    transaction.oncomplete = event => { if (held) { release = () => completion?.(event); announce(); } else completion?.(event); };
                  } else Reflect.set(transaction, property, value, transaction);
                  return true;
                },
              });
            };
            const value = Reflect.get(database, property, database); return typeof value === "function" ? value.bind(database) : value;
          },
        });
        const value = Reflect.get(target, key, target); return typeof value === "function" ? value.bind(target) : value;
      },
      set(target, key, value) { Reflect.set(target, key, value, target); return true; },
    });
  } });
  return { committed, release: () => release() };
}
it("independent durable retirement after claim commit but before delivery never reveals a textbox", async () => {
  await staged(); const other = independentContext(); const delayed = delayCommit("claimed"); render(<Fixture/>);
  fireEvent.click(screen.getByRole("button", { name: "이 계정의 초안으로 가져오기" }));
  await delayed.committed;
  await other.fenceIncomingShares(true, scope, owner.account);
  delayed.release();
  await screen.findByText("공유를 가져올 수 없습니다. 로그인 상태를 확인한 뒤 다시 공유해 주세요.");
  expect(screen.queryByRole("textbox", { name: "공유 글 편집" })).not.toBeInTheDocument();
  expect(screen.getByRole("textbox", { name: "Ordinary draft" })).toHaveValue("existing draft");
});
it("independent durable retirement after consume commit but before delivery never inserts", async () => {
  await staged(); const other = independentContext(); render(<Fixture/>);
  fireEvent.click(screen.getByRole("button", { name: "이 계정의 초안으로 가져오기" }));
  await screen.findByRole("textbox", { name: "공유 글 편집" });
  const delayed = delayCommit("consumed");
  fireEvent.click(screen.getByRole("button", { name: "초안에 넣기" }));
  await delayed.committed;
  await other.fenceIncomingShares(true, scope, owner.account);
  delayed.release();
  await screen.findByText("공유를 가져올 수 없습니다. 로그인 상태를 확인한 뒤 다시 공유해 주세요.");
  expect(screen.getByRole("textbox", { name: "Ordinary draft" })).toHaveValue("existing draft");
  expect(network.mock.calls.every(call => ["/api/v1/auth/session", "/api/v1/shares/origin"].includes(String(call[0])))).toBe(true);
});
it("a remote original-session rejection never displays payload and clears the stored handoff", async () => {
  await staged();
  network.mockImplementation(async (...args: unknown[]) => args[0] === "/api/v1/shares/origin" ? new Response(null, { status: 403 }) : new Response(JSON.stringify({ user: { id: owner.account }, chat_draft_scope: scope })));
  render(<Fixture/>); fireEvent.click(screen.getByRole("button", { name: "이 계정의 초안으로 가져오기" }));
  await screen.findByText("공유를 가져올 수 없습니다. 로그인 상태를 확인한 뒤 다시 공유해 주세요.");
  expect(screen.queryByRole("textbox", { name: "공유 글 편집" })).not.toBeInTheDocument();
  expect((await readShareRow()).pending).toBeUndefined();
});

it("already displayed plaintext hides immediately on a retirement notification", async () => {
  await staged(); render(<Fixture/>);
  fireEvent.click(screen.getByRole("button", { name: "이 계정의 초안으로 가져오기" }));
  await screen.findByRole("textbox", { name: "공유 글 편집" });
  await fenceIncomingShares();
  await waitFor(() => expect(screen.queryByRole("textbox", { name: "공유 글 편집" })).not.toBeInTheDocument());
});

it("held server origin proof cannot disclose after independent generation retirement", async () => {
  await staged(); const other = independentContext();
  let release!: (response: Response) => void;
  network.mockImplementation(async (...args: unknown[]) => args[0] === "/api/v1/shares/origin"
    ? new Promise<Response>(resolve => { release = resolve; })
    : new Response(JSON.stringify({ user: { id: owner.account }, chat_draft_scope: scope })));
  render(<Fixture/>); fireEvent.click(screen.getByRole("button", { name: "이 계정의 초안으로 가져오기" }));
  await waitFor(() => expect(release).toBeTypeOf("function"));
  await other.fenceIncomingShares(true, scope, owner.account);
  release(new Response(null, { status: 204 }));
  await screen.findByText("공유를 가져올 수 없습니다. 로그인 상태를 확인한 뒤 다시 공유해 주세요.");
  expect(screen.queryByRole("textbox", { name: "공유 글 편집" })).not.toBeInTheDocument();
});

it.each([1, 3])("actual same-account cookie changes between final session GET and origin proof %i", async switchAt => {
  await staged();
  let actualCookieScope = scope, originCalls = 0;
  network.mockImplementation(async (...args: unknown[]) => {
    if (args[0] !== "/api/v1/shares/origin") return new Response(JSON.stringify({ user: { id: owner.account }, chat_draft_scope: actualCookieScope }));
    const input = JSON.parse((args[1] as RequestInit).body as string);
    if (++originCalls === switchAt) actualCookieScope = "b".repeat(64);
    // The R2 endpoint lacked this field and accepted an original valid row under
    // any same-owner session. R3 must send the exact expected current namespace.
    return new Response(null, { status: input.expected_current_scope !== undefined && input.expected_current_scope !== actualCookieScope ? 401 : 204 });
  });
  render(<Fixture/>); fireEvent.click(screen.getByRole("button", { name: "이 계정의 초안으로 가져오기" }));
  if (switchAt === 3) {
    await screen.findByRole("textbox", { name: "공유 글 편집" });
    fireEvent.click(screen.getByRole("button", { name: "초안에 넣기" }));
  }
  await screen.findByText("공유를 가져올 수 없습니다. 로그인 상태를 확인한 뒤 다시 공유해 주세요.");
  expect(screen.queryByRole("textbox", { name: "공유 글 편집" })).not.toBeInTheDocument();
  expect(screen.getByRole("textbox", { name: "Ordinary draft" })).toHaveValue("existing draft");
  expect(originCalls).toBe(switchAt);
});

it("unclaimed original A can explicitly import under fresh same-owner B without migrating a claim", async () => {
  await staged(); const renewed = "b".repeat(64);
  network.mockImplementation(async (...args: unknown[]) => {
    if (args[0] !== "/api/v1/shares/origin") return new Response(JSON.stringify({ user: { id: owner.account }, chat_draft_scope: renewed }));
    const input = JSON.parse((args[1] as RequestInit).body as string);
    return new Response(null, { status: input.origin_scope === scope && input.expected_current_scope === renewed ? 204 : 401 });
  });
  render(<Fixture sessionScope={renewed}/>);
  fireEvent.click(screen.getByRole("button", { name: "이 계정의 초안으로 가져오기" }));
  await screen.findByRole("textbox", { name: "공유 글 편집" });
  fireEvent.click(screen.getByRole("button", { name: "초안에 넣기" }));
  await screen.findByText("초안에 넣었습니다. 내용을 확인하고 보내기를 눌러 주세요.");
  expect((screen.getByRole("textbox", { name: "Ordinary draft" }) as HTMLTextAreaElement).value).toContain("existing draft\n\nSynthetic title");
});
