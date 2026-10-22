import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { type Card, cardKey, closed, countdown, nextJump, opened } from "./approvals";
import { ApprovalCard, ApprovalStack } from "./Cards";

const T0 = Date.parse("2026-10-22T14:00:00Z");
const card = (id: string, at = 0, extra: Partial<Card> = {}): Card => ({
  id,
  pane: "p1",
  requester_agent: "Claude Code",
  agent: "Codex",
  title: "review: auth",
  cwd: "/Users/me/dev/app",
  task: "Review the diff on branch feat/auth for bugs.",
  created_at: T0 + at,
  expires_at: T0 + at + 120_000,
  ...extra,
});
const folder = (p: string) => p.replace("/Users/me", "~");

type Key = { key: string; metaKey: boolean; preventDefault: () => void; stopPropagation: () => void };
const press = (c: Card, key: string, metaKey = false) => {
  const onAnswer = vi.fn();
  const el = ApprovalCard({ card: c, now: T0, folder, onAnswer }) as ReactElement<{ onKeyDown: (e: Key) => void }>;
  const e = { key, metaKey, preventDefault: vi.fn(), stopPropagation: vi.fn() };
  el.props.onKeyDown(e);
  return { onAnswer, e };
};

describe("countdown", () => {
  it("shows m:ss and the share left for the thin bar", () => {
    expect(countdown(card("a1"), T0 + 13_000)).toEqual({ text: "1:47", share: 107 / 120 });
    expect(countdown(card("a1"), T0 + 119_001).text).toBe("0:01");
    expect(countdown(card("a1"), T0 + 200_000)).toEqual({ text: "0:00", share: 0 });
  });
});

describe("keys on a focused card", () => {
  it("⌘Enter allows, Esc denies, plain Enter and Tab are left alone", () => {
    expect(cardKey({ key: "Enter", metaKey: true })).toBe("allow");
    expect(cardKey({ key: "Escape", metaKey: false })).toBe("deny");
    expect(cardKey({ key: "Enter", metaKey: false })).toBeNull();
    expect(cardKey({ key: "Tab", metaKey: false })).toBeNull();

    const allow = press(card("a1"), "Enter", true);
    expect(allow.onAnswer).toHaveBeenCalledWith("a1", true);
    expect(allow.e.stopPropagation).toHaveBeenCalled();
    expect(press(card("a1"), "Escape").onAnswer).toHaveBeenCalledWith("a1", false);
    expect(press(card("a1"), "a").onAnswer).not.toHaveBeenCalled();
  });

  it("an expired card ignores them", () => {
    expect(press(card("a1", 0, { expired: true }), "Enter", true).onAnswer).not.toHaveBeenCalled();
  });
});

describe("card", () => {
  const html = (c: Card) => renderToStaticMarkup(<ApprovalCard card={c} now={T0 + 13_000} folder={folder} onAnswer={vi.fn()} />);

  it("shows who asks for what, the countdown and both buttons, as an alertdialog", () => {
    const out = html(card("a1"));
    expect(out).toContain('role="alertdialog"');
    expect(out).toContain('aria-labelledby="a1-title"');
    expect(out).toContain('aria-describedby="a1-task"');
    expect(out).toContain("Claude Code ingin membuka agen");
    expect(out).toContain("Codex · &quot;review: auth&quot; · ");
    expect(out).toContain("~/dev/app");
    expect(out).toContain("1:47");
    expect(out).toContain("Tolak");
    expect(out).toContain("Izinkan ⌘↩");
  });

  it("does not steal focus: no autofocus, and only ⌘J or a click puts focus on it", () => {
    const out = html(card("a1"));
    expect(out.toLowerCase()).not.toContain("autofocus");
    expect(out).toMatch(/^<div[^>]*tabindex="-1"/);
  });

  it("turns into Kedaluwarsa without buttons when its time ran out", () => {
    const out = html(card("a1", 0, { expired: true }));
    expect(out).toContain("Kedaluwarsa");
    expect(out).not.toContain("Izinkan");
  });
});

describe("stack", () => {
  it("is a polite live region even when empty, oldest card on top", () => {
    expect(renderToStaticMarkup(<ApprovalStack cards={[]} folder={folder} onAnswer={vi.fn()} />)).toBe(
      '<div class="pane-cards" aria-live="polite"></div>',
    );
    const list = opened([], [card("a2", 5000), card("a1", 0)]);
    const out = renderToStaticMarkup(<ApprovalStack cards={list} folder={folder} onAnswer={vi.fn()} />);
    expect(out.indexOf('data-approval="a1"')).toBeLessThan(out.indexOf('data-approval="a2"'));
  });

  it("adds each card once and keeps it until closed; a timeout leaves it as expired", () => {
    let list = opened([], [card("a1")]);
    list = opened(list, [card("a1"), card("a3", 9000), card("a2", 4000)]);
    expect(list.map((c) => c.id)).toEqual(["a1", "a2", "a3"]);
    expect(closed(list, "a2", "approved").map((c) => c.id)).toEqual(["a1", "a3"]);
    expect(closed(list, "a1", "denied").map((c) => c.id)).toEqual(["a2", "a3"]);
    const timedOut = closed(list, "a3", "timeout");
    expect(timedOut.map((c) => [c.id, c.expired ?? false])).toEqual([
      ["a1", false],
      ["a2", false],
      ["a3", true],
    ]);
  });
});

describe("⌘J", () => {
  const list = [card("a1"), card("a2", 1000), card("a3", 2000, { expired: true })];

  it("goes to the oldest pending card first, then newer ones, then the ribbon, then round again", () => {
    expect(nextJump(list, true, null)).toBe("a1");
    expect(nextJump(list, true, "a1")).toBe("a2");
    expect(nextJump(list, true, "a2")).toBe("ribbon");
    expect(nextJump(list, true, "ribbon")).toBe("a1");
    expect(nextJump(list, false, "a2")).toBe("a1");
  });

  it("goes to the ribbon alone when no card waits, and nowhere when neither is there", () => {
    expect(nextJump([], true, null)).toBe("ribbon");
    expect(nextJump([card("a3", 0, { expired: true })], false, null)).toBeNull();
  });
});
