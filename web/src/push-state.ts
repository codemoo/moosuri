/** Minimal binding IDs only; no endpoint, auth key, notification body or API cache. */
export type PushBinding = {
  device_id: string;
  account_id: string | null;
  binding_id: string | null;
  generation: string;
  revision: number;
};
const name = "moosuri-push-binding-v1";
let currentAccount: string | null | undefined;
function database(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const request = indexedDB.open(name, 1);
    request.onupgradeneeded = () => request.result.createObjectStore("binding");
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(new Error("Push binding unavailable"));
    request.onblocked = () => reject(new Error("Push binding unavailable"));
  });
}
async function change<T>(
  work: (row: PushBinding) => { row: PushBinding; value: T },
): Promise<T> {
  const db = await database();
  return new Promise((resolve, reject) => {
    const tx = db.transaction("binding", "readwrite");
    const store = tx.objectStore("binding");
    const request = store.get("device");
    let value: T;
    request.onsuccess = () => {
      try {
        const old = request.result as PushBinding | undefined;
        const result = work(
          old ?? {
            device_id: crypto.randomUUID(),
            account_id: null,
            binding_id: null,
            generation: crypto.randomUUID(),
            revision: 0,
          },
        );
        value = result.value;
        store.put(result.row, "device");
      } catch {
        tx.abort();
      }
    };
    tx.oncomplete = () => {
      db.close();
      resolve(value);
    };
    tx.onerror = tx.onabort = () => {
      db.close();
      reject(new Error("Push binding unavailable"));
    };
  });
}
export async function readPushBinding(): Promise<PushBinding | null> {
  const db = await database();
  return new Promise((resolve, reject) => {
    const tx = db.transaction("binding", "readonly");
    const request = tx.objectStore("binding").get("device");
    request.onsuccess = () =>
      resolve((request.result as PushBinding | undefined) ?? null);
    request.onerror = () => reject(new Error("Push binding unavailable"));
    tx.oncomplete = () => db.close();
  });
}
function rotate(row: PushBinding): PushBinding {
  const revision = (row.revision ?? 0) + 1;
  if (!Number.isSafeInteger(revision) || revision < 1)
    throw new Error("Push intent exhausted");
  return {
    ...row,
    revision,
    binding_id: null,
    generation: crypto.randomUUID(),
  };
}
export async function isPushEnrollmentCurrent(
  operation: PushBinding,
): Promise<boolean> {
  const row = await readPushBinding();
  return (
    currentAccount === operation.account_id &&
    row?.account_id === operation.account_id &&
    row.device_id === operation.device_id &&
    row.generation === operation.generation &&
    row.revision === operation.revision
  );
}
/** Explicit off ends display authority, while the authenticated lifetime stays. */
export async function deactivatePushBinding(
  account: string,
): Promise<PushBinding> {
  return change((row) => {
    if (currentAccount !== account || row.account_id !== account)
      throw new Error("Stale account");
    const next = rotate(row);
    return { row: next, value: next };
  });
}
export async function activatePushAccount(
  account: string,
): Promise<PushBinding> {
  currentAccount = account;
  return change((row) => {
    if (currentAccount !== account) throw new Error("Stale account");
    const next =
      row.account_id === account
        ? row
        : {
            ...rotate(row),
            account_id: account,
          };
    return { row: next, value: next };
  });
}
export async function beginPushEnrollment(
  account: string,
): Promise<PushBinding> {
  return change((row) => {
    if (currentAccount !== account || row.account_id !== account)
      throw new Error("Stale account");
    const next = rotate(row);
    return { row: next, value: next };
  });
}
export async function completePushEnrollment(
  operation: PushBinding,
  binding: string,
): Promise<boolean> {
  return change((row) => {
    const valid =
      currentAccount === operation.account_id &&
      row.account_id === operation.account_id &&
      row.device_id === operation.device_id &&
      row.generation === operation.generation &&
      row.revision === operation.revision;
    return { row: valid ? { ...row, binding_id: binding } : row, value: valid };
  });
}
export async function clearPushBinding(
  expectedAccount?: string,
): Promise<boolean> {
  if (expectedAccount === undefined || currentAccount === expectedAccount)
    currentAccount = null;
  return change((row) => {
    const changed =
      expectedAccount === undefined || row.account_id === expectedAccount;
    return {
      row: changed
        ? {
            ...rotate(row),
            account_id: null,
          }
        : row,
      value: changed,
    };
  });
}
