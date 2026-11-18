import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { useWorkspaceViewport } from "./workspace-viewport";

const resize = new Event("resize");
let viewport: EventTarget & {height: number; offsetTop: number; scale: number};
let hidden: boolean;
let layoutHeight: number;
let frames: FrameRequestCallback[];
function AppFrame() {
  const frame = useWorkspaceViewport();
  return <div ref={frame} data-testid="frame"><textarea aria-label="Draft" defaultValue="keep this draft"/></div>;
}
function nextFrame() {
  act(() => frames.splice(0).forEach(callback => callback(0)));
}
function dimensions() {
  const style = screen.getByTestId("frame").style;
  return [style.getPropertyValue("--workspace-height"), style.getPropertyValue("--workspace-top")];
}
function editor() {
  return screen.getByRole("textbox", {name: "Draft"});
}
beforeEach(() => {
  frames = [];
  hidden = false;
  layoutHeight = 844;
  viewport = Object.assign(new EventTarget(), { height: 844, offsetTop: 0, scale: 1 });
  vi.stubGlobal("visualViewport", viewport);
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => (frames.push(callback), frames.length));
  vi.stubGlobal("cancelAnimationFrame", (id: number) => { if (id) frames[id - 1] = () => {}; });
  vi.spyOn(document, "hidden", "get").mockImplementation(() => hidden);
  vi.spyOn(document.documentElement, "clientHeight", "get").mockImplementation(() => layoutHeight);
});
afterEach(() => vi.unstubAllGlobals());

it("clears stale keyboard geometry on PWA background and preserves the draft after resume", () => {
  render(<AppFrame />);
  editor().focus();
  Object.assign(viewport, {height: 460, offsetTop: 40});
  viewport.dispatchEvent(resize);
  nextFrame();
  expect(dimensions()).toEqual(["460px", "40px"]);
  hidden = true;
  fireEvent(document, new Event("visibilitychange"));
  expect(dimensions()).toEqual(["", ""]);
  expect(editor()).not.toHaveFocus();
  hidden = false;
  fireEvent(document, new Event("visibilitychange"));
  fireEvent(window, new Event("focus"));
  nextFrame();
  expect(dimensions()).toEqual(["", ""]);
  expect(editor()).toHaveValue("keep this draft");
  editor().focus();
  Object.assign(viewport, {height: 512, offsetTop: 0});
  viewport.dispatchEvent(resize);
  nextFrame();
  expect(dimensions()).toEqual(["512px", "0px"]);
});

it("uses CSS after page-cache restoration and rotation, and drops an orphaned keyboard offset on blur", () => {
  render(<AppFrame />);
  editor().focus();
  Object.assign(viewport, {height: 500, offsetTop: 600});
  viewport.dispatchEvent(resize);
  nextFrame();
  expect(dimensions()).toEqual(["500px", "344px"]);
  fireEvent(window, new Event("pagehide"));
  fireEvent(window, new Event("pageshow"));
  layoutHeight = 390;
  Object.assign(viewport, {height: 390, offsetTop: 0});
  fireEvent(window, resize);
  nextFrame();
  expect(dimensions()).toEqual(["", ""]);
  editor().focus();
  viewport.height = 220;
  viewport.dispatchEvent(resize);
  nextFrame();
  expect(dimensions()).toEqual(["220px", "0px"]);
  editor().blur();
  nextFrame();
  expect(dimensions()).toEqual(["", ""]);
});
