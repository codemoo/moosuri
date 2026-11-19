import { useEffect, useState } from "react";
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

// Replace the OAuth callback entry after its fragment has been captured.
export function restoreConnectorAddress(service: string | null) {
  const url = new URL(location.href);
  url.search = "";
  url.hash = screenName({ view: "connections", service: service ?? undefined });
  window.history.replaceState(null, "", url);
}

export function useMemberRoute(admin: boolean, initialConnector: () => MemberRoute | null) {
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
      if (back && window.history.state?.memberParent === target) {
        window.history.back();
        return;
      }
      url.hash = hash;
      if (back) window.history.replaceState(null, "", url);
      else window.history.pushState({ memberParent: previous }, "", url);
    }
    setRoute(next);
  }
  return { route, open };
}
