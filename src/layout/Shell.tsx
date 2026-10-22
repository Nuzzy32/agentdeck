import { useEffect, useRef } from "react";
import { agentIcon, SHELL } from "../launcher/Launcher";
import { Icon, IconButton } from "./Icon";
import { nearLimit } from "./logic";
import { type AgentInfo, LOOP_LIMIT, type Message, type Pane } from "./model";
import { StatusDot } from "./Pane";

/** What the chrome shows of a workspace: its folder for display (`~/…`) and its panes in layout order. */
export type WorkspaceView = { id: number; name: string; folder: string; panes: Pane[] };

export function TitleBar({
  workspace,
  sidebarOpen,
  messagesOpen,
  onToggleSidebar,
  onToggleMessages,
}: {
  workspace: WorkspaceView | null;
  sidebarOpen: boolean;
  messagesOpen: boolean;
  onToggleSidebar: () => void;
  onToggleMessages: () => void;
}) {
  return (
    <header className="titlebar" data-tauri-drag-region>
      <IconButton
        icon="panelLeft"
        label={sidebarOpen ? "Ciutkan sidebar (⌘B)" : "Tampilkan sidebar (⌘B)"}
        pressed={sidebarOpen}
        onClick={onToggleSidebar}
      />
      <span className="titlebar-name" data-tauri-drag-region>
        {workspace ? workspace.name : "agentdeck"}
      </span>
      {workspace ? (
        <span className="mono titlebar-folder" data-tauri-drag-region>
          {workspace.folder}
        </span>
      ) : null}
      <span className="pane-spacer" data-tauri-drag-region />
      {workspace ? (
        <button
          type="button"
          className="text-btn"
          title="Panel Pesan (⌘⇧M)"
          aria-pressed={messagesOpen}
          onClick={onToggleMessages}
        >
          <Icon name="panelRight" />
          Pesan
        </button>
      ) : null}
    </header>
  );
}

export function Sidebar({
  workspaces,
  active,
  focusedPane,
  onNewWorkspace,
  onSelectWorkspace,
  onFocusPane,
}: {
  workspaces: WorkspaceView[];
  active: WorkspaceView | null;
  focusedPane: string | null;
  onNewWorkspace: () => void;
  onSelectWorkspace: (id: number) => void;
  onFocusPane: (id: string) => void;
}) {
  return (
    <nav className="sidebar" aria-label="Workspace dan pane">
      <div className="section-head">
        <span>Workspace</span>
        <IconButton icon="plus" label="Workspace baru (⌘T)" onClick={onNewWorkspace} />
      </div>
      {workspaces.length === 0 ? <p className="sidebar-empty">Belum ada workspace.</p> : null}
      <ul className="list">
        {workspaces.map((ws, i) => {
          const isActive = ws.id === active?.id;
          const needsYou = ws.panes.some((p) => p.status === "needs_you");
          return (
            <li key={ws.id}>
              <button
                type="button"
                className={`row row-workspace${isActive ? " row-active" : ""}`}
                aria-current={isActive ? "true" : undefined}
                title={`${ws.name} · ${ws.folder}${i < 9 ? ` (⌘${i + 1})` : ""}`}
                onClick={() => onSelectWorkspace(ws.id)}
              >
                <span className="row-line">
                  <span className="row-name">{ws.name}</span>
                  <span className="row-meta">{ws.panes.length} pane</span>
                </span>
                <span className="row-line">
                  <span className="mono row-name row-folder">{ws.folder}</span>
                  {needsYou ? (
                    <span className="row-meta warn">
                      <span className="dot dot-needs_you" aria-hidden="true">
                        !
                      </span>
                      butuh kamu
                    </span>
                  ) : null}
                </span>
              </button>
            </li>
          );
        })}
      </ul>

      {active ? (
        <>
          <div className="section-head">
            <span>Pane · {active.name}</span>
          </div>
          <ul className="list">
            {active.panes.map((pane) => (
              <li key={pane.id}>
                <button
                  type="button"
                  className={`row${pane.parent ? " row-child" : ""}${pane.id === focusedPane ? " row-active" : ""}`}
                  aria-current={pane.id === focusedPane ? "true" : undefined}
                  title={`Fokus ${pane.id} · ${pane.agent}: ${pane.title}`}
                  onClick={() => onFocusPane(pane.id)}
                >
                  {pane.parent ? (
                    <span className="tree-elbow" aria-hidden="true">
                      └
                    </span>
                  ) : null}
                  <StatusDot pane={pane} />
                  <span className="mono">{pane.id}</span>
                  <span className="row-name">{pane.agent}</span>
                  {pane.pending > 0 ? (
                    <span className="count" aria-label={`${pane.pending} pesan menunggu`}>
                      {pane.pending}
                    </span>
                  ) : null}
                </button>
              </li>
            ))}
          </ul>
        </>
      ) : null}

      <button type="button" className="row sidebar-settings" title="Settings (⌘,)">
        <Icon name="settings" />
        Settings
      </button>
    </nav>
  );
}

const MESSAGE_STATUS: Record<Message["status"], { text: string; tone: string }> = {
  queued: { text: "menunggu", tone: "" },
  delivered: { text: "terkirim", tone: "ok" },
  paused: { text: "dijeda", tone: "warn" },
  dropped: { text: "dibuang", tone: "danger" },
};

