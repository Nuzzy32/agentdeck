import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { shortcut } from "../layout/keys";
import { leaves } from "../layout/tree";
import { DEFAULTS, merge, type Prompt } from "../settings/settings";
import { Palette } from "./Palette";
import { expand, gridTree, NAME_MAX, PRESETS, PROMPTS_MAX, savePrompt, search, TEXT_MAX } from "./prompts";

const review: Prompt = { name: "Review", text: "Tinjau diff ini.\nSingkat saja." };
const tests: Prompt = { name: "Tes", text: "Jalankan pnpm test" };

describe("palette search", () => {
  it("lists prompts then presets, and filters both by name or text", () => {
    expect(search("", [review, tests]).map((i) => (i.kind === "prompt" ? i.prompt.name : i.preset.id))).toEqual([
      "Review",
      "Tes",
      "solo",
      "pair",
    ]);
    expect(search("PNPM", [review, tests])).toEqual([{ kind: "prompt", prompt: tests, index: 1 }]);
    expect(search("codex", [review, tests]).map((i) => i.kind === "preset" && i.preset.id)).toEqual(["pair"]);
    expect(search("tidak ada", [review])).toEqual([]);
  });

  it("is ⌘P, which no other shortcut uses", () => {
    const key = { key: "p", code: "KeyP", metaKey: true, shiftKey: false, altKey: false, ctrlKey: false };
    expect(shortcut(key)).toEqual({ type: "prompts" });
    expect(shortcut({ ...key, shiftKey: true })).toBeNull();
    expect(shortcut({ ...key, metaKey: false })).toBeNull();
  });
});

describe("saving a prompt", () => {
  it("adds, changes and keeps names unique", () => {
    expect(savePrompt([], null, { name: "  Review ", text: review.text })).toEqual([review]);
    expect(savePrompt([review, tests], 0, { name: "review", text: "baru" })).toEqual([
      { name: "review", text: "baru" },
      tests,
    ]);
    expect(savePrompt([review, tests], 1, { name: "REVIEW", text: "x" })).toEqual({
      error: 'Nama "REVIEW" sudah dipakai.',
    });
  });

  it("refuses in place what the core would refuse", () => {
    const err = (draft: Prompt, list: Prompt[] = []) => {
      const r = savePrompt(list, null, draft);
      return "error" in r ? r.error : null;
    };
    expect(err({ name: "", text: "x" })).toMatch(/^Nama/);
    expect(err({ name: "a\tb", text: "x" })).toMatch(/^Nama/);
    expect(err({ name: "n".repeat(NAME_MAX + 1), text: "x" })).toMatch(/^Nama/);
    expect(err({ name: "a", text: " \n" })).toMatch(/^Teks harus/);
    expect(err({ name: "a", text: "x".repeat(TEXT_MAX + 1) })).toMatch(/^Teks harus/);
    expect(err({ name: "a", text: "x\u001b[201~\r" })).toMatch(/karakter kontrol/);
    expect(err({ name: "a", text: "baris\n\tdua\r\n" })).toBeNull();
    const full = Array.from({ length: PROMPTS_MAX }, (_, i) => ({ name: `p${i}`, text: "x" }));
    expect(err({ name: "baru", text: "x" }, full)).toMatch(/paling banyak/);
  });

  it("replaces the whole list in a settings patch", () => {
    const s = merge({ ...DEFAULTS, prompts: [review, tests] }, { prompts: [tests] });
    expect(s.prompts).toEqual([tests]);
    expect(s.terminal).toEqual(DEFAULTS.terminal);
  });
});

describe("session presets", () => {
  it("are catalogue ids and a split, nothing an agent could be launched with", () => {
    for (const p of PRESETS) {
      expect(Object.keys(p).sort()).toEqual(["agents", "detail", "id", "name"]);
      for (const a of p.agents) expect(a).toMatch(/^[a-z][a-z0-9-]*$/);
    }
    expect(PRESETS.map((p) => [p.name, p.agents])).toEqual([
      ["Solo", ["claude-code"]],
      ["Pair", ["claude-code", "codex"]],
    ]);
  });

  it("place panes in order with even shares: Pair is Claude left, Codex right", () => {
    expect(gridTree(["p1"])).toEqual({ pane: "p1" });
    expect(gridTree(["p1", "p2"])).toEqual({ dir: "row", ratio: 0.5, a: { pane: "p1" }, b: { pane: "p2" } });
    const three = gridTree(["p1", "p2", "p3"]);
    expect(leaves(three)).toEqual(["p1", "p2", "p3"]);
    expect(three).toMatchObject({ dir: "row", ratio: 1 / 3, b: { dir: "row", ratio: 0.5 } });
    // Four or more: two rows, the top one gets the odd pane.
    const five = gridTree(["p1", "p2", "p3", "p4", "p5"]);
    expect(five).toMatchObject({ dir: "col", ratio: 0.5 });
    expect(leaves(five)).toEqual(["p1", "p2", "p3", "p4", "p5"]);
    if (!("a" in five)) throw new Error("split expected");
    expect(leaves(five.a)).toEqual(["p1", "p2", "p3"]);
    expect(() => gridTree([])).toThrow();
  });

  it("expand counts in program order (D-034)", () => {
    const order = ["shell", "claude-code", "codex"];
    expect(expand({ codex: 2, "claude-code": 1 }, order)).toEqual(["claude-code", "codex", "codex"]);
    expect(expand({ shell: 1, "claude-code": 0, gemini: 3 }, order)).toEqual(["shell"]);
    expect(expand({ codex: -1, shell: 1.7 }, order)).toEqual(["shell"]);
  });
});

describe("palette view", () => {
  const view = (prompts: Prompt[], target: string | null) =>
    renderToStaticMarkup(
      <Palette
        prompts={prompts}
        target={target}
        folder="/Users/a/proj"
        recents={[]}
        home="/Users/a"
        onPaste={() => undefined}
        onSave={() => undefined}
        onPreset={async () => null}
        onClose={() => undefined}
      />,
    );

  it("names the pane a prompt goes to and lists the presets", () => {
    const html = view([review], "p2 Shell");
    expect(html).toContain("ditempel ke p2 Shell, tanpa Enter");
    expect(html).toContain("Review");
    expect(html).toContain("Tinjau diff ini.");
    expect(html).not.toContain("Singkat saja.");
    expect(html).toContain("Pair");
    expect(html).toContain("Claude Code kiri, Codex kanan");
  });

  it("disables prompts without a running pane, and says when the library is empty", () => {
    expect(view([review], null)).toMatch(/<button type="button" data-row="true" class="palette-item" disabled="">/);
    expect(view([review], null)).toContain("fokuskan pane yang berjalan");
    expect(view([], "p1 Shell")).toContain("Belum ada prompt.");
  });
});
