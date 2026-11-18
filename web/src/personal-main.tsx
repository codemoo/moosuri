import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import PersonalPreview from "./PersonalPreview";
createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <PersonalPreview />
  </StrictMode>,
);