export function MessagesPanel({ workspace, messages }: { workspace: WorkspaceView; messages: Message[] }) {
  return (
    <aside className="messages" aria-label="Panel Pesan">
      <div className="section-head">
        <span>Pesan · {workspace.name}</span>
        <button type="button" className="text-btn" title="Filter pesan">
          Semua ▾
        </button>
      </div>
      {messages.length === 0 ? (
        <p className="messages-empty">
          Belum ada pesan antar agen di workspace ini. Pesan muncul saat agen memakai pane_send.
        </p>
      ) : (
        <ul className="list messages-list">
          {messages.map((m) => {
            const s = MESSAGE_STATUS[m.status];
            return (
              <li key={`${m.time}-${m.from}`}>
                <button type="button" className="message" title="Buka isi lengkap">
                  <span className="message-line">
                    <span className="mono faint">{m.time}</span>
                    <span className="mono">
                      {m.from} → {m.to}
                    </span>
                    <span className={s.tone}>
                      {s.text}
                      {m.waited ? ` ${m.waited}` : ""}
                    </span>
                  </span>
                  <span className="mono message-preview">{m.preview}</span>
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </aside>
  );
}

export function StatusBar({ loopCount }: { loopCount: number }) {
  const amber = nearLimit(loopCount, LOOP_LIMIT);
  return (
    <footer className="statusbar">
      <button type="button" className="status-item" title="Server MCP lokal berjalan">
        <span className="dot dot-idle" aria-hidden="true" />
        MCP aktif
      </button>
      <button type="button" className="status-item" title="Buka Settings › Kolaborasi">
        Agen membuka pane: mati
      </button>
      <button type="button" className="status-item" title="Ubah sakelar saling kirim workspace ini">
        Saling kirim: mati
      </button>
      <button
        type="button"
        className={`status-item${amber ? " warn" : ""}`}
        title="Pesan antar agen sejak masukan terakhirmu"
      >
        Pesan antar agen {loopCount}/{LOOP_LIMIT}
      </button>
    </footer>
  );
}

/** UI-UX §4.7. Each choice opens the launcher with that program selected; agents not yet checked show as such. */
export function EmptyState({ agents, onStart }: { agents: AgentInfo[] | null; onStart: (choice: string) => void }) {
  const choices = [
    { id: SHELL, name: "Shell", installed: true as boolean | null },
    ...(agents ?? [
      { id: "claude-code", name: "Claude Code", installed: null },
      { id: "codex", name: "Codex", installed: null },
    ]),
  ];
  return (
    <div className="empty">
      <h1 className="empty-title">Mulai kerja</h1>
      <div className="empty-choices">
        {choices.map((agent) => (
          <button
            key={agent.id}
            type="button"
            className="empty-choice"
            disabled={agent.installed === false}
            onClick={() => onStart(agent.id)}
          >
            <Icon name={agentIcon(agent.id)} size={20} />
            <span className="empty-choice-name">{agent.name}</span>
            <span className={agent.installed ? "ok" : "faint"}>
              {agent.installed === null ? "Memeriksa…" : agent.installed ? "Terpasang" : "Belum terpasang"}
            </span>
          </button>
        ))}
      </div>
      <p className="empty-text">
        Buka beberapa agen berdampingan.
        <br />
        Agen bisa saling memberi tugas setelah kamu mengizinkannya di Settings.
      </p>
      <button type="button" className="link-btn">
        Settings › Kolaborasi
      </button>
    </div>
  );
}

/**
 * "Stop … in pN?" before closing a pane (UI-UX §3, SPN-04). Native modal dialog: Esc cancels, focus stays inside, and
 * Batal is focused by default so a stray Enter does not kill the process.
 */
export function CloseDialog({
  pane,
  childCount,
  onConfirm,
  onCancel,
}: {
  pane: Pane;
  childCount: number;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const dlg = ref.current;
    if (!dlg) return;
    dlg.showModal();
    cancel.current?.focus();
    return () => dlg.close();
  }, []);
  return (
    <dialog
      ref={ref}
      className="dialog"
      role="alertdialog"
      aria-labelledby="close-title"
      onCancel={(e) => {
        e.preventDefault();
        onCancel();
      }}
    >
      <p id="close-title" className="dialog-title">
        Hentikan {pane.agent} di {pane.id}?
      </p>
      {childCount > 0 ? (
        <p className="dialog-text">
          {pane.id} punya {childCount} anak yang masih hidup; anak tidak ikut ditutup.
        </p>
      ) : null}
      <div className="dialog-actions">
        <button type="button" ref={cancel} className="btn" onClick={onCancel}>
          Batal
        </button>
        <button type="button" className="btn btn-danger" onClick={onConfirm}>
          Hentikan
        </button>
      </div>
    </dialog>
  );
}

export type Toast = { id: number; text: string };

/** Bottom-right toasts (UI-UX §5): at most 3, polite live region, never take focus. */
export function Toasts({ toasts, onDismiss }: { toasts: Toast[]; onDismiss: (id: number) => void }) {
  return (
    <div className="toasts" aria-live="polite">
      {toasts.map((t) => (
        <div key={t.id} className="toast">
          <span>{t.text}</span>
          <IconButton icon="x" label="Tutup pemberitahuan" onClick={() => onDismiss(t.id)} />
        </div>
      ))}
    </div>
  );
}
