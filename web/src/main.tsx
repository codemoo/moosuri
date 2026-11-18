import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Theme } from "@astryxdesign/core/theme";
import { neutralTheme } from "@astryxdesign/theme-neutral/built";
import "@astryxdesign/core/reset.css";
import "@astryxdesign/core/astryx.css";
import "@astryxdesign/theme-neutral/theme.css";
import App from "./App";
import { PwaStatus } from "./PwaStatus";
import { registerPublicShell } from "./pwa";
import "./styles.css";
import "./workspace.css";

// WebKit may allow pinch scaling despite the viewport directive.
function preventTouchPageZoom(event: Event) {
  if (window.matchMedia("(pointer: coarse)").matches) event.preventDefault();
}
for (const gesture of ["gesturestart", "gesturechange"]) {
  document.addEventListener(gesture, preventTouchPageZoom, { passive: false, capture: true });
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Theme theme={neutralTheme} mode="dark">
      <App />
      <PwaStatus />
    </Theme>
  </StrictMode>,
);
void registerPublicShell();
