import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Theme } from "@astryxdesign/core/theme";
import { neutralTheme } from "@astryxdesign/theme-neutral/built";
import "@astryxdesign/core/reset.css";
import "@astryxdesign/core/astryx.css";
import "@astryxdesign/theme-neutral/theme.css";
import "./styles.css";
import "./workspace.css";
import "./public-information.css";
import "./touch-viewport";
import { PublicInformation, resolvePublicInformationPath } from "./PublicInformation";
import PersonalInvitation, { takeInvitationToken } from "./PersonalInvitation";

const root = createRoot(document.getElementById("root")!);
const publicPage = resolvePublicInformationPath(window.location.pathname);

if (window.location.pathname === "/join") {
  const token = takeInvitationToken(window.location, window.history);
  root.render(<StrictMode><Theme theme={neutralTheme} mode="dark"><PersonalInvitation token={token}/></Theme></StrictMode>);
} else if (publicPage) {
  // Public pages never load the member application or contact any API.
  root.render(
    <StrictMode>
      <Theme theme={neutralTheme} mode="dark">
        <PublicInformation page={publicPage} />
      </Theme>
    </StrictMode>,
  );
} else {
  void import("./PersonalPreview").then(({ default: PersonalPreview }) => {
    root.render(
      <StrictMode>
        <Theme theme={neutralTheme} mode="dark">
          <PersonalPreview />
        </Theme>
      </StrictMode>,
    );
  });
}
