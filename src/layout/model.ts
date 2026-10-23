// App model: panes and workspaces (T1.5), agent launch and status (T1.7). Messages arrive with T2.7–T2.9.
import type { Tree } from "./tree";

export type PaneStatus = "starting" | "working" | "idle" | "needs_you" | "exited" | "unknown";
export type NeedsKind = "permission" | "question" | "auth";

/** `pane_create` error from the core: ready-to-show text, plus the install command when the CLI is missing. */
export type LaunchError = { message: string; install: string | null };

/** `PaneEvent` from pty.rs. `unknown` status: the agent cleared its report, the output heuristic takes over. */
export type PaneEvent =
  | {
      type: "status";
      status: "idle" | "working" | "needs_you" | "unknown";
      kind: NeedsKind | null;
      msg: string | null;
      error: boolean;
    }
  | { type: "exited"; code: number | null };

/** A child's last `pane_report` (report.rs `Report`), shown in its header (T4.2). `ts`: epoch ms. */
export type Report = {
  pane: string;
  status: "done" | "failed" | "blocked";
  summary: string;
  files: string[];
  tests: "pass" | "fail" | "none";
  ts: number;
};

/** `agents_list` from catalogue.rs. */
export type AgentInfo = { id: string; name: string; installed: boolean; version: string | null; install: string };

export type Pane = {
  id: string;
  /** Catalogue id (`claude-code`, `codex`), or null for a shell. */
  agentId: string | null;
  agent: string;
  title: string;
  /** For display (`~/…`). */
  folder: string;
  /** Absolute path the process runs in. */
  cwd: string;
  status: PaneStatus;
  kind?: NeedsKind;
  /** Agent's own status text (OSC 7501 `msg`), shown as the status tooltip. */
  msg?: string;
  /** The agent reported `error`: dot stays `idle` with an error icon. */
  error?: boolean;
  /** Status came from the agent itself; the output heuristic stays out until it clears it. */
  official: boolean;
  startedAt: number;
  exitCode?: number | null;
  /** Spawn failed (UI-UX §3 "Gagal dijalankan"); no process. */
  failed?: LaunchError;
  parent?: string;
  report?: Report;
  pending: number;
  /** Core pane id from `pane_create`; null while spawning or after a failed spawn. */
  ptyId: number | null;
  /** Which spawn of this pane events belong to; a restart makes earlier events stale. */
  spawn: number;
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

const REPORT_LABEL: Record<Report["status"], string> = { done: "selesai", failed: "gagal", blocked: "terhambat" };
const TESTS_LABEL: Record<Report["tests"], string | null> = { pass: "tes lulus", fail: "tes gagal", none: null };

/** Header badge for a child's last report: short label, and the summary, files and time as its tooltip. */
export function reportBadge(r: Report): { label: string; title: string } {
  const tests = TESTS_LABEL[r.tests];
  const time = new Date(r.ts).toLocaleTimeString("id-ID", { hour: "2-digit", minute: "2-digit" });
  const files = r.files.length ? `\nBerkas: ${r.files.join(", ")}` : "";
  return {
    label: `hasil: ${REPORT_LABEL[r.status]}${tests ? ` · ${tests}` : ""}`,
    title: `Laporan ${time}\n${r.summary}${files}`,
  };
}

/** An agent with no official report yet stays `starting` this long before the output heuristic applies. */
export const STARTING_MS = 5000;

/** Core event → pane (ARCHITECTURE §6). */
export function applyEvent(p: Pane, ev: PaneEvent): Pane {
  if (ev.type === "exited") {
    return { ...p, status: "exited", exitCode: ev.code, official: true, kind: undefined, msg: undefined };
  }
  if (p.status === "exited") return p;
  if (ev.status === "unknown") {
    return { ...p, status: "unknown", official: false, kind: undefined, msg: undefined, error: false };
  }
  return { ...p, status: ev.status, kind: ev.kind ?? undefined, msg: ev.msg ?? undefined, error: ev.error, official: true };
}

/**
 * Output heuristic (ARCHITECTURE §6): output in the last second → `working`, otherwise `unknown`. Never overrides the
 * agent's own report, an exit, or an agent that is still within its starting grace.
 */
export function applyActivity(p: Pane, busy: boolean, now: number): Pane {
  if (p.official || p.status === "exited" || p.ptyId === null) return p;
  if (p.status === "starting" && now - p.startedAt < STARTING_MS) return p;
  const status = busy ? "working" : "unknown";
  return status === p.status ? p : { ...p, status };
}

/** Most recent first, no duplicates, at most 5 (UI-UX §4.1). */
export function pushRecent(list: string[], folder: string): string[] {
  return [folder, ...list.filter((f) => f !== folder)].slice(0, 5);
}

/** `~` and `~/x` → absolute; anything else unchanged. */
export function expandHome(path: string, home: string | null): string {
  if (!home) return path;
  if (path === "~") return home;
  return path.startsWith("~/") ? `${home}${path.slice(1)}` : path;
}

/** Header title: the first line of the task, else the folder name. */
export function paneTitle(task: string, folder: string): string {
  const line = task.trim().split("\n")[0]?.trim() ?? "";
  if (!line) return folder.split("/").filter(Boolean).pop() ?? "/";
  return line.length > 60 ? `${line.slice(0, 59)}…` : line;
}

/** Absolute → `~/…` for display. */
export function tildePath(path: string, home: string | null): string {
  if (!home) return path;
  if (path === home) return "~";
  return path.startsWith(`${home}/`) ? `~${path.slice(home.length)}` : path;
}
