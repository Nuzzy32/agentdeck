import { TerminalPane } from "../pane/TerminalPane";
import { IconButton } from "./Icon";
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

export type PaneActions = {
  onFocus: () => void;
  onSplit: (dir: "row" | "col") => void;
  onZoom: () => void;
  onClose: () => void;
};

export function PaneView({
  pane,
  focused,
  zoomed,
  fontSize,
  actions,
}: {
  pane: Pane;
  focused: boolean;
  zoomed: boolean;
  fontSize: number;
  actions: PaneActions;
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
      <TerminalPane id={pane.ptyId} focused={focused} fontSize={fontSize} />
    </section>
  );
}
