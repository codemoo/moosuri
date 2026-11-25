import { useEffect, useRef, useState } from "react";
import type { Command, Summary } from "./types";

const extensionId = "lcpppplcklhmbmmfionndhkklkaogpma";
type Port = {
  postMessage(message: unknown): void;
  disconnect(): void;
  onMessage: { addListener(fn: (message: { status?: string }) => void): void };
  onDisconnect: { addListener(fn: () => void): void };
};
type ChromeRuntime = { connect(id: string, options: { name: string }): Port; lastError?: unknown };

export function GoogleAutofill({ item, email, run }: {
  item: Summary; email: string; run: <T>(command: Command) => Promise<T>;
}) {
  const port = useRef<Port | null>(null);
  const heartbeat = useRef<number | undefined>(undefined);
  const [message, setMessage] = useState("");
  const [armed, setArmed] = useState(false);
  const [installer, setInstaller] = useState(false);
  function stop() {
    window.clearInterval(heartbeat.current);
    const before = port.current;
    port.current = null;
    before?.disconnect();
  }
  useEffect(() => {
    // A package is served only on installations that built it for their specific vault.
    const controller = new AbortController();
    void fetch("/google-autofill.zip", { method: "HEAD", signal: controller.signal, cache: "no-store" })
      .then(response => { if (!controller.signal.aborted) setInstaller(response.ok && response.headers.get("content-type")?.split(";")[0] === "application/zip"); })
      .catch(() => {});
    window.addEventListener("pagehide", stop);
    return () => { stop(); controller.abort(); window.removeEventListener("pagehide", stop); };
  }, []);
  function arm() {
    if (port.current) { stop(); setArmed(false); setMessage(""); return; }
    const runtime = (window as Window & { chrome?: { runtime?: ChromeRuntime } }).chrome?.runtime;
    const missing = "이 Chrome에 무수리 볼트 확장을 설치한 뒤 다시 시도하세요.";
    if (!runtime?.connect) { setMessage(missing); return; }
    let channel: Port;
    try { channel = runtime.connect(extensionId, { name: "google-login" }); }
    catch { setMessage(missing); return; }
    port.current = channel;
    setArmed(true);
    let started = false, spent = false;
    const finish = (text: string) => {
      if (port.current !== channel) return;
      stop(); setArmed(false); setMessage(text);
    };
    channel.onDisconnect.addListener(() => {
      void runtime.lastError;
      finish(started ? "입력 요청이 끝났습니다. 필요하면 다시 시작하세요." : missing);
    });
    channel.onMessage.addListener(async reply => {
      if (port.current !== channel) return;
      if (reply.status === "armed") {
        started = true;
        window.clearInterval(heartbeat.current);
        heartbeat.current = window.setInterval(() => {
          if (port.current === channel) {
            try { channel.postMessage({ kind: "ping" }); } catch { finish("확장 연결이 끊겼습니다. 다시 시작하세요."); }
          }
        }, 20_000);
        setMessage("2분 안에 이 계정의 Google 비밀번호 화면에서 무수리 볼트 확장 버튼을 누르세요.");
      } else if (reply.status === "busy") {
        finish("다른 볼트 창에 입력 요청이 있습니다. 그 요청을 마치거나 취소하세요.");
      } else if (reply.status === "request" && started && !spent) {
        spent = true;
        try {
          const value = await run<string>({ op: "google_fill", email, item: item.id, revision: item.revision });
          if (port.current === channel) channel.postMessage({ kind: "credential", value });
        } catch { finish("볼트 항목이나 세션이 변경됐습니다. 항목을 다시 확인하세요."); }
      } else if (reply.status === "filled") finish("Google 비밀번호를 입력했습니다. Google 화면에서 로그인을 계속하세요.");
      else finish("이 계정의 Google 비밀번호 화면을 확인한 뒤 다시 시도하세요.");
    });
    try { channel.postMessage({ kind: "arm", email }); }
    catch { finish(missing); }
  }
  return <section className="vault-card" aria-label="Google 로그인">
    <button type="button" className="primary" onClick={arm}>{armed ? "입력 요청 취소" : "Google 비밀번호 자동 입력"}</button>
    {message && <p role="status">{message}</p>}
    {installer && <details><summary>Chrome 확장 설치</summary>
      <p>이 기기에 압축을 풀고 Chrome 확장 프로그램 관리에서 개발자 모드의 ‘압축해제된 확장 프로그램을 로드합니다’를 선택하세요.</p>
      <a href="/google-autofill.zip" download>이 볼트용 확장 다운로드</a>
    </details>}
  </section>;
}
