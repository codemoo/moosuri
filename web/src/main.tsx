import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { PwaStatus } from "./PwaStatus";
import { registerPublicShell } from "./pwa";
import "./styles.css";

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
    <PwaStatus />
  </StrictMode>,
);
void registerPublicShell();
