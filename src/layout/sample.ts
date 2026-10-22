// Static sample data for the T1.2 layout. Replaced by real panes from the Rust core in T1.3–T1.7.

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
};

export type Workspace = { name: string; folder: string; panes: Pane[] };

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

export const WORKSPACES: Workspace[] = [
  {
    name: "api",
    folder: "~/dev/app",
    panes: [
      {
        id: "p1",
        agent: "Claude Code",
        title: "feat: auth",
        folder: "~/dev/projects/client/app",
        status: "working",
        pending: 0,
      },
      {
        id: "p2",
        agent: "Codex",
        title: "review: auth",
        folder: "~/dev/projects/client/app",
        status: "needs_you",
        kind: "permission",
        parent: "p1",
        pending: 1,
      },
    ],
  },
  {
    name: "web",
    folder: "~/dev/web",
    panes: [{ id: "p3", agent: "Shell", title: "zsh", folder: "~/dev/web", status: "needs_you", pending: 0 }],
  },
];

export const MESSAGES: Message[] = [
  { time: "14:02", from: "p1", to: "p2", status: "delivered", preview: "Review the diff on branch feat/auth for bugs." },
  { time: "14:09", from: "p2", to: "p1", status: "queued", waited: "2 mnt", preview: "RESULT status=done 3 findings" },
];

export const LOOP_LIMIT = 12;
export const LOOP_COUNT = 3;

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
