import { describe, expect, it } from "vitest";
import { type Pane, statusLabel, type Workspace } from "./model";
import { hydrate, snapshot } from "./persist";

const p1: Pane = {
  id: "p1",
  agentId: "claude-code",
  agent: "Claude Code",
  title: "feat: auth",
  folder: "~/app",
  cwd: "/Users/u/app",
  status: "working",
  official: true,
  startedAt: 5,
  pending: 2,
  ptyId: 7,
  spawn: 3,
  msg: "editing",
  report: { pane: "p1", status: "done", summary: "s", files: [], tests: "pass", ts: 0 },
};
const p2: Pane = { ...p1, id: "p2", agentId: null, agent: "Shell", title: "app", parent: "p1", report: undefined };
const ws: Workspace = {
  id: 2,
  name: "app",
  folder: "/Users/u/app",
  tree: { dir: "row", ratio: 0.6, a: { pane: "p1" }, b: { pane: "p2" } },
  focused: "p2",
  zoomed: "p2",
};

describe("layout.json snapshot (T4.3)", () => {
  it("keeps the layout and who each pane is, nothing of the run", () => {
    const l = snapshot([ws], { p1, p2 }, 2);
    expect(l).toEqual({
      version: 1,
      active: 2,
      workspaces: [
        {
          id: 2,
          name: "app",
          folder: "/Users/u/app",
          tree: ws.tree,
          focused: "p2",
          panes: [
            { id: "p1", agent: "claude-code", name: "Claude Code", title: "feat: auth", cwd: "/Users/u/app", parent: null },
            { id: "p2", agent: null, name: "Shell", title: "app", cwd: "/Users/u/app", parent: "p1" },
          ],
        },
      ],
    });
  });

  it("gives back waiting panes with no process, no zoom, and the same ids", () => {
    let n = 10;
    const h = hydrate(snapshot([ws], { p1, p2 }, 2), "/Users/u", () => n++);
    expect(h.active).toBe(2);
    expect(h.workspaces).toEqual([{ ...ws, zoomed: null }]);
    expect(h.panes.p1).toMatchObject({ status: "exited", restored: true, ptyId: null, pending: 0, folder: "~/app" });
    expect(h.panes.p1?.report).toBeUndefined();
    expect(h.panes.p1?.msg).toBeUndefined();
    expect(h.panes.p2?.parent).toBe("p1");
    expect(h.panes.p1?.spawn).not.toBe(h.panes.p2?.spawn);
    expect(statusLabel(h.panes.p1 as Pane)).toBe("belum dijalankan");
  });
});
