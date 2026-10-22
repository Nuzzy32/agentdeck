// Panes the core opens for `pane_spawn` (T3.2, T4.1): the events, where the child goes, and the toast texts.
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { PaneEvent } from "../layout/model";
import { type Cell, canSplit, type Dir, type Rect, rects, split, type Tree } from "../layout/tree";

/** `agentdeck://pane-spawned` (spawn.rs `Spawned`). `branch`: the child's own worktree, if any. */
export type Spawned = {
  pane: string;
  pty: number;
  workspace: number;
  parent: string;
  agent: string;
  agent_name: string;
  title: string;
  cwd: string;
  branch: string | null;
};

/** `agentdeck://spawn-refused`: the switch or a limit refused a spawn before any card. */
export type SpawnRefused = { pane: string; code: string; limit: number | null };

/** `pane_close` result: a child's worktree left in place because it has changes. */
export type KeptWorktree = { path: string; branch: string; why: string };

export const spawnClient = {
  onSpawned: (cb: (s: Spawned) => void): Promise<UnlistenFn> =>
    listen<Spawned>("agentdeck://pane-spawned", (e) => cb(e.payload)),
  // A child's status and exit; always after its `pane-spawned`.
  onEvent: (cb: (pane: string, ev: PaneEvent) => void): Promise<UnlistenFn> =>
    listen<{ pane: string; event: PaneEvent }>("agentdeck://pane-event", (e) => cb(e.payload.pane, e.payload.event)),
  onRefused: (cb: (r: SpawnRefused) => void): Promise<UnlistenFn> =>
    listen<SpawnRefused>("agentdeck://spawn-refused", (e) => cb(e.payload)),
};

/** The child goes beside its parent: split right when there is room, else below, else right anyway. */
export function placeChild(tree: Tree, parent: string, child: string, area: Rect | null, cell: Cell): Tree {
  const r = area ? rects(tree, area).get(parent) : undefined;
  const dir: Dir = !r || canSplit(r, "row", cell) || !canSplit(r, "col", cell) ? "row" : "col";
  return split(tree, parent, dir, child);
}

/** Toast for a refused spawn (UI-UX §5); null for codes the agent handles alone. */
export function refusedText(r: SpawnRefused): string | null {
  switch (r.code) {
    case "SPAWN_OFF":
      return `${r.pane} meminta membuka pane, tapi fitur ini mati. Nyalakan di Settings › Kolaborasi.`;
    case "CAP_CHILDREN":
      return `${r.pane} mencapai batas ${r.limit ?? "?"} anak.`;
    case "CAP_GLOBAL":
      return `${r.pane} ditolak: sudah ${r.limit ?? "?"} pane agen yang dibuka agen.`;
    case "RATE":
      return `${r.pane} mencapai batas ${r.limit ?? "?"} pane baru per 10 menit.`;
    default:
      return null;
  }
}

/** Toast when closing a child keeps its worktree (T4.1). */
export function keptText(pane: string, k: KeptWorktree, folder: (path: string) => string): string {
  return `Worktree ${pane} punya perubahan, dibiarkan di ${folder(k.path)} (branch ${k.branch}).`;
}
