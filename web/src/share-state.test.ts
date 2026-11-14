import { beforeEach, expect, it, vi } from "vitest";
import { IDBFactory } from "fake-indexeddb";
import { appendShare, bindShareSession, claimShare, consumeShare, fenceIncomingShares, readShareRow, SHARE_TTL, shareEpoch, stageShare } from "./share-state";
const a = { account: "synthetic-a", scope: "a".repeat(64) };
const b = { account: "synthetic-b", scope: "b".repeat(64) };
const payload = { title: "Synthetic title", text: "<script>inert()</script>", url: "https://example.invalid/private" };
beforeEach(async () => { vi.useRealTimers(); vi.stubGlobal("indexedDB", new IDBFactory()); await fenceIncomingShares(); });
async function prepared() {
  const row = await bindShareSession(await readShareRow(), a);
  const id = await stageShare(row, payload);
  return { row, id };
}
it("committed pending handoff survives module restart, but only one racing window claims plaintext", async () => {
  const { id } = await prepared();
  vi.resetModules();
  const restarted = await import("./share-state");
  const row = await restarted.readShareRow();
  const results = await Promise.allSettled([1, 2].map(() => restarted.claimShare(row, id, crypto.randomUUID(), a)));
  expect(results.filter(x => x.status === "fulfilled")).toHaveLength(1);
  const result = results.find(x => x.status === "fulfilled") as PromiseFulfilledResult<Awaited<ReturnType<typeof claimShare>>>;
  expect(result.value.payload).toEqual(payload);
  await expect(restarted.claimShare(row, id, crypto.randomUUID(), a)).rejects.toThrow();
});
it("consume erases the payload, preserves a TTL tombstone, and replay/reload cannot claim", async () => {
  const { row, id } = await prepared();
  const claim = await claimShare(row, id, crypto.randomUUID(), a);
  await consumeShare(claim);
  expect((await readShareRow()).pending).toMatchObject({ id, state: "consumed" });
  expect((await readShareRow()).pending).not.toHaveProperty("payload");
  await expect(consumeShare(claim)).rejects.toThrow();
  await expect(claimShare(row, id, crypto.randomUUID(), a)).rejects.toThrow();
  const next = await stageShare(await readShareRow(), payload);
  expect(next).not.toBe(id);
  expect((await readShareRow()).tombstones).toContainEqual({ id, expires: claim.expires });
});
it("ordinary expiry remains opaque until same-owner fresh login; old session cannot claim", async () => {
  const { row, id } = await prepared();
  const expired = await bindShareSession(row, null);
  await expect(claimShare(expired, id, crypto.randomUUID(), a)).rejects.toThrow();
  const renewed = { ...a, scope: "c".repeat(64) };
  const current = await bindShareSession(expired, renewed);
  expect((await claimShare(current, id, crypto.randomUUID(), renewed)).payload).toEqual(payload);
});
it("alternate account destroys pending payload and old generation cannot repopulate", async () => {
  const { row, id } = await prepared();
  const next = await bindShareSession(row, b);
  expect(next.pending).toBeUndefined();
  await expect(stageShare(row, payload)).rejects.toThrow();
  await expect(claimShare(next, id, crypto.randomUUID(), b)).rejects.toThrow();
});
it("synchronous logout token and durable CAS reject late bind/stage/consume from windows and worker", async () => {
  const { row, id } = await prepared();
  const token = shareEpoch();
  const claim = await claimShare(row, id, crypto.randomUUID(), a);
  const cleared = fenceIncomingShares(true);
  await expect(stageShare(row, payload, token)).rejects.toThrow();
  await cleared;
  await expect(bindShareSession(row, a, shareEpoch())).rejects.toThrow();
  await expect(consumeShare(claim)).rejects.toThrow();
  const now = await readShareRow();
  expect(now.pending).toBeUndefined();
  // Cookie may still be valid while logout HTTP waits: retired scope stays blocked.
  await expect(bindShareSession(now, a)).rejects.toThrow();
});
it("expired or future-clock handoff is erased without revealing plaintext", async () => {
  const { id } = await prepared();
  vi.spyOn(Date, "now").mockReturnValue(Date.now() + SHARE_TTL + 1);
  await expect(claimShare(await readShareRow(), id, crypto.randomUUID(), a)).rejects.toThrow();
  expect((await readShareRow()).pending).toBeUndefined();
});
it("new deliberate share replaces abandoned claim; stale consumer never inserts old bytes", async () => {
  const { row, id } = await prepared();
  const claim = await claimShare(row, id, crypto.randomUUID(), a);
  const next = await stageShare(await readShareRow(), { ...payload, text: "replacement" });
  await expect(consumeShare(claim)).rejects.toThrow();
  expect((await claimShare(await readShareRow(), next, crypto.randomUUID(), a)).payload.text).toBe("replacement");
});
it("unbound device cannot stage and append uses latest draft with an enforced UTF-8 byte limit", async () => {
  await expect(stageShare(await readShareRow(), payload)).rejects.toThrow();
  expect(appendShare("typed meanwhile", "edited share")).toBe("typed meanwhile\n\nedited share");
  expect(appendShare("x".repeat(8190), "한")).toBeNull();
  expect(appendShare("", " ")).toBeNull();
});

it("a synchronous ordinary lifetime fence irreversibly retires an already disclosed claim", async () => {
  const { row, id } = await prepared();
  const claim = await claimShare(row, id, crypto.randomUUID(), a);
  await fenceIncomingShares();
  await expect(consumeShare(claim)).rejects.toThrow();
});
it("clock rollback removes future-created payload without allowing import", async () => {
  const { id } = await prepared();
  vi.spyOn(Date, "now").mockReturnValue(Date.now() - 1000);
  expect((await readShareRow()).pending).toBeUndefined();
  await expect(claimShare(await readShareRow(), id, crypto.randomUUID(), a)).rejects.toThrow();
});

it("an old account's delayed clear cannot erase another account's newer handoff", async () => {
  const { row } = await prepared();
  const next = await bindShareSession(row, b);
  const id = await stageShare(next, payload);
  await fenceIncomingShares(true, a.scope, a.account);
  expect((await readShareRow()).pending?.id).toBe(id);
  expect((await readShareRow()).account).toBe(b.account);
});

it("expiry preserves the last authenticated origin namespace for a share received while expired", async () => {
  const initial = await bindShareSession(await readShareRow(), a);
  const expired = await bindShareSession(initial, null);
  expect(expired.scope).toBeNull();
  expect(expired.originScope).toBe(a.scope);
  const id = await stageShare(expired, payload);
  expect((await readShareRow()).pending?.originalScope).toBe(a.scope);
  const next = await bindShareSession(await readShareRow(), { ...a, scope: "c".repeat(64) });
  const claim = await claimShare(next, id, crypto.randomUUID(), { ...a, scope: "c".repeat(64) });
  expect(claim.originalScope).toBe(a.scope);
});
