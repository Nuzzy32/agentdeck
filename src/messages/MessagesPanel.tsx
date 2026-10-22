// Messages panel, UI-UX §4.4 (MSG-05). Driven by props: rows come from `messages_list` (src/core/messages.ts),
// Copy and Discard go back to the caller.
import { useEffect, useState } from "react";
import { isWaiting, type MessageRow, type MessageState } from "../core/messages";
import "./messages.css";

/** A row as the panel needs it; the workspace is implied. */
export type PanelMessage = Omit<MessageRow, "workspace">;

export type PaneLabel = { id: string; agent: string; title: string };
export type Filter = "all" | "waiting";

const STATE: Record<MessageState, { text: string; tone: string }> = {
  queued: { text: "menunggu", tone: "" },
  delivered: { text: "terkirim", tone: "ok" },
  paused: { text: "dijeda", tone: "warn" },
  dropped: { text: "dibuang", tone: "danger" },
};

export const visible = (messages: PanelMessage[], filter: Filter) =>
  filter === "all" ? messages : messages.filter(isWaiting);

/** Local `HH:MM`. */
export function clock(ts: number): string {
  const d = new Date(ts);
  if (Number.isNaN(d.getTime())) return "--:--";
  return `${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
}

/** How long a message has waited: `12 dtk`, `3 mnt`, `1 j 5 mnt`. */
export function waited(ts: number, now: number): string {
  const s = Math.max(0, Math.floor((now - ts) / 1000)) || 0;
  if (s < 60) return `${s} dtk`;
  const m = Math.floor(s / 60);
  return m < 60 ? `${m} mnt` : `${Math.floor(m / 60)} j ${m % 60} mnt`;
}

const firstLine = (text: string) => text.split("\n", 1)[0];

/** Full text (selectable), the drop reason, Salin, and Buang for messages still waiting. */
export function MessageDetail({
  id,
  m,
  onCopy,
  onDiscard,
}: {
  id: string;
  m: PanelMessage;
  onCopy: (text: string) => void;
  onDiscard: (id: string) => void;
}) {
  return (
    <div id={id} className="msgs-detail">
      {m.state === "dropped" && m.reason ? <p className="msgs-reason danger">Alasan: {m.reason}</p> : null}
      <pre className="mono msgs-text">{m.text}</pre>
      <div className="msgs-actions">
        <button type="button" className="btn btn-small" onClick={() => onCopy(m.text)}>
          Salin
        </button>
        {isWaiting(m) ? (
          <button type="button" className="btn btn-small msgs-discard" onClick={() => onDiscard(m.id)}>
            Buang
          </button>
        ) : null}
      </div>
    </div>
  );
}

export function MessagesPanel({
  title,
  messages,
  panes,
  now,
  onCopy,
  onDiscard,
}: {
  /** Workspace name. */
  title: string;
  /** Oldest first; the newest row sits at the bottom. */
  messages: PanelMessage[];
  /** For the chip tooltips (agent and title). */
  panes: PaneLabel[];
  /** Clock for the waiting time; ticks every 15 s when omitted. */
  now?: number;
  onCopy: (text: string) => void;
  onDiscard: (id: string) => void;
}) {
  const [filter, setFilter] = useState<Filter>("all");
  const [open, setOpen] = useState<string | null>(null);
  const [tick, setTick] = useState(() => Date.now());
  const anyQueued = messages.some((m) => m.state === "queued");
  useEffect(() => {
    if (now !== undefined || !anyQueued) return;
    const id = setInterval(() => setTick(Date.now()), 15_000);
    return () => clearInterval(id);
  }, [now, anyQueued]);
  const clockNow = now ?? tick;

  const rows = visible(messages, filter);
  const paneTitle = (id: string) => {
    const p = panes.find((x) => x.id === id);
    return p ? `${id} · ${p.agent}${p.title ? `: ${p.title}` : ""}` : id;
  };

  return (
    <aside className="messages" aria-label="Panel Pesan">
      <div className="section-head">
        <span>Pesan · {title}</span>
        <div className="msgs-filter" role="group" aria-label="Filter pesan">
          {(
            [
              ["all", "Semua"],
              ["waiting", "Menunggu"],
            ] as const
          ).map(([value, label]) => (
            <button
              key={value}
              type="button"
              className="text-btn"
              aria-pressed={filter === value}
              onClick={() => setFilter(value)}
            >
              {label}
            </button>
          ))}
        </div>
      </div>
      {rows.length === 0 ? (
        <p className="messages-empty">
          {messages.length === 0
            ? "Belum ada pesan antar agen di workspace ini. Pesan muncul saat agen memakai pane_send."
            : "Tidak ada pesan yang menunggu."}
        </p>
      ) : (
        <ul className="list messages-list">
          {rows.map((m) => {
            const s = STATE[m.state];
            const expanded = open === m.id;
            const detail = `msg-detail-${m.id}`;
            return (
              <li key={`${m.ts}-${m.id}`} className={expanded ? "msgs-item msgs-open" : "msgs-item"}>
                <button
                  type="button"
                  className="message"
                  aria-expanded={expanded}
                  aria-controls={expanded ? detail : undefined}
                  onClick={() => setOpen(expanded ? null : m.id)}
                >
                  <span className="message-line">
                    <span className="mono faint">{clock(m.ts)}</span>
                    <span className="msgs-route">
                      <span className="badge mono" title={paneTitle(m.from)}>
                        {m.from}
                      </span>
                      <span aria-hidden="true">→</span>
                      <span className="sr-only">ke</span>
                      <span className="badge mono" title={paneTitle(m.to)}>
                        {m.to}
                      </span>
                    </span>
                    <span className={`msgs-state ${s.tone}`}>
                      {s.text}
                      {m.state === "queued" ? ` ${waited(m.ts, clockNow)}` : ""}
                    </span>
                  </span>
                  <span className="mono message-preview">{firstLine(m.text)}</span>
                </button>
                {expanded ? <MessageDetail id={detail} m={m} onCopy={onCopy} onDiscard={onDiscard} /> : null}
              </li>
            );
          })}
        </ul>
      )}
    </aside>
  );
}
