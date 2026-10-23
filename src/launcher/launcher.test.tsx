import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { MANY_MAX } from "../prompts/prompts";
import { bump, Launcher, previewRows, SHORTCUTS } from "./Launcher";

const agents = [
  { id: "claude-code", name: "Claude Code", installed: true, version: "2.1.309", install: "" },
  { id: "codex", name: "Codex", installed: false, version: null, install: "npm i -g @openai/codex" },
];
const none = async () => null;
const view = (many: boolean) =>
  renderToStaticMarkup(
    <Launcher
      title="Workspace baru"
      agents={agents}
      initial="claude-code"
      folder="/Users/u/app"
      recents={[]}
      home="/Users/u"
      onLaunch={none}
      onLaunchMany={many ? none : undefined}
      onClose={() => undefined}
    />,
  );

describe("workspace launcher counts (D-034)", () => {
  it("keeps the total within 0..MANY_MAX", () => {
    expect(bump({}, "codex", 1)).toEqual({ codex: 1 });
    expect(bump({ codex: 0 }, "codex", -1)).toEqual({ codex: 0 });
    expect(bump({ "claude-code": MANY_MAX }, "codex", 1)).toEqual({ "claude-code": MANY_MAX, codex: 0 });
    expect(bump({ "claude-code": 7 }, "claude-code", 1)).toEqual({ "claude-code": 8 });
  });

  it("shortcuts are plain counts of catalogue ids, nothing an agent could be launched with", () => {
    for (const s of SHORTCUTS) {
      expect(Object.keys(s).sort()).toEqual(["counts", "label"]);
      for (const n of Object.values(s.counts)) expect(n).toBeLessThanOrEqual(MANY_MAX / 2);
    }
  });

  it("shows steppers and both switches off; shortcuts needing a missing agent are hidden", () => {
    const out = view(true);
    expect(out).toContain("Terminal sekaligus");
    expect(out).toContain('aria-label="Tambah Claude Code"');
    expect(out).toContain("1 Claude");
    expect(out).not.toContain("Claude + Codex");
    expect(out).toContain("Worktree baru per terminal agen");
    expect(out).toContain("Lewati izin di semua terminal agen");
    expect(out).not.toMatch(/type="checkbox"[^>]*checked/);
  });

  it("the split launcher stays one program, without the switches", () => {
    const out = view(false);
    expect(out).toContain('name="program"');
    expect(out).not.toContain("Lewati izin");
  });

  it("previews the same rows gridTree builds", () => {
    expect(previewRows([])).toEqual([[]]);
    expect(previewRows(["a", "b", "c"])).toEqual([["a", "b", "c"]]);
    expect(previewRows(["a", "b", "c", "d", "e"])).toEqual([["a", "b", "c"], ["d", "e"]]);
  });

  it("the start screen is a page, not a modal, without Batal", () => {
    const out = renderToStaticMarkup(
      <Launcher
        inline
        title="Mulai kerja"
        agents={agents}
        initial="claude-code"
        folder="/Users/u/app"
        recents={["/Users/u/dev/api"]}
        home="/Users/u"
        onLaunch={none}
        onLaunchMany={none}
        onClose={() => undefined}
      />,
    );
    expect(out).not.toContain("<dialog");
    expect(out).not.toContain("Batal");
    expect(out).toContain("Buka 1 terminal");
    expect(out).toContain("~/dev/api");
  });
});
