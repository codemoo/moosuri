import { useLayoutEffect, useRef } from "react";

// Mobile browsers can resize only the visual viewport when the keyboard opens.
// Keep the application frame inside that viewport without disabling pinch zoom.
export function useWorkspaceViewport() {
  const frame = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const element = frame.current;
    const viewport = window.visualViewport;
    if (!element || !viewport) return;
    let request = 0;
    const update = () => {
      request = 0;
      // A pinch-zoomed visual viewport is a magnifying window, not a new layout.
      if (Math.abs(viewport.scale - 1) > 0.01) return;
      if (Number.isFinite(viewport.height) && viewport.height > 0) {
        element.style.setProperty("--workspace-height", `${viewport.height}px`);
        element.style.setProperty("--workspace-top", `${Math.max(0, viewport.offsetTop)}px`);
      }
    };
    const schedule = () => {
      if (!request) request = window.requestAnimationFrame(update);
    };
    update();
    viewport.addEventListener("resize", schedule);
    viewport.addEventListener("scroll", schedule);
    window.addEventListener("resize", schedule);
    return () => {
      window.cancelAnimationFrame(request);
      viewport.removeEventListener("resize", schedule);
      viewport.removeEventListener("scroll", schedule);
      window.removeEventListener("resize", schedule);
      element.style.removeProperty("--workspace-height");
      element.style.removeProperty("--workspace-top");
    };
  }, []);
  return frame;
}
