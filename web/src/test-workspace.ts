import { fireEvent, screen, within } from "@testing-library/react";

export function openWorkspaceMenu() {
  const trigger = screen.getByRole("button", { name: "Open conversations" });
  if (trigger.getAttribute("aria-expanded") !== "true") fireEvent.click(trigger);
  return within(screen.getByRole("dialog", { name: "Moosuri" }));
}
