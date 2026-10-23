import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import type { Pane } from "./model";
import { PaneView } from "./Pane";

const child: Pane = {
  id: "p2",
  agentId: "codex",
  agent: "Codex",
  title: "review: auth",
  folder: "~/app",
  cwd: "/Users/u/app",
  status: "idle",
  official: true,
  startedAt: 0,
  pending: 0,
  ptyId: null,
  spawn: 1,
  parent: "p1",
};

const noop = () => undefined;
const actions = {
  onFocus: noop,
  onSplit: noop,
  onZoom: noop,
  onClose: noop,
  onActivity: noop,
  onRestart: noop,
  onShellHere: noop,
};
const html = (pane: Pane) =>
  renderToStaticMarkup(<PaneView pane={pane} focused={false} zoomed={false} fontSize={13} actions={actions} />);

describe("PaneView header report (T4.2)", () => {
  it("shows the child's last report after its relation badge, with the summary as tooltip", () => {
    const out = html({
      ...child,
      report: { pane: "p2", status: "failed", summary: "Two tests fail.", files: [], tests: "fail", ts: 0 },
    });
    expect(out).toContain('class="badge pane-report pane-report-failed"');
    expect(out).toContain("hasil: gagal · tes gagal");
    expect(out).toContain("Two tests fail.");
    expect(out.indexOf("anak p1")).toBeLessThan(out.indexOf("hasil:"));
  });

  it("shows nothing before the first report", () => {
    expect(html(child)).not.toContain("pane-report");
  });
});

describe("PaneView header bypass and worktree (D-034)", () => {
  it("marks a bypassed pane in red words and shows its worktree branch", () => {
    const out = html({ ...child, parent: undefined, bypass: true, branch: "agent/p3" });
    expect(out).toContain('class="badge pane-bypass"');
    expect(out).toContain("bypass izin");
    expect(out).toContain("agent/p3");
    expect(html(child)).not.toContain("pane-bypass");
  });
});
