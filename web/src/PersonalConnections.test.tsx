// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const request = vi.fn();
vi.mock("./account-scope", () => ({ useScope: () => ({ request }) }));
vi.mock("./PersonalGoogle", () => ({
  googleReturn: () => false,
  usePersonalGoogle: () => ({ status: "disabled", retry() {}, busy: false, action() {}, connection: null, notice: undefined }),
}));
vi.mock("./personal-google-workspace", () => ({
  isWorkspaceGoogleService: () => false,
  workspaceGoogleReturn: () => null,
  usePersonalGoogleWorkspace: () => ({ status: "disabled", retry: async () => {}, busy: false, action() {}, connections: {}, notice: undefined }),
}));
vi.mock("./PersonalGoogleSetup", () => ({ PersonalGoogleSetup: () => null, installationGoogleSetupPath: "/setup" }));
vi.mock("./WorkspaceNavigation", () => ({ WorkspaceIcon: () => null }));
vi.mock("./member-route", () => ({ setupOpenedFromAdmin: () => false }));
vi.mock("./connector-marks/gmail.png?no-inline", () => ({ default: "mark.png" }));
vi.mock("./connector-marks/drive.png?no-inline", () => ({ default: "mark.png" }));
vi.mock("./connector-marks/docs.png?no-inline", () => ({ default: "mark.png" }));
vi.mock("./connector-marks/slides.png?no-inline", () => ({ default: "mark.png" }));
vi.mock("./connector-marks/sheets.png?no-inline", () => ({ default: "mark.png" }));
import { PersonalConnections } from "./PersonalConnections";

const ORIGIN = "https://vault.example.test";
const props: Parameters<typeof PersonalConnections>[0] = {
  session: {user: {id: "11111111-1111-4111-8111-111111111111", email: "member@example.test"}} as Parameters<typeof PersonalConnections>[0]["session"],
  expired: vi.fn(),
  onBack: vi.fn(),
  navigation: { route: { view: "connections", service: "vault", setup: false }, open: vi.fn() },
};

describe("personal vault connection", () => {
  const submitted: { action: string; target: string; handoff: string }[] = [];
  beforeEach(() => {
    request.mockReset();
    submitted.length = 0;
    vi.spyOn(HTMLFormElement.prototype, "submit").mockImplementation(function (this: HTMLFormElement) {
      submitted.push({ action: this.action, target: this.target, handoff: new FormData(this).get("handoff") as string });
    });
  });
  afterEach(() => { cleanup(); vi.restoreAllMocks(); });

  it("posts a short-lived handoff to the exact vault origin in a new window without standalone-login copy", async () => {
    const popup = { close: vi.fn() };
    vi.spyOn(window, "open").mockReturnValue(popup as never);
    request.mockImplementation(async (path: string) => path === "/personal/vault"
      ? { state: "ready", site_origin: ORIGIN }
      : { token: "ab".repeat(32), vault_origin: ORIGIN, expires_at: 1 });
    render(<PersonalConnections {...props} />);
    await userEvent.click(await screen.findByRole("button", { name: "볼트 열기" }));
    await waitFor(() => expect(submitted).toHaveLength(1));
    expect(submitted[0].action).toBe(`${ORIGIN}/session`);
    expect(submitted[0].handoff).toBe("ab".repeat(32));
    expect(submitted[0].target).toMatch(/^moosuri-vault-/);
    expect(window.open).toHaveBeenCalledWith("about:blank", submitted[0].target);
    expect(request).toHaveBeenCalledWith("/personal/vault/handoff", "POST", {});
    expect(screen.queryByText(/볼트 계정으로 로그인하세요/)).toBeNull();
    expect(popup.close).not.toHaveBeenCalled();
  });

  it("closes the window and submits nothing when the handoff names another origin", async () => {
    const popup = { close: vi.fn() };
    vi.spyOn(window, "open").mockReturnValue(popup as never);
    request.mockImplementation(async (path: string) => path === "/personal/vault"
      ? { state: "ready", site_origin: ORIGIN }
      : { token: "ab".repeat(32), vault_origin: "https://evil.example.test", expires_at: 1 });
    render(<PersonalConnections {...props} />);
    await userEvent.click(await screen.findByRole("button", { name: "볼트 열기" }));
    await waitFor(() => expect(popup.close).toHaveBeenCalled());
    expect(submitted).toHaveLength(0);
  });
});
