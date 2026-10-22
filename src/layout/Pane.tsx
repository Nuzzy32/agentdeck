import { TerminalPane } from "../pane/TerminalPane";
import { IconButton } from "./Icon";
import { splitMiddle } from "./logic";
import { type Pane, statusLabel } from "./sample";

/** Status dot; shape differs per status so it never relies on colour alone (UI-UX §1.3, §3). */
export function StatusDot({ pane }: { pane: Pane }) {
  return (
    <span className={`dot dot-${pane.status}`} aria-hidden="true">
      {pane.status === "needs_you" ? "!" : null}
    </span>
  );
}

export function PaneView({ pane, focused, onFocus }: { pane: Pane; focused: boolean; onFocus: () => void }) {
  const label = statusLabel(pane);
  const { head, tail } = splitMiddle(pane.folder);
  return (
    <section
      className={`pane${focused ? " pane-focused" : ""}`}
      aria-label={`${pane.id} ${pane.agent}: ${pane.title}`}
      onMouseDown={onFocus}
    >
      <header className="pane-header">
        <span className="pane-status" title={label}>
          <StatusDot pane={pane} />
          <span className="pane-status-label">{label}</span>
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
        {pane.pending > 0 ? <span className="badge chip">{pane.pending} pesan</span> : null}
        <span className="pane-actions">
          <IconButton icon="splitRight" label="Split kanan (⌘D)" />
          <IconButton icon="splitDown" label="Split bawah (⌘⇧D)" />
          <IconButton icon="maximize" label="Perbesar (⌘⇧Enter)" />
          <IconButton icon="x" label="Tutup pane (⌘W)" />
        </span>
      </header>
      {pane.ptyId !== undefined ? (
        <TerminalPane id={pane.ptyId} focused={focused} />
      ) : (
        <div className="pane-body mono">
          {pane.agent} di {pane.folder} (data contoh)
        </div>
      )}
    </section>
  );
}
