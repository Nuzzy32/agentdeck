// Prompt library and session presets (D-032). Pure parts, so they run under Vitest; Palette.tsx is the view.
import { type Dir, split, type Tree } from "../layout/tree";
import type { Prompt } from "../settings/settings";

/** The core's limits (settings.rs); mirrored only to refuse in place, the core has the last word. */
export const PROMPTS_MAX = 100;
export const NAME_MAX = 80;
export const TEXT_MAX = 10_000;

/**
 * A session preset: catalogue agent ids in pane order and the split direction. Deliberately no args, flags, or env:
 * every launch goes through `pane_create`, which takes those from the catalogue alone.
 */
export type Preset = { id: string; name: string; detail: string; agents: readonly string[]; dir: Dir };

export const PRESETS: readonly Preset[] = [
  { id: "solo", name: "Solo", detail: "Claude Code", agents: ["claude-code"], dir: "row" },
  { id: "pair", name: "Pair", detail: "Claude Code kiri, Codex kanan", agents: ["claude-code", "codex"], dir: "row" },
];

// ponytail: each pane splits the last one, so three or more get 1/2, 1/4, 1/4; even shares when such a preset exists.
export function presetTree(ids: readonly string[], dir: Dir): Tree {
  const [first, ...rest] = ids;
  if (first === undefined) throw new Error("preset without panes");
  let tree: Tree = { pane: first };
  let last = first;
  for (const id of rest) {
    tree = split(tree, last, dir, id);
    last = id;
  }
  return tree;
}

export type Item = { kind: "prompt"; prompt: Prompt; index: number } | { kind: "preset"; preset: Preset };

/** Palette rows for `query`: prompts (by name or text), then presets; an empty query lists everything. */
export function search(query: string, prompts: readonly Prompt[], presets: readonly Preset[] = PRESETS): Item[] {
  const q = query.trim().toLowerCase();
  const hit = (...fields: string[]) => q === "" || fields.some((f) => f.toLowerCase().includes(q));
  return [
    ...prompts.flatMap((prompt, index) => (hit(prompt.name, prompt.text) ? [{ kind: "prompt" as const, prompt, index }] : [])),
    ...presets.filter((p) => hit(p.name, p.detail)).map((preset) => ({ kind: "preset" as const, preset })),
  ];
}

/** Control characters other than tab, line feed and carriage return; the core refuses them in a prompt. */
const CONTROL = /(?![\t\n\r])\p{Cc}/u;

/** The list after saving `draft` at `at` (null: a new prompt), or why the core would refuse it. */
export function savePrompt(prompts: readonly Prompt[], at: number | null, draft: Prompt): Prompt[] | { error: string } {
  const name = draft.name.trim();
  const text = draft.text;
  if (name === "" || [...name].length > NAME_MAX || /\p{Cc}/u.test(name)) {
    return { error: `Nama harus berisi 1–${NAME_MAX} karakter, satu baris.` };
  }
  if (prompts.some((p, i) => i !== at && p.name.trim().toLowerCase() === name.toLowerCase())) {
    return { error: `Nama "${name}" sudah dipakai.` };
  }
  if (text.trim() === "" || [...text].length > TEXT_MAX) return { error: `Teks harus berisi 1–${TEXT_MAX} karakter.` };
  if (CONTROL.test(text)) return { error: "Teks berisi karakter kontrol; hanya tab dan baris baru yang boleh." };
  if (at === null && prompts.length >= PROMPTS_MAX) return { error: `Pustaka paling banyak ${PROMPTS_MAX} prompt.` };
  const next = { name, text };
  return at === null ? [...prompts, next] : prompts.map((p, i) => (i === at ? next : p));
}

/**
 * Pastes `text` the way ⌘V does: xterm.js takes the paste event (bracketed paste when the program asked for it) and
 * the pane's input clock marks the write as typed. Trailing line breaks are dropped and no Enter is added, so the
 * person reviews it first; without bracketed paste a line break inside the text acts as Enter, as with ⌘V.
 */
export function pasteInto(target: HTMLTextAreaElement, text: string): void {
  const data = new DataTransfer();
  data.setData("text/plain", text.trimEnd());
  target.focus();
  target.dispatchEvent(new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true }));
}
