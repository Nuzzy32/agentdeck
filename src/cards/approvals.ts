// Spawn approval cards (UI-UX §4.2): the core's contract and the pure logic behind the cards.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** A pending card from the core (`approvals.rs`). Times are ms since the epoch. */
export type Approval = {
  id: string;
  pane: string;
  requester_agent: string;
  agent: string;
  title: string;
  cwd: string;
  task: string;
  created_at: number;
  expires_at: number;
};

export type Outcome = "approved" | "denied" | "timeout";

/** A card on screen: pending, or showing "Kedaluwarsa" for `EXPIRED_MS` after its time ran out. */
export type Card = Approval & { expired?: boolean };

export const EXPIRED_MS = 3000;

export const approvalClient = {
  pending: () => invoke<Approval[]>("approvals_pending"),
  // False when the card was no longer pending in the core.
  answer: (id: string, allow: boolean) => invoke<boolean>("approval_answer", { id, allow }),
  onOpened: (cb: (a: Approval) => void): Promise<UnlistenFn> =>
    listen<Approval>("agentdeck://approval", (e) => cb(e.payload)),
  onClosed: (cb: (id: string, outcome: Outcome) => void): Promise<UnlistenFn> =>
    listen<{ id: string; outcome: Outcome }>("agentdeck://approval-closed", (e) => cb(e.payload.id, e.payload.outcome)),
};

/** Adds cards not yet shown, keeping the stack oldest first. */
export function opened(cards: Card[], add: Approval[]): Card[] {
  const fresh = add.filter((a) => !cards.some((c) => c.id === a.id));
  return fresh.length === 0 ? cards : [...cards, ...fresh].sort((a, b) => a.created_at - b.created_at);
}

/** A timed-out card turns into "Kedaluwarsa"; an answered or cancelled one goes at once. */
export function closed(cards: Card[], id: string, outcome: Outcome): Card[] {
  return outcome === "timeout"
    ? cards.map((c) => (c.id === id ? { ...c, expired: true } : c))
    : cards.filter((c) => c.id !== id);
}

/** Time left as "m:ss" and as a 0–1 share of the card's whole time, for the text and the thin bar. */
export function countdown(card: Pick<Approval, "created_at" | "expires_at">, now: number) {
  const left = Math.max(0, card.expires_at - now);
  const secs = Math.ceil(left / 1000);
  const total = card.expires_at - card.created_at;
  return {
    text: `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, "0")}`,
    share: total > 0 ? Math.min(1, left / total) : 0,
  };
}

/** Keys on a focused card: ⌘Enter allows, Esc denies; anything else is left alone (Tab moves between buttons). */
export function cardKey(e: Pick<KeyboardEvent, "key" | "metaKey">): "allow" | "deny" | null {
  if (e.key === "Enter" && e.metaKey) return "allow";
  if (e.key === "Escape") return "deny";
  return null;
}

/**
 * ⌘J: the oldest pending card first, then each newer one, then the loop guard ribbon, then round again.
 * `current` is the card id (or "ribbon") holding focus now, if any.
 */
export function nextJump(cards: Card[], ribbon: boolean, current: string | null): string | null {
  const targets = [...cards.filter((c) => !c.expired).map((c) => c.id), ...(ribbon ? ["ribbon"] : [])];
  if (targets.length === 0) return null;
  const i = current === null ? -1 : targets.indexOf(current);
  return targets[(i + 1) % targets.length] ?? null;
}
