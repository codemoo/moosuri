import { useEffect, useState } from "react";
import type { SessionView } from "../shared/api-types";
import type { MemberView } from "./PersonalWorkspaceChrome";

export type MemberRoute = {
  view: MemberView;
  engine?: boolean;
  service?: string;
  setup?: boolean;
};

function readRoute(admin: boolean): MemberRoute {
  const screen = location.hash.slice(1);
  const [view, detail, setup] = screen.split("/");
  if (view === "connections") {
    const service = ["gmail", "drive", "docs", "slides", "sheets", "vault"].includes(detail) ? detail : undefined;
    return { view, service, setup: admin && (detail === "setup" || (!!service && setup === "setup")) };
  }
  if (view === "admin" && admin) return { view, engine: detail === "engine" };
  return { view: view === "memory" ? "memory" : "chat" };
}

function screenName(route: MemberRoute) {
  if (route.view === "admin" && route.engine) return "admin/engine";
  if (route.view === "connections") return [route.view, route.service, route.setup && "setup"].filter(Boolean).join("/");
  return route.view;
}

type ConnectorOrigin = "chat" | "memory" | "admin" | "admin/engine";
function connectorOrigin(value: unknown): ConnectorOrigin | undefined {
  if (value === "chat" || value === "memory" || value === "admin" || value === "admin/engine") return value;
}

function memberEntry(session: SessionView) {
  const entry = window.history.state;
  return session.chat_draft_scope && entry?.account === session.user.id && entry?.scope === session.chat_draft_scope
    ? entry : undefined;
}

export function currentConnectorOrigin(session: SessionView) {
  return connectorOrigin(memberEntry(session)?.connectorOrigin);
}

function entryFor(session: SessionView, origin?: ConnectorOrigin, parent?: string) {
  return { account: session.user.id, scope: session.chat_draft_scope, connectorOrigin: origin, memberParent: parent };
}

// The callback is a fresh document. Its previous browser entry may belong to
// Google, so restore a logical parent without asking the browser to go back.
export function restoreConnectorAddress(service: string | null, session: SessionView, origin?: unknown) {
  const url = new URL(location.href);
  url.search = "";
  url.hash = screenName({ view: "connections", service: service ?? undefined });
  window.history.replaceState(entryFor(session, connectorOrigin(origin)), "", url);
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
    else open({ view: origin === "memory" ? "memory" : "chat" }, true);
  }
  return { route, open, leaveCatalog };
}
