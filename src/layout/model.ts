// App model: panes and workspaces (T1.5). Agent status and messages arrive with T1.7 and T2.7–T2.9.
import type { Tree } from "./tree";

export type PaneStatus = "starting" | "working" | "idle" | "needs_you" | "exited" | "unknown";
export type NeedsKind = "permission" | "question" | "auth";

export type Pane = {
  id: string;
  agent: string;
  title: string;
  folder: string;
  status: PaneStatus;
  kind?: NeedsKind;
  exitCode?: number;
  parent?: string;
  pending: number;
  /** Core pane id from `pane_create_shell`. */
  ptyId: number;
};

export type Workspace = {
  id: number;
  name: string;
  /** Absolute path; panes start here. */
  folder: string;
  tree: Tree;
  focused: string;
  /** Pane shown alone (⌘⇧Enter), or null. */
  zoomed: string | null;
};

export type Message = {
  time: string;
  from: string;
  to: string;
  status: "queued" | "delivered" | "paused" | "dropped";
  waited?: string;
  preview: string;
};

export type AgentChoice = { name: string; installed: boolean };

export const AGENTS: AgentChoice[] = [
  { name: "Shell", installed: true },
  { name: "Claude Code", installed: true },
  { name: "Codex", installed: false },
];

/** Default loop guard limit (UI-UX §4.5); the real counter comes with T2.8. */
export const LOOP_LIMIT = 12;

const NEEDS_LABEL: Record<NeedsKind, string> = {
  permission: "butuh izin",
  question: "butuh jawaban",
  auth: "perlu login",
};

/** Status label text, UI-UX §3. */
export function statusLabel(pane: Pane): string {
  switch (pane.status) {
    case "starting":
      return "memulai";
    case "working":
      return "bekerja";
    case "idle":
      return "siap";
    case "needs_you":
      return pane.kind ? NEEDS_LABEL[pane.kind] : "butuh kamu";
    case "exited":
      return `selesai (kode ${pane.exitCode ?? "?"})`;
    case "unknown":
      return "tidak diketahui";
  }
}
