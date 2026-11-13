import { useSyncExternalStore } from "react";
import { installApp, installReady, subscribeInstall } from "./pwa";

export function PwaInstallButton() {
  const available = useSyncExternalStore(subscribeInstall, installReady, () => false);
  return available ? <button className="nav-link" onClick={() => void installApp()}>
    앱으로 설치<span aria-hidden="true">↗</span>
  </button> : null;
}
