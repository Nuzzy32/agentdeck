import { describe, expect, it } from "vitest";
import { type Action, nextFontSize, shortcut } from "./keys";
import {
  canSplit,
  CHROME,
  clampRatio,
  divider,
  leaves,
  MIN_COLS,
  MIN_ROWS,
  neighbor,
  rects,
  remove,
  setRatio,
  split,
  splits,
  type Tree,
} from "./tree";

// p1 | p2 on top, p3 below both:
// ┌────┬────┐
// │ p1 │ p2 │
// ├────┴────┤
// │   p3    │
// └─────────┘
const fixture = (): Tree => split(split({ pane: "p1" }, "p1", "col", "p3"), "p1", "row", "p2");

describe("split", () => {
  it("puts the new pane right of or below the old one", () => {
    expect(split({ pane: "p1" }, "p1", "row", "p2")).toEqual({
      dir: "row",
      ratio: 0.5,
      a: { pane: "p1" },
      b: { pane: "p2" },
    });
    expect(leaves(fixture())).toEqual(["p1", "p2", "p3"]);
  });

  it("leaves the tree alone for an unknown pane", () => {
    const t = fixture();
    expect(split(t, "p9", "row", "p4")).toEqual(t);
  });
});

describe("remove", () => {
  it("lets the sibling take the parent's place", () => {
    const t = remove(fixture(), "p2");
    expect(t).toEqual({ dir: "col", ratio: 0.5, a: { pane: "p1" }, b: { pane: "p3" } });
    expect(remove(fixture(), "p3")).toEqual({ dir: "row", ratio: 0.5, a: { pane: "p1" }, b: { pane: "p2" } });
  });

  it("returns null when the last pane goes", () => {
    expect(remove({ pane: "p1" }, "p1")).toBeNull();
  });
});

describe("setRatio and rects", () => {
  it("resizes the split at a path and lays panes out with the gap", () => {
    const t = setRatio(fixture(), "a", 0.25);
    const r = rects(t, { x: 0, y: 0, w: 804, h: 404 });
    expect(r.get("p1")).toEqual({ x: 0, y: 0, w: 200, h: 200 });
    expect(r.get("p2")).toEqual({ x: 204, y: 0, w: 600, h: 200 });
    expect(r.get("p3")).toEqual({ x: 0, y: 204, w: 804, h: 200 });
    const dividers = splits(t, { x: 0, y: 0, w: 804, h: 404 }).map((s) => [s.path, divider(s.node.dir, s.node.ratio, s.rect)]);
    expect(dividers).toEqual([
      ["", { x: 0, y: 200, w: 804, h: 4 }],
      ["a", { x: 200, y: 0, w: 4, h: 200 }],
    ]);
  });
});

describe("neighbor", () => {
  it("moves focus across the layout", () => {
    const t = fixture();
    expect(neighbor(t, "p1", "right")).toBe("p2");
    expect(neighbor(t, "p2", "left")).toBe("p1");
    expect(neighbor(t, "p1", "down")).toBe("p3");
    expect(neighbor(t, "p2", "down")).toBe("p3");
    expect(neighbor(t, "p3", "up")).toBe("p1");
  });

  it("stays put at the edge", () => {
    const t = fixture();
    expect(neighbor(t, "p1", "left")).toBeNull();
    expect(neighbor(t, "p1", "up")).toBeNull();
    expect(neighbor(t, "p3", "down")).toBeNull();
    expect(neighbor({ pane: "p1" }, "p1", "right")).toBeNull();
  });

  it("prefers the pane that overlaps most", () => {
    const t = setRatio(fixture(), "a", 0.8);
    expect(neighbor(t, "p3", "up")).toBe("p1");
  });
});

