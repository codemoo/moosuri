import { useLayoutEffect, useRef } from "react";

// CSS owns the visible frame except while the browser overlays a keyboard.
export function useWorkspaceViewport() {
  const frame = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const element = frame.current;
    const viewport = window.visualViewport;
    if (!element || !viewport) return;
    const editable = "textarea, input:not([type]), input:is([type=text],[type=password],[type=search],[type=url],[type=email],[type=tel],[type=number]), [contenteditable=true], [contenteditable=plaintext-only]";
    const focusedEditor = () => {
      const active = document.activeElement;
      return active instanceof HTMLElement && element.contains(active) && active.matches(editable)
        ? active : null;
    };
    let request = 0;
    let visible = !document.hidden;
    const clear = () => {
      element.style.removeProperty("--workspace-height");
      element.style.removeProperty("--workspace-top");
    };
    const update = () => {
      request = 0;
      const height = viewport.height;
      const availablePan = document.documentElement.clientHeight - height;
      if (!visible || !focusedEditor() || Math.abs(viewport.scale - 1) > 0.01
        || !Number.isFinite(availablePan) || height <= 0 || availablePan < 1) {
        clear();
        return;
      }
      element.style.setProperty("--workspace-height", `${height}px`);
      element.style.setProperty("--workspace-top", `${Math.max(0, Math.min(availablePan, viewport.offsetTop || 0))}px`);
    };
    const schedule = () => {
      if (visible && !request) request = window.requestAnimationFrame(update);
    };
    const suspend = () => {
      visible = false;
      window.cancelAnimationFrame(request);
      request = 0;
      clear();
      // Close the old keyboard on app switch. The editor still owns its draft.
      focusedEditor()?.blur();
    };
    const resume = () => {
      visible = !document.hidden;
      schedule();
    };
    const visibility = () => {
      if (document.hidden) suspend();
      else resume();
    };
    viewport.addEventListener("resize", schedule);
    viewport.addEventListener("scroll", schedule);
    window.addEventListener("resize", schedule);
    window.addEventListener("focus", schedule);
    window.addEventListener("pagehide", suspend);
    window.addEventListener("pageshow", resume);
    document.addEventListener("focusin", schedule);
    document.addEventListener("focusout", schedule);
    document.addEventListener("visibilitychange", visibility);
    update();
    return () => {
      window.cancelAnimationFrame(request);
      viewport.removeEventListener("resize", schedule);
      viewport.removeEventListener("scroll", schedule);
      window.removeEventListener("resize", schedule);
      window.removeEventListener("focus", schedule);
      window.removeEventListener("pagehide", suspend);
      window.removeEventListener("pageshow", resume);
      document.removeEventListener("focusin", schedule);
      document.removeEventListener("focusout", schedule);
      document.removeEventListener("visibilitychange", visibility);
      clear();
    };
  }, []);
  return frame;
}
