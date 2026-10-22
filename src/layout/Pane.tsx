import { type ReactNode, useState } from "react";
import { TerminalPane } from "../pane/TerminalPane";
import { Icon, IconButton } from "./Icon";
import { splitMiddle } from "./logic";
import { type Pane, statusLabel } from "./model";

/** Status dot; shape differs per status so it never relies on colour alone (UI-UX §1.3, §3). */
export function StatusDot({ pane }: { pane: Pane }) {
  return (
    <span className={`dot dot-${pane.status}`} aria-hidden="true">
      {pane.status === "needs_you" ? "!" : null}
    </span>
  );
}

/** Copyable install command for a missing CLI (UI-UX §5). The text stays selectable if the clipboard fails. */
export function InstallHint({ command }: { command: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <span className="install">
      <code className="mono install-cmd">{command}</code>
      <button
        type="button"
        className="btn btn-small"
        onClick={() =>
          navigator.clipboard.writeText(command).then(
            () => setCopied(true),
            () => setCopied(false),
          )
        }
      >
        <Icon name="copy" size={12} />
        {copied ? "Disalin" : "Salin"}
      </button>
    </span>
  );
}

export type PaneActions = {
  onFocus: () => void;
  onSplit: (dir: "row" | "col") => void;
  onZoom: () => void;
  onClose: () => void;
  onActivity: (busy: boolean) => void;
  /** Same program again, in the same pane (exited or failed). */
  onRestart: () => void;
  /** Replace the exited program with a shell in its folder. */
  onShellHere: () => void;
};

export function PaneView({
  pane,
  focused,
  zoomed,
  fontSize,
  actions,
  cards,
}: {
  pane: Pane;
  focused: boolean;
  zoomed: boolean;
  fontSize: number;
  actions: PaneActions;
  /** Spawn approval cards this pane asked for (UI-UX §4.2), shown over the top of its terminal. */
  cards?: ReactNode;
}) {
  const label = statusLabel(pane);
  const { head, tail } = splitMiddle(pane.folder);
  return (
    <section
      className={`pane${focused ? " pane-focused" : ""}`}
      aria-label={`${pane.id} ${pane.agent}: ${pane.title}`}
      onMouseDown={actions.onFocus}
    >
      <header className="pane-header">
        <span className="pane-status" title={pane.msg ?? label}>
          <StatusDot pane={pane} />
          <span className="pane-status-label">{label}</span>
          {pane.error ? (
            <span className="pane-error" role="img" aria-label="agen melapor galat" title="Agen melapor galat">
              <Icon name="alert" size={12} />
            </span>
          ) : null}
        </span>
        <span className="mono pane-id">{pane.id}</span>
        <span className="pane-agent">{pane.agent}</span>
        <span className="pane-title" title={pane.title}>
          {pane.title}
        </span>
        <span className="mono pane-folder" title={pane.folder}>
          <span className="pane-folder-head">{head}</span>
          <span className="pane-folder-tail">{tail}</span>
        </span>
        <span className="pane-spacer" />
        {pane.parent ? <span className="badge pane-relation">anak {pane.parent}</span> : null}
        {pane.pending > 0 ? (
          <span className="badge chip" title={`${pane.pending} pesan menunggu dikirim ke ${pane.id}`}>
            {pane.pending} pesan
          </span>
        ) : null}
        <span className="pane-actions">
          <IconButton icon="splitRight" label="Split kanan (⌘D)" onClick={() => actions.onSplit("row")} />
          <IconButton icon="splitDown" label="Split bawah (⌘⇧D)" onClick={() => actions.onSplit("col")} />
          <IconButton
            icon={zoomed ? "minimize" : "maximize"}
            label={zoomed ? "Kembalikan (⌘⇧Enter)" : "Perbesar (⌘⇧Enter)"}
            pressed={zoomed}
            onClick={actions.onZoom}
          />
          <IconButton icon="x" label="Tutup pane (⌘W)" onClick={actions.onClose} />
        </span>
      </header>
      {cards}
      {pane.failed ? (
        <div className="pane-body pane-failed" role="alert">
          <p className="pane-failed-text">{pane.failed.message}</p>
          {pane.failed.install ? <InstallHint command={pane.failed.install} /> : null}
          <button type="button" className="btn" onClick={actions.onRestart}>
            Coba lagi
          </button>
        </div>
      ) : pane.ptyId !== null ? (
        <TerminalPane id={pane.ptyId} focused={focused} fontSize={fontSize} onActivity={actions.onActivity} />
      ) : (
        <div className="pane-body" />
      )}
      {pane.status === "exited" ? (
        <div className="pane-ribbon" role="status">
          <span>
            {pane.agent} selesai (kode {pane.exitCode ?? "?"})
          </span>
          <span className="pane-spacer" />
          <button type="button" className="btn btn-small" onClick={actions.onRestart}>
            Jalankan ulang
          </button>
          <button type="button" className="btn btn-small" onClick={actions.onShellHere}>
            Buka shell di sini
          </button>
        </div>
      ) : null}
    </section>
  );
}
