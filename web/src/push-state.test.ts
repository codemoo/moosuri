import { beforeEach, it, expect, vi } from "vitest";
import { IDBFactory } from "fake-indexeddb";
import {
  activatePushAccount,
  beginPushEnrollment,
  completePushEnrollment,
  clearPushBinding,
  readPushBinding,
  deactivatePushBinding,
  isPushEnrollmentCurrent,
} from "./push-state";
beforeEach(() => vi.stubGlobal("indexedDB", new IDBFactory()));
it("a held A enrollment and old A logout cannot replace or clear B's binding", async () => {
  await activatePushAccount("account-a");
  const a = await beginPushEnrollment("account-a");
  await activatePushAccount("account-b");
  const b = await beginPushEnrollment("account-b");
  expect(await completePushEnrollment(b, "binding-b")).toBe(true);
  expect(await completePushEnrollment(a, "binding-a")).toBe(false);
  expect(await clearPushBinding("account-a")).toBe(false);
  expect(await readPushBinding()).toMatchObject({
    account_id: "account-b",
    binding_id: "binding-b",
  });
});
it("a newer enrollment operation fences an earlier lost acknowledgement", async () => {
  await activatePushAccount("account-a");
  const first = await beginPushEnrollment("account-a");
  const next = await beginPushEnrollment("account-a");
  expect(await completePushEnrollment(first, "old")).toBe(false);
  expect(await completePushEnrollment(next, "new")).toBe(true);
  const row = await readPushBinding();
  expect(row?.binding_id).toBe("new");
  expect(Object.keys(row!).sort()).toEqual([
    "account_id",
    "binding_id",
    "device_id",
    "generation",
    "revision",
  ]);
});
it("logout preserves only the non-authorizing device identity and rotates the generation", async () => {
  await activatePushAccount("account-a");
  const first = await beginPushEnrollment("account-a");
  await completePushEnrollment(first, "old");
  await clearPushBinding("account-a");
  expect(await readPushBinding()).toMatchObject({
    device_id: first.device_id,
    account_id: null,
    binding_id: null,
  });
  expect((await readPushBinding())?.generation).not.toBe(first.generation);
});

it("explicit off preserves authenticated lifetime but logout still rejects re-enrollment", async () => {
  await activatePushAccount("a");
  const first = await beginPushEnrollment("a");
  await completePushEnrollment(first, "first");
  const off = await deactivatePushBinding("a");
  expect(off.account_id).toBe("a");
  expect(off.binding_id).toBeNull();
  const next = await beginPushEnrollment("a");
  expect(next.revision).toBeGreaterThan(off.revision);
  expect(off.revision).toBeGreaterThan(first.revision);
  expect(await isPushEnrollmentCurrent(first)).toBe(false);
  expect(await completePushEnrollment(next, "next")).toBe(true);
  await clearPushBinding("a");
  await expect(beginPushEnrollment("a")).rejects.toThrow();
  await expect(deactivatePushBinding("a")).rejects.toThrow();
  await activatePushAccount("a");
  expect(await isPushEnrollmentCurrent(next)).toBe(false);
});
it("independent tab operations allocate distinct monotonic revisions atomically", async () => {
  await activatePushAccount("a");
  const operations = await Promise.all([
    beginPushEnrollment("a"),
    beginPushEnrollment("a"),
    beginPushEnrollment("a"),
  ]);
  expect(new Set(operations.map((v) => v.revision)).size).toBe(3);
  expect(operations.map((v) => v.revision)).toEqual(
    [...operations.map((v) => v.revision)].sort((a, b) => a - b),
  );
  expect(await completePushEnrollment(operations[0], "old")).toBe(false);
  expect(await completePushEnrollment(operations[2], "new")).toBe(true);
  await activatePushAccount("b");
  await expect(deactivatePushBinding("a")).rejects.toThrow();
  expect((await readPushBinding())?.account_id).toBe("b");
});

it("revision exhaustion fails closed without changing the last safe binding", async () => {
  await activatePushAccount("a");
  const db = await new Promise<IDBDatabase>((resolve) => {
    const request = indexedDB.open("moosuri-push-binding-v1", 1);
    request.onsuccess = () => resolve(request.result);
  });
  const current = (await readPushBinding())!;
  await new Promise<void>((resolve) => {
    const tx = db.transaction("binding", "readwrite");
    tx.objectStore("binding").put(
      { ...current, revision: Number.MAX_SAFE_INTEGER },
      "device",
    );
    tx.oncomplete = () => resolve();
  });
  db.close();
  await expect(beginPushEnrollment("a")).rejects.toThrow();
  expect((await readPushBinding())?.revision).toBe(Number.MAX_SAFE_INTEGER);
  expect((await readPushBinding())?.generation).toBe(current.generation);
});
