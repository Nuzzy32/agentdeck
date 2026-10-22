// Keyboard shortcuts, UI-UX §7. Every one uses ⌘, so Ctrl/⌥ stay with the program in the terminal; ⌘C/⌘V are not
// mapped and keep copy/paste. ⌘J reaches the loop guard ribbon (cards join in T3.1); ⌘F is not mapped yet.
import type { Side } from "./tree";

export type Action =
  | { type: "newWorkspace" }
  | { type: "split"; dir: "row" | "col" }
  | { type: "close" }
  | { type: "focus"; side: Side }
  | { type: "workspace"; index: number }
  | { type: "workspaceStep"; delta: 1 | -1 }
  | { type: "zoom" }
  | { type: "sidebar" }
  | { type: "messages" }
  | { type: "jump" }
  | { type: "font"; delta: 1 | -1 | 0 }
  | { type: "settings" };

export type KeyInput = Pick<KeyboardEvent, "key" | "code" | "metaKey" | "shiftKey" | "altKey" | "ctrlKey">;

const ARROWS: Record<string, Side> = { ArrowLeft: "left", ArrowRight: "right", ArrowUp: "up", ArrowDown: "down" };

/** Maps a keydown to an app action, or null to leave it to the terminal or the browser. */
export function shortcut(e: KeyInput): Action | null {
  if (!e.metaKey || e.ctrlKey) return null;
  // Letters by `key` (follows the keyboard layout), digits and punctuation by `code` (Shift changes their `key`).
  const key = e.key.toLowerCase();
  if (e.altKey) {
    const side = ARROWS[e.key];
    return side && !e.shiftKey ? { type: "focus", side } : null;
  }
  if (e.shiftKey) {
    if (key === "d") return { type: "split", dir: "col" };
    if (key === "m") return { type: "messages" };
    if (e.key === "Enter") return { type: "zoom" };
    if (e.code === "BracketLeft") return { type: "workspaceStep", delta: -1 };
    if (e.code === "BracketRight") return { type: "workspaceStep", delta: 1 };
    if (e.code === "Equal") return { type: "font", delta: 1 }; // ⌘+
    return null;
  }
  if (key === "t") return { type: "newWorkspace" };
  if (key === "d") return { type: "split", dir: "row" };
  if (key === "w") return { type: "close" };
  if (key === "b") return { type: "sidebar" };
  if (key === "j") return { type: "jump" };
  if (e.code === "Equal") return { type: "font", delta: 1 };
  if (e.code === "Minus") return { type: "font", delta: -1 };
  if (e.code === "Digit0") return { type: "font", delta: 0 };
  if (e.code === "Comma") return { type: "settings" };
  const digit = /^Digit([1-9])$/.exec(e.code);
  return digit ? { type: "workspace", index: Number(digit[1]) - 1 } : null;
}

/** Terminal font size range and default (UI-UX §6). */
export const FONT = { min: 10, max: 24, base: 13 };

export function nextFontSize(size: number, delta: 1 | -1 | 0): number {
  return delta === 0 ? FONT.base : Math.min(FONT.max, Math.max(FONT.min, size + delta));
}
