/** One origin/service-worker subscription; ownership never expires mid-mutation. */
const name = "moosuri-push-subscription-v1";
export const supportsPushBrowserLock = () =>
  typeof navigator.locks?.request === "function";
export async function withPushBrowserLock<T>(
  work: () => Promise<T>,
): Promise<T> {
  if (!supportsPushBrowserLock())
    throw new Error("Push browser locking unavailable");
  return await navigator.locks.request(name, { mode: "exclusive" }, work);
}
