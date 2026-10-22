import { type FormEvent, type ReactNode, type Ref, useEffect, useId, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import {
  type AgentDetail,
  type AgentsView,
  FONT_SIZE,
  LIMITS,
  LINE_HEIGHT,
  loadAgents,
  parseInRange,
  type Patch,
  type Range,
  SCROLLBACK,
  type Settings,
  step,
  type Theme,
} from "./settings";
import "./settings.css";

export type Section = "agents" | "collab" | "terminal" | "about";

const SECTIONS: { id: Section; label: string }[] = [
  { id: "agents", label: "Agen" },
  { id: "collab", label: "Kolaborasi" },
  { id: "terminal", label: "Terminal" },
  { id: "about", label: "Tentang" },
];

const THEMES: { id: Theme; label: string }[] = [
  { id: "system", label: "Sistem" },
  { id: "dark", label: "Gelap" },
  { id: "light", label: "Terang" },
];

/**
 * Settings (UI-UX §4.6): native modal dialog, so Esc closes and focus stays inside; on close focus goes back to where
 * it was (the pane's terminal). Every change is saved at once; there is no Save button.
 */
export function SettingsModal({
  initial,
  settings,
  onChange,
  onClose,
}: {
  initial: Section;
  settings: Settings;
  onChange: (patch: Patch) => void;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const [section, setSection] = useState(initial);
  const [agents, setAgents] = useState<AgentsView | null>(null);
  const [agentsError, setAgentsError] = useState<string | null>(null);
  const [version, setVersion] = useState<string | null>(null);
  const [aboutError, setAboutError] = useState<string | null>(null);

  useEffect(() => {
    const dlg = ref.current;
    if (!dlg) return;
    const before = document.activeElement;
    dlg.showModal();
    dlg.querySelector<HTMLElement>("[aria-current=page]")?.focus();
    return () => {
      dlg.close();
      if (before instanceof HTMLElement) before.focus();
    };
  }, []);

  const refreshAgents = () =>
    loadAgents().then(
      (v) => {
        setAgents(v);
        setAgentsError(null);
      },
      (e: unknown) => setAgentsError(String(e)),
    );

  useEffect(() => {
    void refreshAgents();
    getVersion().then(setVersion, () => setVersion(null));
  }, []);

  const setPath = (id: string, path: string | null) =>
    invoke("agent_set_path", { id, path }).then(refreshAgents, (e: unknown) => setAgentsError(String(e)));

  const openLogs = () => invoke("logs_open").then(() => setAboutError(null), (e: unknown) => setAboutError(String(e)));

  return (
    <SettingsDialog dialogRef={ref} section={section} onSection={setSection} onClose={onClose}>
      {section === "agents" ? (
        <AgentsSection view={agents} error={agentsError} onSetPath={(id, p) => void setPath(id, p)} />
      ) : section === "collab" ? (
        <CollabSection settings={settings} onChange={onChange} />
      ) : section === "terminal" ? (
        <TerminalSection settings={settings} onChange={onChange} />
      ) : (
        <AboutSection version={version} error={aboutError} onOpenLogs={() => void openLogs()} />
      )}
    </SettingsDialog>
  );
}

/** The dialog frame and left navigation, without state, so tests can call it. */
export function SettingsDialog({
  dialogRef,
  section,
  onSection,
  onClose,
  children,
}: {
  dialogRef?: Ref<HTMLDialogElement>;
  section: Section;
  onSection: (s: Section) => void;
  onClose: () => void;
  children: ReactNode;
}) {
  const current = SECTIONS.find((s) => s.id === section)?.label ?? "";
  return (
    <dialog
      ref={dialogRef}
      className="dialog settings"
      aria-label="Settings"
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
    >
      <nav className="settings-nav" aria-label="Bagian Settings">
        {SECTIONS.map((s) => (
          <button
            key={s.id}
            type="button"
            className={`row${s.id === section ? " row-active" : ""}`}
            aria-current={s.id === section ? "page" : undefined}
            onClick={() => onSection(s.id)}
          >
            {s.label}
          </button>
        ))}
      </nav>
      <section className="settings-body" aria-label={current}>
        <div className="settings-head">
          <h2 className="settings-title">{current}</h2>
          <button type="button" className="btn btn-small" onClick={onClose} title="Tutup (Esc)">
            Tutup
          </button>
        </div>
        {children}
      </section>
    </dialog>
  );
}

export function AgentsSection({
  view,
  error,
  onSetPath,
}: {
  view: AgentsView | null;
  error: string | null;
  onSetPath: (id: string, path: string | null) => void;
}) {
  if (!view) {
    return <p className="settings-note">{error ? `Daftar agen tidak terbaca: ${error}` : "Memeriksa agen…"}</p>;
  }
  return (
    <div className="settings-list">
      {error ? (
        <p className="settings-note danger" role="alert">
          {error}
        </p>
      ) : null}
      {view.agents.map((a) => (
        <AgentRow key={a.id} agent={a} detailed={view.detailed} onSetPath={onSetPath} />
      ))}
      {view.detailed ? null : (
        <p className="settings-note">Path, metode login, dan path manual belum tersedia di versi core ini.</p>
      )}
    </div>
  );
}

export function AgentRow({
  agent: a,
  detailed,
  onSetPath,
}: {
  agent: AgentDetail;
  detailed: boolean;
  onSetPath: (id: string, path: string | null) => void;
}) {
  const submit = (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    const value = new FormData(e.currentTarget).get("path");
    const path = typeof value === "string" ? value.trim() : "";
    onSetPath(a.id, path === "" ? null : path);
  };
  return (
    <div className="settings-agent">
      <div className="settings-agent-head">
        <span className="settings-agent-name">{a.name}</span>
        <span className={a.installed ? "ok" : "faint"}>{a.installed ? "Terdeteksi" : "Tidak terdeteksi"}</span>
        {a.version ? <span className="mono faint">{a.version}</span> : null}
      </div>
      {a.path ? <div className="mono settings-path">{a.path}</div> : null}
      {!a.installed && a.install ? (
        <div className="faint">
          Pasang: <span className="mono">{a.install}</span>
        </div>
      ) : null}
      {a.login ? (
        <div className="muted">
          Login: {a.login.method}
          {a.login.detail ? ` · ${a.login.detail}` : ""}
        </div>
      ) : null}
      {a.api_key_env.length > 0 ? (
        <p className="settings-warn warn">
          {a.api_key_env.join(", ")} ada di lingkunganmu. agentdeck tidak meneruskan {a.api_key_env.join(", ")} ke
          agen; agen memakai login CLI-nya sendiri.
        </p>
      ) : null}
      {detailed ? (
        <form className="settings-inline" onSubmit={submit}>
          <label className="sr-only" htmlFor={`path-${a.id}`}>
            Path manual {a.name}
          </label>
          <input
            key={a.path_override ?? ""}
            id={`path-${a.id}`}
            name="path"
            className="input mono"
            defaultValue={a.path_override ?? ""}
            placeholder={a.path ?? `Path manual ${a.name}`}
            spellCheck={false}
          />
          <button type="submit" className="btn btn-small">
            Simpan path
          </button>
          {a.path_override ? (
            <button type="button" className="btn btn-small" onClick={() => onSetPath(a.id, null)}>
              Deteksi otomatis
            </button>
          ) : null}
        </form>
      ) : null}
    </div>
  );
}

export function CollabSection({ settings, onChange }: { settings: Settings; onChange: (p: Patch) => void }) {
  return (
    <div className="settings-list">
      <Switch
        label="Agen boleh membuka pane"
        hint="Agen bisa meminta membuka agen lain; setiap permintaan tetap butuh persetujuanmu."
        checked={settings.spawn_enabled}
        onChange={(on) => onChange({ spawn_enabled: on })}
      />
      <Switch
        label="Saling kirim untuk workspace baru"
        hint="Bawaan sakelar “Saling kirim” saat workspace dibuat. Workspace yang sudah ada tidak berubah."
        checked={settings.peer_default}
        onChange={(on) => onChange({ peer_default: on })}
      />
      <h3 className="settings-subtitle">Batas</h3>
      {LIMITS.map((l) => (
        <Stepper
          key={l.key}
          label={l.label}
          value={settings.limits[l.key]}
          range={l.range}
          onChange={(v) => onChange({ limits: { [l.key]: v } })}
        />
      ))}
    </div>
  );
}

export function TerminalSection({ settings, onChange }: { settings: Settings; onChange: (p: Patch) => void }) {
  const t = settings.terminal;
  const set = (p: Partial<Settings["terminal"]>) => onChange({ terminal: p });
  return (
    <div className="settings-list">
      <FontField value={t.font_family} onChange={(font_family) => set({ font_family })} />
      <Stepper
        label="Ukuran font (⌘= ⌘- ⌘0)"
        value={t.font_size}
        range={FONT_SIZE}
        onChange={(font_size) => set({ font_size })}
      />
      <Stepper
        label="Tinggi baris"
        value={t.line_height}
        range={LINE_HEIGHT}
        onChange={(line_height) => set({ line_height })}
      />
      <Stepper
        label="Scrollback (baris)"
        value={t.scrollback}
        range={SCROLLBACK}
        onChange={(scrollback) => set({ scrollback })}
      />
      <fieldset className="settings-field settings-themes">
        <legend>Tema terminal</legend>
        {THEMES.map((th) => (
          <label key={th.id} className="settings-radio">
            <input
              type="radio"
              name="term-theme"
              value={th.id}
              checked={t.theme === th.id}
              onChange={() => set({ theme: th.id })}
            />
            {th.label}
          </label>
        ))}
      </fieldset>
      <Switch
        label="Mode pembaca layar"
        hint="Isi terminal dibacakan oleh VoiceOver; sedikit lebih lambat."
        checked={t.screen_reader}
        onChange={(screen_reader) => set({ screen_reader })}
      />
    </div>
  );
}

export function AboutSection({
  version,
  error,
  onOpenLogs,
}: {
  version: string | null;
  error: string | null;
  onOpenLogs: () => void;
}) {
  return (
    <div className="settings-list">
      <p className="settings-note">
        agentdeck <span className="mono">{version ?? "—"}</span>
      </p>
      <p className="settings-note muted">Dokumentasi ada di folder docs/ repositori agentdeck.</p>
      <div>
        <button type="button" className="btn" onClick={onOpenLogs}>
          Buka folder log
        </button>
      </div>
      {error ? (
        <p className="settings-note danger" role="alert">
          {error}
        </p>
      ) : null}
    </div>
  );
}

function Switch({
  label,
  hint,
  checked,
  onChange,
}: {
  label: string;
  hint: string;
  checked: boolean;
  onChange: (on: boolean) => void;
}) {
  const hintId = useId();
  return (
    <label className="settings-switch">
      <input
        type="checkbox"
        role="switch"
        checked={checked}
        aria-describedby={hintId}
        onChange={(e) => onChange(e.target.checked)}
      />
      <span>
        <span className="settings-label">{label}</span>
        <span id={hintId} className="settings-hint">
          {hint}
        </span>
      </span>
    </label>
  );
}

/** Number with − / + buttons. Typed text outside the range is refused in place and never sent. */
export function Stepper({
  label,
  value,
  range,
  onChange,
}: {
  label: string;
  value: number;
  range: Range;
  onChange: (v: number) => void;
}) {
  const id = useId();
  const [draft, setDraft] = useState<string | null>(null);
  const parsed = draft === null ? null : parseInRange(draft, range);
  const error = parsed && "error" in parsed ? parsed.error : null;
  const commit = () => {
    if (!parsed || !("value" in parsed)) return;
    if (parsed.value !== value) onChange(parsed.value);
    setDraft(null);
  };
  const press = (dir: 1 | -1) => {
    setDraft(null);
    const next = step(value, range, dir);
    if (next !== value) onChange(next);
  };
  return (
    <div className="settings-field settings-stepper-row">
      <label htmlFor={id} className="settings-label">
        {label}
      </label>
      <div className="settings-stepper">
        <button
          type="button"
          className="btn btn-small"
          aria-label={`Kurangi ${label}`}
          disabled={value <= range.min}
          onClick={() => press(-1)}
        >
          −
        </button>
        <input
          id={id}
          className="input mono"
          inputMode="decimal"
          value={draft ?? String(value)}
          aria-invalid={error ? true : undefined}
          aria-describedby={`${id}-hint`}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === "Enter") commit();
            else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
              e.preventDefault();
              press(e.key === "ArrowUp" ? 1 : -1);
            }
          }}
        />
        <button
          type="button"
          className="btn btn-small"
          aria-label={`Tambah ${label}`}
          disabled={value >= range.max}
          onClick={() => press(1)}
        >
          +
        </button>
      </div>
      <span
        id={`${id}-hint`}
        className={error ? "settings-hint danger" : "settings-hint"}
        role={error ? "alert" : undefined}
      >
        {error ?? `${range.min}–${range.max}`}
      </span>
    </div>
  );
}

function FontField({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const id = useId();
  const [draft, setDraft] = useState<string | null>(null);
  const empty = draft !== null && draft.trim() === "";
  const commit = () => {
    if (draft === null || empty) return;
    if (draft.trim() !== value) onChange(draft.trim());
    setDraft(null);
  };
  return (
    <div className="settings-field">
      <label htmlFor={id} className="settings-label">
        Font
      </label>
      <input
        id={id}
        className="input mono"
        value={draft ?? value}
        spellCheck={false}
        aria-invalid={empty ? true : undefined}
        aria-describedby={`${id}-hint`}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => {
          if (e.key === "Enter") commit();
        }}
      />
      <span
        id={`${id}-hint`}
        className={empty ? "settings-hint danger" : "settings-hint"}
        role={empty ? "alert" : undefined}
      >
        {empty ? "Font tidak boleh kosong." : "Nama font CSS, dipisah koma. Font sistem saja."}
      </span>
    </div>
  );
}
