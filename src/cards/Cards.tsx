import { type KeyboardEvent, useEffect, useState } from "react";
import { type Approval, approvalClient, type Card, cardKey, closed, countdown, EXPIRED_MS, opened } from "./approvals";
import "./cards.css";

type Answer = (id: string, allow: boolean) => void;

/**
 * One spawn card (UI-UX §4.2). It never takes focus itself: ⌘J (App) moves focus here, then ⌘Enter allows, Esc
 * denies and Tab moves between the controls.
 */
export function ApprovalCard({
  card,
  now,
  folder,
  onAnswer,
}: {
  card: Card;
  now: number;
  folder: (path: string) => string;
  onAnswer: Answer;
}) {
  const { text, share } = countdown(card, now);
  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const action = card.expired ? null : cardKey(e);
    if (!action) return;
    e.preventDefault();
    e.stopPropagation();
    onAnswer(card.id, action === "allow");
  };
  return (
    <div
      className={`approval${card.expired ? " approval-expired" : ""}`}
      role="alertdialog"
      aria-labelledby={`${card.id}-title`}
      aria-describedby={`${card.id}-task`}
      data-approval={card.id}
      tabIndex={-1}
      onKeyDown={onKeyDown}
    >
      <div className="approval-head">
        <span id={`${card.id}-title`} className="approval-title">
          <span className="mono">{card.pane}</span> · {card.requester_agent} ingin membuka agen
        </span>
        <span className="mono approval-time">{card.expired ? "Kedaluwarsa" : text}</span>
      </div>
      <div className="approval-bar" aria-hidden="true">
        <div style={{ transform: `scaleX(${card.expired ? 0 : share})` }} />
      </div>
      <div className="approval-meta">
        {card.agent} · "{card.title}" · <span className="mono">{folder(card.cwd)}</span>
      </div>
      <pre id={`${card.id}-task`} className="mono approval-task" tabIndex={0}>
        {card.task}
      </pre>
      {card.expired ? null : (
        <div className="approval-actions">
          <button type="button" className="btn btn-small" onClick={() => onAnswer(card.id, false)}>
            Tolak
          </button>
          <button type="button" className="btn btn-small btn-primary" onClick={() => onAnswer(card.id, true)}>
            Izinkan ⌘↩
          </button>
        </div>
      )}
    </div>
  );
}

/** The cards of one pane, oldest on top, inside a polite live region that is there before any card arrives. */
export function ApprovalStack({
  cards,
  folder,
  onAnswer,
}: {
  cards: Card[];
  folder: (path: string) => string;
  onAnswer: Answer;
}) {
  const [now, setNow] = useState(Date.now);
  const ticking = cards.length > 0;
  useEffect(() => {
    if (!ticking) return;
    setNow(Date.now());
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [ticking]);
  return (
    <div className="pane-cards" aria-live="polite">
      {cards.map((c) => (
        <ApprovalCard key={c.id} card={c} now={now} folder={folder} onAnswer={onAnswer} />
      ))}
    </div>
  );
}

/** Every card of the app, oldest first, kept in step with the core's events; reloads pick up pending cards. */
export function useApprovals(client = approvalClient) {
  const [cards, setCards] = useState<Card[]>([]);
  useEffect(() => {
    let live = true;
    const drop = (id: string) => setCards((list) => list.filter((c) => c.id !== id));
    const offs = [
      client.onOpened((a: Approval) => setCards((list) => opened(list, [a]))),
      client.onClosed((id, outcome) => {
        setCards((list) => closed(list, id, outcome));
        if (outcome === "timeout") setTimeout(() => live && drop(id), EXPIRED_MS);
      }),
    ];
    client.pending().then(
      (list) => live && setCards((c) => opened(c, list)),
      (e: unknown) => console.error("approvals_pending", e),
    );
    return () => {
      live = false;
      for (const off of offs) off.then((f) => f(), () => undefined);
    };
  }, [client]);
  // A card the core no longer has (closed while the WebView reloaded) just goes away when answered.
  const answer: Answer = (id, allow) =>
    void client.answer(id, allow).then(
      (pending) => pending || setCards((list) => list.filter((c) => c.id !== id)),
      (e: unknown) => console.error("approval_answer", e),
    );
  return { cards, answer };
}
