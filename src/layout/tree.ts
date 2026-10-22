// Split tree of a workspace (UI-UX §2): pure functions so split, close, resize and focus moves can be unit tested.

/** `row`: children side by side (split right, ⌘D). `col`: stacked (split down, ⌘⇧D). */
export type Dir = "row" | "col";
/** A leaf holds a pane id; a split holds the share of the first child (0–1). */
export type Tree = { pane: string } | Split;
export type Split = { dir: Dir; ratio: number; a: Tree; b: Tree };
/** Path from the root to a split: "" is the root, each letter picks child `a` or `b`. */
export type Path = string;
export type Rect = { x: number; y: number; w: number; h: number };
export type Side = "left" | "right" | "up" | "down";

/** Pixels the app draws around a terminal grid; must match layout.css. */
export const CHROME = {
  /** Divider between two panes. */
  gap: 4,
  /** Pane header (28, border included) + pane border top/bottom (2) + .xterm padding (8). */
  padY: 38,
  /** Pane border left/right (2) + .xterm padding (16) + the scrollbar width FitAddon reserves (14). */
  padX: 32,
};
/** Smallest pane allowed (UI-UX §2). */
export const MIN_COLS = 40;
export const MIN_ROWS = 8;
export type Cell = { w: number; h: number };

export function leaves(t: Tree): string[] {
  return "pane" in t ? [t.pane] : [...leaves(t.a), ...leaves(t.b)];
}

/** Replaces leaf `id` with a split: the old pane first, `newId` second (right of or below it). */
export function split(t: Tree, id: string, dir: Dir, newId: string): Tree {
  if ("pane" in t) return t.pane === id ? { dir, ratio: 0.5, a: t, b: { pane: newId } } : t;
  return { ...t, a: split(t.a, id, dir, newId), b: split(t.b, id, dir, newId) };
}

/** Removes leaf `id`; its sibling takes the parent's place. Null when the last pane goes. */
export function remove(t: Tree, id: string): Tree | null {
  if ("pane" in t) return t.pane === id ? null : t;
  const a = remove(t.a, id);
  const b = remove(t.b, id);
  if (!a) return b;
  if (!b) return a;
  return { ...t, a, b };
}

export function setRatio(t: Tree, path: Path, ratio: number): Tree {
  if ("pane" in t) return t;
  if (path === "") return { ...t, ratio };
  const [head, rest] = [path[0], path.slice(1)];
  return head === "a" ? { ...t, a: setRatio(t.a, rest, ratio) } : { ...t, b: setRatio(t.b, rest, ratio) };
}

/** Splits `r` along `dir` the way the flex layout does: the gap first, the rest by ratio. */
export function divide(r: Rect, dir: Dir, ratio: number, gap = CHROME.gap): [Rect, Rect] {
  if (dir === "row") {
    const wa = (r.w - gap) * ratio;
    return [
      { ...r, w: wa },
      { ...r, x: r.x + wa + gap, w: r.w - gap - wa },
    ];
  }
  const ha = (r.h - gap) * ratio;
  return [
    { ...r, h: ha },
    { ...r, y: r.y + ha + gap, h: r.h - gap - ha },
  ];
}

/** Pixel rectangle of every pane when the tree fills `r`. */
export function rects(t: Tree, r: Rect): Map<string, Rect> {
  if ("pane" in t) return new Map([[t.pane, r]]);
  const [ra, rb] = divide(r, t.dir, t.ratio);
  return new Map([...rects(t.a, ra), ...rects(t.b, rb)]);
}

/** Terminal grid that fits a pane rectangle. */
export function grid(r: Rect, cell: Cell): { cols: number; rows: number } {
  return { cols: Math.floor((r.w - CHROME.padX) / cell.w), rows: Math.floor((r.h - CHROME.padY) / cell.h) };
}

/** False when splitting a pane of size `r` would leave panes smaller than 40 × 8 (UI-UX §2). */
export function canSplit(r: Rect, dir: Dir, cell: Cell): boolean {
  const { cols, rows } = grid(divide(r, dir, 0.5)[1], cell);
  return cols >= MIN_COLS && rows >= MIN_ROWS;
}

/** Smallest pixel size a subtree can take with every pane at least 40 × 8. */
export function minSize(t: Tree, cell: Cell): { w: number; h: number } {
  if ("pane" in t) return { w: MIN_COLS * cell.w + CHROME.padX, h: MIN_ROWS * cell.h + CHROME.padY };
  const a = minSize(t.a, cell);
  const b = minSize(t.b, cell);
  return t.dir === "row"
    ? { w: a.w + b.w + CHROME.gap, h: Math.max(a.h, b.h) }
    : { w: Math.max(a.w, b.w), h: a.h + b.h + CHROME.gap };
}

/** Keeps a dragged divider where both sides still fit 40 × 8; leaves `ratio` as is when nothing fits. */
export function clampRatio(t: Tree, size: number, ratio: number, cell: Cell): number {
  if ("pane" in t) return ratio;
  const key = t.dir === "row" ? "w" : "h";
  const avail = size - CHROME.gap;
  const lo = minSize(t.a, cell)[key] / avail;
  const hi = 1 - minSize(t.b, cell)[key] / avail;
  return lo > hi ? t.ratio : Math.min(hi, Math.max(lo, ratio));
}

/**
 * Pane next to `id` on `side`: the nearest one beyond that edge that overlaps it on the other axis, preferring the
 * larger overlap. Null at the edge of the workspace.
 */
export function neighbor(t: Tree, id: string, side: Side): string | null {
  const all = rects(t, { x: 0, y: 0, w: 1000, h: 1000 });
  const cur = all.get(id);
  if (!cur) return null;
  const eps = 1e-6;
  let best: { id: string; dist: number; overlap: number } | null = null;
  for (const [other, r] of all) {
    if (other === id) continue;
    const horizontal = side === "left" || side === "right";
    const dist =
      side === "right" ? r.x - (cur.x + cur.w) : side === "left" ? cur.x - (r.x + r.w)
      : side === "down" ? r.y - (cur.y + cur.h) : cur.y - (r.y + r.h);
    const overlap = horizontal
      ? Math.min(cur.y + cur.h, r.y + r.h) - Math.max(cur.y, r.y)
      : Math.min(cur.x + cur.w, r.x + r.w) - Math.max(cur.x, r.x);
    if (dist < -eps || overlap <= eps) continue;
    if (!best || dist < best.dist - eps || (Math.abs(dist - best.dist) <= eps && overlap > best.overlap)) {
      best = { id: other, dist, overlap };
    }
  }
  return best?.id ?? null;
}

/** Every split with its path and pixel rectangle, for drawing and dragging dividers. */
export function splits(t: Tree, r: Rect, path: Path = ""): { path: Path; node: Split; rect: Rect }[] {
  if ("pane" in t) return [];
  const [ra, rb] = divide(r, t.dir, t.ratio);
  return [{ path, node: t, rect: r }, ...splits(t.a, ra, `${path}a`), ...splits(t.b, rb, `${path}b`)];
}

/** Divider strip of a split occupying `r`. */
export function divider(dir: Dir, ratio: number, r: Rect): Rect {
  const [a] = divide(r, dir, ratio);
  return dir === "row" ? { ...r, x: r.x + a.w, w: CHROME.gap } : { ...r, y: r.y + a.h, h: CHROME.gap };
}
