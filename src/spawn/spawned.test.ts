import { describe, expect, it } from "vitest";
import { leaves, type Tree } from "../layout/tree";
import { keptText, placeChild, refusedText } from "./spawned";

const cell = { w: 8, h: 16 };

describe("placeChild", () => {
  const tree: Tree = { dir: "row", ratio: 0.5, a: { pane: "p1" }, b: { pane: "p2" } };

  it("splits the parent to the right when wide enough", () => {
    const t = placeChild(tree, "p1", "p3", { x: 0, y: 0, w: 2000, h: 800 }, cell);
    expect(leaves(t)).toEqual(["p1", "p3", "p2"]);
    expect(t).toMatchObject({ a: { dir: "row", a: { pane: "p1" }, b: { pane: "p3" } } });
  });

  it("splits below when only the height has room, right when nothing has", () => {
    const below = placeChild(tree, "p1", "p3", { x: 0, y: 0, w: 1400, h: 800 }, cell);
    expect(below).toMatchObject({ a: { dir: "col" } });
    const tiny = placeChild(tree, "p1", "p3", { x: 0, y: 0, w: 300, h: 100 }, cell);
    expect(tiny).toMatchObject({ a: { dir: "row" } });
    expect(placeChild({ pane: "p1" }, "p1", "p2", null, cell)).toMatchObject({ dir: "row" });
  });
});

describe("toasts", () => {
  it("names the pane and the limit", () => {
    expect(refusedText({ pane: "p1", code: "SPAWN_OFF", limit: null })).toContain("p1 meminta membuka pane");
    expect(refusedText({ pane: "p1", code: "CAP_CHILDREN", limit: 3 })).toBe("p1 mencapai batas 3 anak.");
    expect(refusedText({ pane: "p1", code: "RATE", limit: 5 })).toContain("5 pane baru per 10 menit");
    expect(refusedText({ pane: "p1", code: "NESTED", limit: null })).toBeNull();
    const k = { path: "/Users/me/app/.worktrees/p2", branch: "agent/p2", why: "ada perubahan" };
    expect(keptText("p2", k, (p) => p.replace("/Users/me", "~"))).toBe(
      "Worktree p2 punya perubahan, dibiarkan di ~/app/.worktrees/p2 (branch agent/p2).",
    );
  });
});
