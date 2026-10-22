import { Icon, IconButton, type IconName } from "./Icon";
import { nearLimit } from "./logic";
import { StatusDot } from "./Pane";
import { AGENTS, LOOP_COUNT, LOOP_LIMIT, type Message, type Workspace } from "./sample";

export function TitleBar({
  workspace,
  sidebarOpen,
  messagesOpen,
  onToggleSidebar,
  onToggleMessages,
}: {
  workspace: Workspace | null;
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
  onSelectWorkspace,
  onFocusPane,
}: {
  workspaces: Workspace[];
  active: Workspace | null;
  focusedPane: string | null;
  onSelectWorkspace: (name: string) => void;
  onFocusPane: (id: string) => void;
}) {
  return (
    <nav className="sidebar" aria-label="Workspace dan pane">
      <div className="section-head">
        <span>Workspace</span>
        <IconButton icon="plus" label="Workspace baru (⌘T)" />
      </div>
      {workspaces.length === 0 ? <p className="sidebar-empty">Belum ada workspace.</p> : null}
      <ul className="list">
        {workspaces.map((ws) => {
          const needsYou = ws.panes.some((p) => p.status === "needs_you");
          return (
            <li key={ws.name}>
              <button
                type="button"
                className={`row row-workspace${ws === active ? " row-active" : ""}`}
                aria-current={ws === active ? "true" : undefined}
                title={`${ws.name} · ${ws.folder}`}
                onClick={() => onSelectWorkspace(ws.name)}
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

export function MessagesPanel({ workspace, messages }: { workspace: Workspace; messages: Message[] }) {
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

export function StatusBar() {
  const amber = nearLimit(LOOP_COUNT, LOOP_LIMIT);
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
        Pesan antar agen {LOOP_COUNT}/{LOOP_LIMIT}
      </button>
    </footer>
  );
}

const AGENT_ICON: Record<string, IconName> = { Shell: "terminal", "Claude Code": "sparkles", Codex: "bot" };

export function EmptyState({ onStart }: { onStart: () => void }) {
  return (
    <div className="empty">
      <h1 className="empty-title">Mulai kerja</h1>
      <div className="empty-choices">
        {AGENTS.map((agent) => (
          <button
            key={agent.name}
            type="button"
            className="empty-choice"
            disabled={!agent.installed}
            onClick={onStart}
          >
            <Icon name={AGENT_ICON[agent.name] ?? "terminal"} size={20} />
            <span className="empty-choice-name">{agent.name}</span>
            <span className={agent.installed ? "ok" : "faint"}>{agent.installed ? "Terpasang" : "Belum terpasang"}</span>
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
