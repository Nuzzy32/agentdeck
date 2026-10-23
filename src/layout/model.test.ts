import { describe, expect, it } from "vitest";
import {
  applyActivity,
  applyEvent,
  expandHome,
  type Pane,
  type Report,
  paneTitle,
  pushRecent,
  reportBadge,
  STARTING_MS,
  statusLabel,
  tildePath,
  usageBadge,
} from "./model";

const agent: Pane = {
  id: "p1",
  agentId: "claude-code",
  agent: "Claude Code",
  title: "app",
  folder: "~/app",
  cwd: "/Users/u/app",
  status: "starting",
  official: false,
  startedAt: 0,
  pending: 0,
  ptyId: 7,
  spawn: 1,
};

const status = (s: "idle" | "working" | "needs_you" | "unknown", extra = {}) =>
  ({ type: "status", status: s, kind: null, msg: null, error: false, ...extra }) as const;

describe("applyEvent", () => {
  it("maps OSC 7501 reports to header status and label (ARCHITECTURE §6)", () => {
    const idle = applyEvent(agent, status("idle"));
    expect([idle.status, statusLabel(idle), idle.official]).toEqual(["idle", "siap", true]);
    const auth = applyEvent(idle, status("needs_you", { kind: "auth", msg: "Login expired · Please run /login" }));
    expect([statusLabel(auth), auth.msg]).toEqual(["perlu login", "Login expired · Please run /login"]);
    expect(statusLabel(applyEvent(idle, status("needs_you", { kind: "permission" })))).toBe("butuh izin");
    expect(statusLabel(applyEvent(idle, status("needs_you", { kind: "question" })))).toBe("butuh jawaban");
    expect(statusLabel(applyEvent(idle, status("needs_you")))).toBe("butuh kamu");
    const err = applyEvent(auth, status("idle", { error: true }));
    expect([err.status, err.error, err.kind, err.msg]).toEqual(["idle", true, undefined, undefined]);
  });

  it("hands back to the heuristic when the agent clears its report", () => {
    const cleared = applyEvent(applyEvent(agent, status("working")), status("unknown"));
    expect([cleared.status, cleared.official]).toEqual(["unknown", false]);
  });

  it("keeps the exit and its code", () => {
    const exited = applyEvent(applyEvent(agent, status("working")), { type: "exited", code: 3 });
    expect(statusLabel(exited)).toBe("selesai (kode 3)");
    expect(applyEvent(exited, status("idle")).status).toBe("exited");
    expect(statusLabel(applyEvent(agent, { type: "exited", code: null }))).toBe("selesai (kode ?)");
  });
});

describe("applyActivity", () => {
  it("never overrides an official report, an exit, or a fresh start", () => {
    const idle = applyEvent(agent, status("idle"));
    expect(applyActivity(idle, true, 99_999)).toBe(idle);
    expect(applyActivity(agent, true, STARTING_MS - 1)).toBe(agent);
    const exited = applyEvent(agent, { type: "exited", code: 0 });
    expect(applyActivity(exited, true, 99_999)).toBe(exited);
  });

  it("output means working, quiet means unknown, once no official status exists", () => {
    expect(applyActivity(agent, true, STARTING_MS).status).toBe("working");
    const shell = { ...agent, agentId: null, status: "unknown" as const };
    const busy = applyActivity(shell, true, 0);
    expect(busy.status).toBe("working");
    expect(applyActivity(busy, false, 0).status).toBe("unknown");
    expect(applyActivity(shell, false, 0)).toBe(shell);
  });
});

describe("launcher helpers", () => {
  it("keeps five recent folders, newest first, without duplicates", () => {
    let list: string[] = [];
    for (const f of ["/a", "/b", "/c", "/d", "/e", "/f", "/c"]) list = pushRecent(list, f);
    expect(list).toEqual(["/c", "/f", "/e", "/d", "/b"]);
  });

  it("expands and abbreviates the home folder", () => {
    expect(expandHome("~", "/Users/u")).toBe("/Users/u");
    expect(expandHome("~/dev/app", "/Users/u")).toBe("/Users/u/dev/app");
    expect(expandHome("~other", "/Users/u")).toBe("~other");
    expect(expandHome("/tmp", "/Users/u")).toBe("/tmp");
    expect(tildePath("/Users/u/dev", "/Users/u")).toBe("~/dev");
    expect(tildePath("/Users/uv", "/Users/u")).toBe("/Users/uv");
    expect(tildePath("/Users/u", null)).toBe("/Users/u");
  });

  it("titles a pane by its task's first line, else its folder", () => {
    expect(paneTitle("  review: auth\nmore", "/x/app")).toBe("review: auth");
    expect(paneTitle("", "/x/app")).toBe("app");
    expect(paneTitle("x".repeat(80), "/x")).toHaveLength(60);
  });
});

describe("reportBadge", () => {
  const r: Report = { pane: "p2", status: "done", summary: "Fixed auth.", files: [], tests: "pass", ts: 0 };

  it("says the outcome and the tests in words; no tests part for none", () => {
    expect(reportBadge(r).label).toBe("hasil: selesai · tes lulus");
    expect(reportBadge({ ...r, status: "failed", tests: "fail" }).label).toBe("hasil: gagal · tes gagal");
    expect(reportBadge({ ...r, status: "blocked", tests: "none" }).label).toBe("hasil: terhambat");
  });

  it("puts the summary and files in the tooltip", () => {
    const { title } = reportBadge({ ...r, files: ["a.rs", "b.rs"] });
    expect(title).toContain("Fixed auth.");
    expect(title).toContain("Berkas: a.rs, b.rs");
    expect(reportBadge(r).title).not.toContain("Berkas");
  });
});

describe("usageBadge", () => {
  it("shows the 5-hour window and context, amber from 80%", () => {
    const u = { five_hour: { used: 31, resets_at: 1792766400 }, seven_day: { used: 15, resets_at: 0 }, context: 5 };
    const b = usageBadge(u);
    expect(b?.label).toBe("5j 31% · ktx 5%");
    expect(b?.warn).toBe(false);
    expect(b?.title).toContain("Kuota 7 hari: 15% terpakai");
    expect(b?.title).not.toContain("15% terpakai (reset");
    expect(usageBadge({ ...u, seven_day: { used: 80, resets_at: 0 } })?.warn).toBe(true);
  });

  it("hides before the first turn", () => {
    expect(usageBadge({ five_hour: null, seven_day: null, context: null })).toBeNull();
  });
});
