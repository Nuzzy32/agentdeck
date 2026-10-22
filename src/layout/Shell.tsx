import { type Ref, useEffect, useId, useRef } from "react";
import type { LoopGuard } from "../core/messages";
import { agentIcon, SHELL } from "../launcher/Launcher";
import { Icon, IconButton } from "./Icon";
import { nearLimit } from "./logic";
import { MessagesPanel as MessagesList, type PanelMessage } from "../messages/MessagesPanel";
import type { AgentInfo, Pane } from "./model";
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
  onSettings,
}: {
  workspaces: WorkspaceView[];
  active: WorkspaceView | null;
  focusedPane: string | null;
  onNewWorkspace: () => void;
  onSelectWorkspace: (id: number) => void;
  onFocusPane: (id: string) => void;
  onSettings: () => void;
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

      <button type="button" className="row sidebar-settings" title="Settings (⌘,)" onClick={onSettings}>
        <Icon name="settings" />
        Settings
      </button>
    </nav>
  );
}

/** UI-UX §4.4. Copy goes to the clipboard; Discard and Send now go to the core's queue. */
export function MessagesPanel({
  workspace,
  messages,
  onDiscard,
  onForce,
}: {
  workspace: WorkspaceView;
  messages: PanelMessage[];
  onDiscard: (id: string) => void;
  onForce: (id: string) => void;
}) {
  return (
    <MessagesList
      title={workspace.name}
      messages={messages}
      panes={workspace.panes}
      onCopy={(text) => void navigator.clipboard.writeText(text).catch(() => {})}
      onDiscard={onDiscard}
      onForce={onForce}
    />
  );
}

/**
 * Loop guard ribbon, UI-UX §4.3 (MSG-04). The live region is always there so the ribbon is announced when it appears;
 * it never takes focus by itself, ⌘J moves focus to `focusRef` (Lanjutkan).
 */
export function LoopRibbon({
  guard,
  focusRef,
  onContinue,
  onStop,
  onShowLog,
}: {
  guard: LoopGuard;
  focusRef?: Ref<HTMLButtonElement>;
  onContinue: () => void;
  onStop: () => void;
  onShowLog: () => void;
}) {
  return (
    <div aria-live="polite">
      {guard.paused ? (
        <div className="loop-ribbon" role="group" aria-label="Pengaman putaran">
          <Icon name="alert" size={14} />
          <span className="loop-ribbon-text">
            Agen sudah saling mengirim {guard.limit} pesan tanpa masukanmu. Antrean dijeda.
          </span>
          <button type="button" ref={focusRef} className="btn btn-small" onClick={onContinue}>
            Lanjutkan
          </button>
          <button type="button" className="btn btn-small" onClick={onStop}>
            Hentikan dan buang antrean
          </button>
          <button type="button" className="btn btn-small" onClick={onShowLog}>
            Lihat log
          </button>
        </div>
      ) : null}
    </div>
  );
}

/** UI-UX §4.5. MCP stays static until the core reports it. */
export function StatusBar({
  guard,
  peer,
  spawn,
  onPeer,
  onSpawnSettings,
}: {
  guard: LoopGuard;
  /** Null without a workspace: the switch has nothing to act on. */
  peer: boolean | null;
  /** "Agen boleh membuka pane" from Settings. */
  spawn: boolean;
  /** Asks for the switch to become `on`. */
  onPeer: (on: boolean) => void;
  /** Opens Settings › Kolaborasi. */
  onSpawnSettings: () => void;
}) {
  const amber = nearLimit(guard.count, guard.limit);
  return (
    <footer className="statusbar">
      <button type="button" className="status-item" title="Server MCP lokal berjalan">
        <span className="dot dot-idle" aria-hidden="true" />
        MCP aktif
      </button>
      <button type="button" className="status-item" title="Buka Settings › Kolaborasi" onClick={onSpawnSettings}>
        Agen membuka pane: {spawn ? "aktif" : "mati"}
      </button>
      <button
        type="button"
        className="status-item"
        title={peer ? "Matikan saling kirim di workspace ini" : "Izinkan agen di workspace ini saling kirim"}
        aria-pressed={peer ?? false}
        disabled={peer === null}
        onClick={() => onPeer(!peer)}
      >
        Saling kirim: {peer ? "aktif" : "mati"}
      </button>
      <button
        type="button"
        className={`status-item${amber ? " warn" : ""}`}
        title="Pesan antar agen sejak masukan terakhirmu"
        aria-label={`Pesan antar agen ${guard.count} dari ${guard.limit}${amber ? ", mendekati batas" : ""}`}
      >
        Pesan antar agen {guard.count}/{guard.limit}
      </button>
    </footer>
  );
}

/** UI-UX §4.7. Each choice opens the launcher with that program selected; agents not yet checked show as such. */
export function EmptyState({
  agents,
  onStart,
  onSettings,
}: {
  agents: AgentInfo[] | null;
  onStart: (choice: string) => void;
  /** Opens Settings › Kolaborasi. */
  onSettings: () => void;
}) {
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
      <button type="button" className="link-btn" onClick={onSettings}>
        Settings › Kolaborasi
      </button>
    </div>
  );
}

/**
 * "Stop … in pN?" before closing a pane (UI-UX §3, SPN-04).
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
  return (
    <ConfirmDialog
      title={`Hentikan ${pane.agent} di ${pane.id}?`}
      text={childCount > 0 ? `${pane.id} punya ${childCount} anak yang masih hidup; anak tidak ikut ditutup.` : null}
      confirm="Hentikan"
      danger
      onConfirm={onConfirm}
      onCancel={onCancel}
    />
  );
}

/** Before "Saling kirim" goes on (UI-UX §4.5, MSG-06). */
export function PeerDialog({
  workspace,
  limit,
  onConfirm,
  onCancel,
}: {
  workspace: string;
  limit: number;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <ConfirmDialog
      title={`Izinkan agen di ${workspace} saling kirim?`}
      text={`Agen bisa mengirim prompt ke pane lain di workspace ini, bukan hanya ke induk atau anaknya. Antrean dijeda setelah ${limit} pesan tanpa masukanmu.`}
      confirm="Nyalakan"
      onConfirm={onConfirm}
      onCancel={onCancel}
    />
  );
}

/**
 * Native modal dialog: Esc cancels, focus stays inside, and Batal is focused by default so a stray Enter does not
 * confirm.
 */
function ConfirmDialog({
  title,
  text,
  confirm,
  danger = false,
  onConfirm,
  onCancel,
}: {
  title: string;
  text: string | null;
  confirm: string;
  danger?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const titleId = useId();
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
      aria-labelledby={titleId}
      onCancel={(e) => {
        e.preventDefault();
        onCancel();
      }}
    >
      <p id={titleId} className="dialog-title">
        {title}
      </p>
      {text ? <p className="dialog-text">{text}</p> : null}
      <div className="dialog-actions">
        <button type="button" ref={cancel} className="btn" onClick={onCancel}>
          Batal
        </button>
        <button type="button" className={danger ? "btn btn-danger" : "btn btn-primary"} onClick={onConfirm}>
          {confirm}
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
