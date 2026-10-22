// Pure layout rules from docs/UI-UX.md §2–§4, kept here so they can be unit tested.

export const SIDEBAR_BREAKPOINT = 1100;
export const MESSAGES_BREAKPOINT = 1300;

/** A manual show/hide choice, remembered together with the side of the breakpoint it was made on. */
export type PanelOverride = { open: boolean; wide: boolean } | null;

/**
 * Panels follow the window width (open at or above the breakpoint) until the user toggles them. The toggle holds
 * until the window crosses the breakpoint, then automatic behaviour resumes.
 */
export function panelOpen(width: number, breakpoint: number, override: PanelOverride): boolean {
  const wide = width >= breakpoint;
  return override !== null && override.wide === wide ? override.open : wide;
}

export function togglePanel(width: number, breakpoint: number, override: PanelOverride): PanelOverride {
  return { open: !panelOpen(width, breakpoint, override), wide: width >= breakpoint };
}

/**
 * Splits a folder path so CSS can elide it from the middle: `head` gets an end ellipsis, `tail` (the last segment, or
 * the last 16 characters of a long one) always stays visible.
 */
export function splitMiddle(path: string): { head: string; tail: string } {
  const cut = path.lastIndexOf("/", path.length - 2);
  const start = cut <= 0 ? 0 : Math.max(cut, path.length - 16);
  return { head: path.slice(0, start), tail: path.slice(start) };
}

/** The status bar loop counter turns amber at 75% of the limit (UI-UX §4.5). */
export function nearLimit(count: number, limit: number): boolean {
  return limit > 0 && count / limit >= 0.75;
}
