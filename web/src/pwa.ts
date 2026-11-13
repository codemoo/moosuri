/// <reference types="vite/client" />
type InstallPrompt = Event & { prompt(): Promise<void>; userChoice: Promise<{ outcome: "accepted" | "dismissed" }> };
let prompt: InstallPrompt | null = null;
let listening = false;
const listeners = new Set<() => void>();
const changed = () => { for (const notify of listeners) notify(); };
export const installReady = () => prompt !== null;
export const subscribeInstall = (listener: () => void) => { listeners.add(listener); return () => { listeners.delete(listener); }; };
export async function installApp() {
  const current = prompt;
  if (!current) return;
  prompt = null; changed();
  try { await current.prompt(); await current.userChoice; }
  catch { /* The browser owns install permission and may withdraw availability. */ }
}
function listenForInstallation() {
  if (listening) return;
  listening = true;
  window.addEventListener("beforeinstallprompt", event => {
    event.preventDefault(); prompt = event as InstallPrompt; changed();
  });
  window.addEventListener("appinstalled", () => { prompt = null; changed(); });
}
export async function registerPublicShell(): Promise<ServiceWorkerRegistration | null> {
  if (!import.meta.env.PROD || !window.isSecureContext || !("serviceWorker" in navigator)) return null;
  listenForInstallation();
  try {
    return await navigator.serviceWorker.register("/service-worker.js", { scope: "/", updateViaCache: "none" });
  } catch {
    // Installation is optional; normal online use must remain available.
    return null;
  }
}
