import { useEffect, useState } from "react";
import "./pwa.css";

export function PwaStatus() {
  const [offline, setOffline] = useState(!navigator.onLine);
  useEffect(() => {
    const update = () => setOffline(!navigator.onLine);
    window.addEventListener("offline", update);
    window.addEventListener("online", update);
    return () => {
      window.removeEventListener("offline", update);
      window.removeEventListener("online", update);
    };
  }, []);
  return offline ? <div className="offline-status" role="status" aria-live="polite">
    <span aria-hidden="true">◌</span> 인터넷 연결이 끊겼어요. 연결되면 다시 이어갈 수 있어요.
  </div> : null;
}