describe("canSplit (40 × 8 minimum)", () => {
  const cell = { w: 8, h: 16 };
  // Exactly wide enough for two panes of 40 columns, and tall enough for two of 8 rows.
  const w = 2 * (MIN_COLS * cell.w + CHROME.padX) + CHROME.gap;
  const h = 2 * (MIN_ROWS * cell.h + CHROME.padY) + CHROME.gap;

  it("allows a split that leaves exactly 40 × 8", () => {
    expect(canSplit({ x: 0, y: 0, w, h: 600 }, "row", cell)).toBe(true);
    expect(canSplit({ x: 0, y: 0, w: 900, h }, "col", cell)).toBe(true);
  });

  it("rejects a split below 40 columns or 8 rows", () => {
    expect(canSplit({ x: 0, y: 0, w: w - 2, h: 600 }, "row", cell)).toBe(false);
    expect(canSplit({ x: 0, y: 0, w: 900, h: h - 2 }, "col", cell)).toBe(false);
    // A pane already too short cannot be split sideways either.
    expect(canSplit({ x: 0, y: 0, w: 2000, h: MIN_ROWS * cell.h + CHROME.padY - 1 }, "row", cell)).toBe(false);
  });

  it("keeps a dragged divider inside the 40 × 8 limit", () => {
    const t = split({ pane: "p1" }, "p1", "row", "p2");
    const size = 1000;
    const lo = (MIN_COLS * cell.w + CHROME.padX) / (size - CHROME.gap);
    expect(clampRatio(t, size, 0.01, cell)).toBeCloseTo(lo);
    expect(clampRatio(t, size, 0.99, cell)).toBeCloseTo(1 - lo);
    expect(clampRatio(t, size, 0.5, cell)).toBe(0.5);
    // Too small for both sides: the ratio stays where it was.
    expect(clampRatio(t, 400, 0.9, cell)).toBe(0.5);
  });
});

describe("shortcut", () => {
  const key = (k: string, code: string, mods: { shift?: boolean; alt?: boolean; ctrl?: boolean; meta?: boolean } = {}) =>
    shortcut({
      key: k,
      code,
      metaKey: mods.meta ?? true,
      shiftKey: mods.shift ?? false,
      altKey: mods.alt ?? false,
      ctrlKey: mods.ctrl ?? false,
    });

  it("maps UI-UX §7", () => {
    const cases: [ReturnType<typeof key>, Action][] = [
      [key("t", "KeyT"), { type: "newWorkspace" }],
      [key("d", "KeyD"), { type: "split", dir: "row" }],
      [key("D", "KeyD", { shift: true }), { type: "split", dir: "col" }],
      [key("w", "KeyW"), { type: "close" }],
      [key("ArrowLeft", "ArrowLeft", { alt: true }), { type: "focus", side: "left" }],
      [key("ArrowDown", "ArrowDown", { alt: true }), { type: "focus", side: "down" }],
      [key("1", "Digit1"), { type: "workspace", index: 0 }],
      [key("9", "Digit9"), { type: "workspace", index: 8 }],
      [key("{", "BracketLeft", { shift: true }), { type: "workspaceStep", delta: -1 }],
      [key("}", "BracketRight", { shift: true }), { type: "workspaceStep", delta: 1 }],
      [key("Enter", "Enter", { shift: true }), { type: "zoom" }],
      [key("b", "KeyB"), { type: "sidebar" }],
      [key("M", "KeyM", { shift: true }), { type: "messages" }],
      [key("=", "Equal"), { type: "font", delta: 1 }],
      [key("+", "Equal", { shift: true }), { type: "font", delta: 1 }],
      [key("-", "Minus"), { type: "font", delta: -1 }],
      [key("0", "Digit0"), { type: "font", delta: 0 }],
    ];
    for (const [got, want] of cases) expect(got).toEqual(want);
  });

  it("leaves terminal and clipboard keys alone", () => {
    expect(key("c", "KeyC")).toBeNull();
    expect(key("v", "KeyV")).toBeNull();
    expect(key("d", "KeyD", { meta: false, ctrl: true })).toBeNull();
    expect(key("d", "KeyD", { meta: false })).toBeNull();
    expect(key("∂", "KeyD", { meta: false, alt: true })).toBeNull();
    expect(key("d", "KeyD", { ctrl: true })).toBeNull();
    expect(key("ArrowLeft", "ArrowLeft")).toBeNull();
    expect(key("j", "KeyJ")).toBeNull();
  });
});

describe("nextFontSize", () => {
  it("steps within 10–24 and resets to 13", () => {
    expect(nextFontSize(13, 1)).toBe(14);
    expect(nextFontSize(24, 1)).toBe(24);
    expect(nextFontSize(10, -1)).toBe(10);
    expect(nextFontSize(20, 0)).toBe(13);
  });
});
