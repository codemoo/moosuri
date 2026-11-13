import {
  activatePushAccount as activate,
  clearPushBinding,
  deactivatePushBinding,
  isPushEnrollmentCurrent,
} from "./push-state";
import { supportsPushBrowserLock, withPushBrowserLock } from "./push-lock";
export async function closePushNotices(expectedAccount?: string) {
  try {
    const registration = await navigator.serviceWorker?.getRegistration("/");
    registration?.active?.postMessage({
      type: "moosuri-push-clear",
      expected_account: expectedAccount ?? null,
    });
    for (const notice of (await registration?.getNotifications()) ?? [])
      if (
        notice.tag.startsWith("moosuri-push-") &&
        (expectedAccount === undefined ||
          notice.data?.account_id === expectedAccount)
      )
        notice.close();
  } catch {}
}
export async function activatePushAccount(account: string) {
  const row = await activate(account);
  const closeForeign = async () => {
    if (!(await isPushEnrollmentCurrent(row))) return;
    const registration = await navigator.serviceWorker?.getRegistration("/");
    for (const notice of (await registration?.getNotifications()) ?? [])
      if (
        notice.tag.startsWith("moosuri-push-") &&
        notice.data?.account_id !== account
      )
        notice.close();
  };
  try {
    if (supportsPushBrowserLock()) await withPushBrowserLock(closeForeign);
    else await closeForeign();
  } catch {}
  return row;
}
export async function clearBrowserPushBinding(expectedAccount?: string) {
  try {
    const changed = await clearPushBinding(expectedAccount);
    if (changed) {
      if (supportsPushBrowserLock())
        await withPushBrowserLock(() => closePushNotices(expectedAccount));
      else await closePushNotices(expectedAccount);
    }
    return changed;
  } catch {
    await closePushNotices(expectedAccount);
    return false;
  }
}

export async function deactivateBrowserPushBinding(account: string) {
  const operation = await deactivatePushBinding(account);
  await withPushBrowserLock(async () => {
    if (!(await isPushEnrollmentCurrent(operation))) return;
    await closePushNotices(account);
  });
  return operation;
}
