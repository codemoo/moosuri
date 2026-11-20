import { useEffect, useState } from "react";
import type { SessionView } from "../shared/api-types";
import type { MemberView } from "./PersonalWorkspaceChrome";

export type MemberRoute = {
  view: MemberView;
  taskId?: string;
  engine?: boolean;
  service?: string;
  setup?: boolean;
};

const taskIdPattern = /^[0-9a-f]{8}-(?:[0-9a-f]{4}-){3}[0-9a-f]{12}$/i;

function readRoute(admin: boolean): MemberRoute {
  const screen = location.hash.slice(1);
  const [view, detail, setup] = screen.split("/");
  if (view === "tasks") return { view, taskId: taskIdPattern.test(detail ?? "") ? detail : undefined };
  if (view === "connections") {
    const service = ["gmail", "drive", "docs", "slides", "sheets", "vault"].includes(detail) ? detail : undefined;
    return { view, service, setup: admin && (detail === "setup" || (!!service && setup === "setup")) };
  }
  if (view === "admin" && admin) return { view, engine: detail === "engine" };
  return { view: view === "memory" ? "memory" : "chat" };
}

function screenName(route: MemberRoute) {
  if (route.view === "tasks" && route.taskId) return `tasks/${route.taskId}`;
  if (route.view === "admin" && route.engine) return "admin/engine";
  if (route.view === "connections") return [route.view, route.service, route.setup && "setup"].filter(Boolean).join("/");
  return route.view;
}

type ConnectorOrigin = "tasks" | `tasks/${string}` | "chat" | "memory" | "admin" | "admin/engine";
function connectorOrigin(value: unknown): ConnectorOrigin | undefined {
  if (typeof value === "string" && value.startsWith("tasks/") && taskIdPattern.test(value.slice(6))) return `tasks/${value.slice(6)}`;
  if (value === "tasks" || value === "chat" || value === "memory" || value === "admin" || value === "admin/engine") return value;
}

function memberEntry(session: SessionView) {
  const entry = window.history.state;
  return session.chat_draft_scope && entry?.account === session.user.id && entry?.scope === session.chat_draft_scope
    ? entry : undefined;
}

export function setupOpenedFromAdmin(session: SessionView) {
  const parent = memberEntry(session)?.memberParent;
  return parent === "admin" || parent === "admin/engine";
}

export function currentConnectorOrigin(session: SessionView) {
  return connectorOrigin(memberEntry(session)?.connectorOrigin);
}

function entryFor(session: SessionView, origin?: ConnectorOrigin, parent?: string) {
  return { account: session.user.id, scope: session.chat_draft_scope, connectorOrigin: origin, memberParent: parent };
}

// A verified callback is a new document, usually preceded by an external
// consent page. Give both native history and the in-app Back button the same
// two internal parents. The fragment is consumed before the first push.
export function restoreConnectorAddress(service: string | null, session: SessionView & { installation_admin?: boolean }, origin?: unknown) {
  const url = new URL(location.href);
  const fields = new URLSearchParams(url.hash.slice(1));
  const callback = fields.has("google_handle") || fields.has("google_denied");
  const detail = screenName({ view: "connections", service: service ?? undefined });
  url.search = "";
  let parent = connectorOrigin(origin);
  if ((parent === "admin" || parent === "admin/engine") && !session.installation_admin) parent = "chat";
  if (parent && callback) {
    url.hash = parent === "chat" ? "" : parent;
    window.history.replaceState(entryFor(session), "", url);
    url.hash = "connections";
    window.history.pushState(entryFor(session, parent, parent), "", url);
    if (service) {
      url.hash = detail;
      window.history.pushState(entryFor(session, parent, "connections"), "", url);
    }
  } else {
    url.hash = detail;
    window.history.replaceState((!callback && memberEntry(session)) || entryFor(session), "", url);
  }
}

export function useMemberRoute(session: SessionView, admin: boolean, initialConnector: () => MemberRoute | null) {
  const [route, setRoute] = useState<MemberRoute>(() => initialConnector() ?? readRoute(admin));
  useEffect(() => {
    const restore = () => setRoute(readRoute(admin));
    window.addEventListener("popstate", restore);
    return () => window.removeEventListener("popstate", restore);
  }, [admin]);

  function open(next: MemberRoute, back = false) {
    const url = new URL(location.href);
    const target = screenName(next);
    const previous = screenName(readRoute(admin));
    const hash = target === "chat" ? "" : "#" + target;
    if (url.hash !== hash) {
      if (back && memberEntry(session)?.memberParent === target) {
        window.history.back();
        return;
      }
      url.hash = hash;
      const origin = next.view === "connections"
        ? connectorOrigin(previous) ?? currentConnectorOrigin(session) : undefined;
      const state = entryFor(session, origin, back ? undefined : previous);
      if (back) window.history.replaceState(state, "", url);
      else window.history.pushState(state, "", url);
    }
    setRoute(next);
  }
  function leaveCatalog() {
    const origin = currentConnectorOrigin(session);
    if (admin && (origin === "admin" || origin === "admin/engine"))
      open({ view: "admin", engine: origin === "admin/engine" }, true);
    else if (origin === "tasks" || origin?.startsWith("tasks/"))
      open({ view: "tasks", taskId: origin.slice(6) || undefined }, true);
    else open({ view: origin === "memory" ? "memory" : "chat" }, true);
  }
  return { route, open, leaveCatalog };
}
