import type { BrowserFrame } from "../shared/api-types";

export type BrowserInput =
  | { kind: "click"; x: number; y: number }
  | { kind: "insert_text"; text: string }
  | { kind: "key"; key: string }
  | { kind: "scroll"; delta_x: number; delta_y: number };

export function browserFrameSource(frame: Pick<BrowserFrame, "png_base64">) {
  return `data:image/png;base64,${frame.png_base64}`;
}

export function validBrowserFrame(frame: BrowserFrame) {
  return Number.isInteger(frame.width) && Number.isInteger(frame.height) &&
    frame.width > 0 && frame.height > 0 && !!frame.frame_token &&
    /^[A-Za-z0-9+/=]+$/.test(frame.png_base64);
}

export function browserLocation(text?: string | null) {
  try {
    if (!text || text.length > 2048) return null;
    const url = new URL(text);
    return ["http:", "https:"].includes(url.protocol) && !url.username && !url.password
      ? url : null;
  } catch {
    return null;
  }
}

export function browserClick(frame: BrowserFrame, image: HTMLImageElement, clientX: number, clientY: number): BrowserInput | null {
  const bounds = image.getBoundingClientRect();
  if (!bounds.width || !bounds.height) return null;
  return {
    kind: "click",
    x: Math.min(frame.width - 1, Math.max(0, Math.floor((clientX - bounds.left) * frame.width / bounds.width))),
    y: Math.min(frame.height - 1, Math.max(0, Math.floor((clientY - bounds.top) * frame.height / bounds.height))),
  };
}

export function browserScroll(x: number, y: number): BrowserInput {
  const clamp = (value: number) => Math.max(-2000, Math.min(2000, Math.round(value)));
  return { kind: "scroll", delta_x: clamp(x), delta_y: clamp(y) };
}

export const browserKeys: Readonly<Partial<Record<string, string>>> = {
  Enter: "enter",
  Tab: "tab",
  Backspace: "backspace",
  Escape: "escape",
  ArrowUp: "arrow_up",
  ArrowDown: "arrow_down",
  ArrowLeft: "arrow_left",
  ArrowRight: "arrow_right",
};
