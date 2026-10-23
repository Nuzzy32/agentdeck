// `layout.json` (T4.3, TERM-06): what the core keeps of the layout, and the panes it gives back on open. Every pane
// comes back waiting (D-025); the core checks the file and keeps the worktrees.
import { type Pane, tildePath, type Workspace } from "./model";
import { leaves, type Tree } from "./tree";

/** layout.rs `Layout`. `worktree` is the core's alone, so it never appears here. */
export type Layout = {
  version: 1;
  active: number | null;
  workspaces: {
    id: number;
    name: string;
    folder: string;
    tree: Tree;
    focused: string;
    panes: { id: string; agent: string | null; name: string; title: string; cwd: string; parent: string | null }[];
  }[];
};

/** Only what survives a restart: no status, process, messages or report. */
export function snapshot(workspaces: Workspace[], panes: Record<string, Pane>, active: number | null): Layout {
  return {
    version: 1,
    active,
    workspaces: workspaces.map((w) => ({
      id: w.id,
      name: w.name,
      folder: w.folder,
      tree: w.tree,
      focused: w.focused,
      panes: leaves(w.tree).flatMap((id) => {
        const p = panes[id];
        return p
          ? [{ id, agent: p.agentId, name: p.agent, title: p.title, cwd: p.cwd, parent: p.parent ?? null }]
          : [];
      }),
    })),
  };
}

/** Restored panes have no process (`ptyId` null) and show "belum dijalankan" until the person runs them. */
export function hydrate(
  layout: Layout,
  home: string | null,
  nextSpawn: () => number,
): { panes: Record<string, Pane>; workspaces: Workspace[]; active: number | null } {
  const panes: Record<string, Pane> = {};
  for (const w of layout.workspaces)
    for (const p of w.panes)
      panes[p.id] = {
        id: p.id,
        agentId: p.agent,
        agent: p.name,
        title: p.title,
        folder: tildePath(p.cwd, home),
        cwd: p.cwd,
        status: "exited",
        restored: true,
        official: false,
        startedAt: 0,
        pending: 0,
        ptyId: null,
        spawn: nextSpawn(),
        ...(p.parent ? { parent: p.parent } : {}),
      };
  const workspaces = layout.workspaces.map((w) => ({
    id: w.id,
    name: w.name,
    folder: w.folder,
    tree: w.tree,
    focused: w.focused,
    zoomed: null,
  }));
  return { panes, workspaces, active: layout.active };
}
