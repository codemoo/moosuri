import { useEffect, useRef, useState } from "react";
import type { SessionView } from "../shared/api-types";
import { currentConnectorOrigin, restoreConnectorAddress } from "./member-route";
import { useScope } from "./account-scope";
import { ApiFailure, isAborted } from "./api";

const base = "/personal/google";
const storageKey = "moosuri.personal.google-workspace.v1";
const flowPattern = /^[a-f0-9]{64}$/;
const services = ["drive", "docs", "slides", "sheets"] as const;
type WorkspaceService = typeof services[number];
type Grant = { connection_id: string; epoch: number; mailbox: string };
type Connections = Partial<Record<WorkspaceService, Grant>>;
type Pending = {
  origin?: ReturnType<typeof currentConnectorOrigin>;
  account: string;
  session: string;
  service: WorkspaceService;
  flow: string;
  expires: number;
  phase: "consent" | "completing";
};
type WorkspaceReturn = { service: WorkspaceService | null; fields?: URLSearchParams };

export function isWorkspaceGoogleService(value: string | null): value is WorkspaceService {
  return services.some((service) => value === service);
}

function pendingFor(session: SessionView): Pending | null {
  try {
    const p: Pending | null = JSON.parse(sessionStorage.getItem(storageKey) ?? "null");
    if (p && p.account === session.user.id && session.chat_draft_scope &&
        p.session === session.chat_draft_scope && p.expires > Date.now() &&
        isWorkspaceGoogleService(p.service) && flowPattern.test(p.flow)) return p;
  } catch { /* Unavailable or malformed storage cannot authorize completion. */ }
  return null;
}

export function clearWorkspaceGoogle() {
  try { sessionStorage.removeItem(storageKey); } catch { /* Storage unavailable. */ }
}

export function workspaceGoogleReturn(session: SessionView): WorkspaceReturn | null {
  const fields = new URLSearchParams(location.hash.slice(1));
  if (fields.has("google_handle") || fields.has("google_denied")) {
    if (!fields.has("google_service")) return null; // Existing Gmail callback.
    const service = fields.get("google_service");
    return { service: fields.getAll("google_service").length === 1 && isWorkspaceGoogleService(service) ? service : null, fields };
  }
  const pending = pendingFor(session);
  return pending?.phase === "completing" ? { service: pending.service } : null;
}

export function usePersonalGoogleWorkspace({ session, expired, enabled, returned }: {
  session: SessionView;
  expired: () => void;
  enabled: boolean;
  returned: WorkspaceReturn | null;
}) {
  const scope = useScope(session.user.id, session.chat_draft_scope);
  const [connections, setConnections] = useState<Connections>({});
  const [status, setStatus] = useState<"checking" | "ready" | "disabled" | "error">("checking");
  const [busy, setBusy] = useState(true);
  const [notice, setNotice] = useState<{ service: WorkspaceService | null; message: string } | null>(null);
  const lock = useRef(false);
  const handled = useRef(false);

  function authError(error: unknown) {
    if (isAborted(error)) return true;
    if (error instanceof ApiFailure && error.status === 401) {
      expired();
      return true;
    }
    return false;
  }

  async function refresh() {
    setStatus("checking");
    try {
      const result = await scope.request<{ connections: (Grant & { service: string })[] }>(base + "/workspace");
      const current: Connections = {};
      for (const { service, connection_id, epoch, mailbox } of result.connections) {
        if (isWorkspaceGoogleService(service)) current[service] = { connection_id, epoch, mailbox };
      }
      setConnections(current);
      setStatus("ready");
    } catch (error) {
      if (authError(error)) return;
      setConnections({});
      setStatus(error instanceof ApiFailure && error.status === 404 ? "disabled" : "error");
    }
  }

  async function finish({ service, fields }: WorkspaceReturn) {
    const pending = pendingFor(session);
    const notify = (message: string) => setNotice({ service, message });
    const uncertain = "Google 연결 결과를 확인하지 못했습니다. 현재 연결 상태를 다시 확인해 주세요.";
    if (!fields) {
      restoreConnectorAddress(service, session, pending?.origin);
      clearWorkspaceGoogle();
      notify(uncertain);
      return;
    }

    const handles = fields.getAll("google_handle");
    const denials = fields.getAll("google_denied");
    const flows = fields.getAll("flow");
    const flow = flows[0] ?? denials[0];
    const matches = !!service && pending?.service === service && pending.phase === "consent" && pending.flow === flow;
    restoreConnectorAddress(service, session, matches ? pending.origin ?? "chat" : undefined);
    if (!matches) {
      notify("연결을 시작한 계정과 탭에서 다시 진행해 주세요.");
      return;
    }
    if (denials.length === 1 && handles.length === 0 && (flows.length === 0 || (flows.length === 1 && denials[0] === flow))) {
      clearWorkspaceGoogle();
      notify("Google 연결이 취소됐습니다.");
      return;
    }
    if (handles.length !== 1 || denials.length !== 0 || flows.length !== 1 || !flowPattern.test(handles[0])) {
      notify("Google 연결 정보를 확인할 수 없습니다.");
      return;
    }

    try {
      sessionStorage.setItem(storageKey, JSON.stringify({ ...pending, phase: "completing" }));
      const grant = await scope.request<Grant & { flow_id: string }>(`${base}/${service}/complete`, "POST", { handle: handles[0] }, undefined, { timeoutMs: 75000 });
      if (grant.flow_id !== flow) throw Error("flow mismatch");
      clearWorkspaceGoogle();
    } catch (error) {
      if (authError(error)) throw error;
      clearWorkspaceGoogle();
      notify(uncertain);
    }
  }

  useEffect(() => {
    if (!enabled) return;
    const timer = window.setTimeout(() => void (async () => {
      setBusy(true);
      try {
        if (returned && !handled.current) {
          handled.current = true;
          await finish(returned);
        }
        await refresh();
      } catch (error) {
        if (!isAborted(error)) setStatus("error");
      } finally {
        setBusy(false);
      }
    })(), 0);
    return () => clearTimeout(timer);
  }, [scope, enabled, returned]);

  async function action(service: WorkspaceService, name: "disconnect") {
    if (busy || lock.current || status !== "ready") return;
    const grant = connections[service];
    lock.current = true;
    setBusy(true);
    setNotice(null);
    try {
      if (name === "disconnect" && grant) {
        await scope.request(`${base}/${service}/disconnect`, "POST", {
          connection_id: grant.connection_id, epoch: grant.epoch,
        }, undefined, { timeoutMs: 15000 });
        setConnections((current) => ({ ...current, [service]: undefined }));
      }
    } catch (error) {
      if (authError(error)) return;
      setNotice({ service, message: "Google 연결 해제 결과를 확인하지 못했습니다." });
      await refresh();
    } finally {
      lock.current = false;
      setBusy(false);
    }
  }

  async function retry() {
    if (busy || lock.current) return;
    lock.current = true;
    setBusy(true);
    setNotice(null);
    try { await refresh(); }
    finally {
      lock.current = false;
      setBusy(false);
    }
  }

  return { connections, status, busy, notice, action, retry };
}
