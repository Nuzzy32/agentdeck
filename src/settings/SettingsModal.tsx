import {
  type FormEvent,
  type KeyboardEvent,
  type ReactNode,
  type Ref,
  useEffect,
  useId,
  useRef,
  useState,
} from "react";
import { getVersion } from "@tauri-apps/api/app";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import appIcon from "../../src-tauri/icons/128x128@2x.png";
import { AgentMark, Icon, type IconName } from "../layout/Icon";
import { InstallHint } from "../layout/Pane";
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

export type Section = "agents" | "collab" | "terminal" | "shortcuts" | "about";

type SectionInfo = { id: Section; label: string; icon: IconName; description: string };

const GROUPS: { label: string; sections: SectionInfo[] }[] = [
  {
    label: "Agen",
    sections: [
      {
        id: "agents",
        label: "Agen",
        icon: "bot",
        description: "CLI agen yang ditemukan di mesin ini. Setiap agen memakai login CLI-nya sendiri.",
      },
      {
        id: "collab",
        label: "Kolaborasi",
        icon: "users",
        description: "Seberapa jauh agen boleh membuka pane dan saling mengirim pesan.",
      },
    ],
  },
  {
    label: "Tampilan",
    sections: [
      {
        id: "terminal",
        label: "Terminal",
        icon: "terminal",
        description: "Tampilan dan perilaku semua pane terminal.",
      },
      {
        id: "shortcuts",
        label: "Pintasan",
        icon: "keyboard",
        description: "Semua pintasan memakai ⌘, jadi tidak bentrok dengan program yang berjalan di terminal.",
      },
    ],
  },
  {
    label: "Lainnya",
    sections: [
      {
        id: "about",
        label: "Tentang",
        icon: "info",
        description: "Versi, lisensi, log, dan cara memasang atau memperbarui agentdeck.",
      },
    ],
  },
];

const SECTIONS = GROUPS.flatMap((g) => g.sections);

const THEMES: { id: Theme; label: string }[] = [
  { id: "system", label: "Ikuti sistem" },
  { id: "dark", label: "Gelap" },
  { id: "light", label: "Terang" },
];

/** UI-UX §7, as shown in Settings › Pintasan. */
export const SHORTCUT_LIST: { action: string; keys: string[] }[] = [
  { action: "Workspace baru", keys: ["⌘T"] },
  { action: "Split kanan / bawah", keys: ["⌘D", "⌘⇧D"] },
  { action: "Tutup pane", keys: ["⌘W"] },
  { action: "Pindah fokus pane", keys: ["⌘⌥ + panah"] },
  { action: "Pindah workspace", keys: ["⌘1–⌘9", "⌘⇧[", "⌘⇧]"] },
  { action: "Perbesar / kembalikan pane", keys: ["⌘⇧Enter"] },
  { action: "Panel Pesan", keys: ["⌘⇧M"] },
  { action: "Ciutkan / tampilkan sidebar", keys: ["⌘B"] },
  { action: "Cari di pane", keys: ["⌘F"] },
  { action: "Ukuran font", keys: ["⌘=", "⌘-", "⌘0"] },
  { action: "Palet prompt dan preset", keys: ["⌘P"] },
  { action: "Settings", keys: ["⌘,"] },
];

/** One command that installs or updates agentdeck from source (install.sh at the repository root). */
export const INSTALL_COMMAND = "curl -fsSL https://raw.githubusercontent.com/Nuzzy32/agentdeck/main/install.sh | sh";

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

  const openLogs = () =>
    invoke("logs_open").then(
      () => setAboutError(null),
      (e: unknown) => setAboutError(String(e)),
    );

  return (
    <SettingsDialog dialogRef={ref} section={section} version={version} onSection={setSection} onClose={onClose}>
      {section === "agents" ? (
        <AgentsSection
          view={agents}
          error={agentsError}
          onSetPath={(id, p) => void setPath(id, p)}
          onRetry={() => {
            setAgentsError(null);
            void refreshAgents();
          }}
        />
      ) : section === "collab" ? (
        <CollabSection settings={settings} onChange={onChange} />
      ) : section === "terminal" ? (
        <TerminalSection settings={settings} onChange={onChange} />
      ) : section === "shortcuts" ? (
        <ShortcutsSection />
      ) : (
        <AboutSection version={version} error={aboutError} onOpenLogs={() => void openLogs()} />
      )}
    </SettingsDialog>
  );
}

/** Arrow keys move between sections in the navigation, like a native sidebar list. */
function navKey(e: KeyboardEvent<HTMLElement>, onSection: (s: Section) => void) {
  if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
  e.preventDefault();
  const buttons = [...e.currentTarget.querySelectorAll<HTMLButtonElement>("button[data-section]")];
  const at = buttons.findIndex((b) => b === document.activeElement);
  const next = buttons[(at + (e.key === "ArrowDown" ? 1 : buttons.length - 1)) % buttons.length];
  if (!next) return;
  next.focus();
  onSection(next.dataset.section as Section);
}

/** The dialog frame and left navigation, without state, so tests can call it. */
export function SettingsDialog({
  dialogRef,
  section,
  version = null,
  onSection,
  onClose,
  children,
}: {
  dialogRef?: Ref<HTMLDialogElement>;
  section: Section;
  version?: string | null;
  onSection: (s: Section) => void;
  onClose: () => void;
  children: ReactNode;
}) {
  const current = SECTIONS.find((s) => s.id === section) ?? SECTIONS[0];
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
      <aside className="settings-side">
        <p className="settings-side-title">Settings</p>
        <nav className="settings-nav" aria-label="Bagian Settings" onKeyDown={(e) => navKey(e, onSection)}>
          {GROUPS.map((g) => (
            <div key={g.label} className="settings-nav-group">
              <p className="settings-nav-label">{g.label}</p>
              {g.sections.map((s) => (
                <button
                  key={s.id}
                  type="button"
                  data-section={s.id}
                  className={`settings-nav-item${s.id === section ? " is-active" : ""}`}
                  aria-current={s.id === section ? "page" : undefined}
                  onClick={() => onSection(s.id)}
                >
                  <Icon name={s.icon} size={15} />
                  <span>{s.label}</span>
                </button>
              ))}
            </div>
          ))}
        </nav>
        <p className="settings-side-foot">
          <span>agentdeck</span>
          <span className="mono settings-pill">{version ? `v${version}` : "—"}</span>
        </p>
      </aside>
      <section className="settings-main" aria-labelledby="settings-heading">
        <header className="settings-head">
          <div>
            <h2 id="settings-heading" className="settings-title">
              {current?.label}
            </h2>
            <p className="settings-desc">{current?.description}</p>
          </div>
          <button
            type="button"
            className="icon-btn settings-close"
            aria-label="Tutup (Esc)"
            title="Tutup (Esc)"
            onClick={onClose}
          >
            <Icon name="x" size={16} />
          </button>
        </header>
        <div className="settings-content">{children}</div>
      </section>
    </dialog>
  );
}

/** A titled block of rows on a raised card. */
function Group({ title, children }: { title?: string; children: ReactNode }) {
  return (
    <section className="settings-group">
      {title ? <h3 className="settings-group-title">{title}</h3> : null}
      <div className="settings-card">{children}</div>
    </section>
  );
}

export function AgentsSection({
  view,
  error,
  onSetPath,
  onRetry,
}: {
  view: AgentsView | null;
  error: string | null;
  onSetPath: (id: string, path: string | null) => void;
  onRetry?: () => void;
}) {
  if (!view) {
    return error ? (
      <div className="settings-callout settings-callout-danger" role="alert">
        <Icon name="alert" size={16} />
        <div>
          <p className="settings-callout-text">Daftar agen tidak terbaca: {error}</p>
          {onRetry ? (
            <button type="button" className="btn btn-small" onClick={onRetry}>
              <Icon name="refresh" size={12} />
              Coba lagi
            </button>
          ) : null}
        </div>
      </div>
    ) : (
      <div className="settings-agents" aria-busy="true">
        <p className="sr-only">Memeriksa agen…</p>
        <div className="settings-agent settings-skeleton" aria-hidden="true" />
        <div className="settings-agent settings-skeleton" aria-hidden="true" />
        <p className="settings-note" aria-hidden="true">
          Memeriksa agen…
        </p>
      </div>
    );
  }
  return (
    <div className="settings-agents">
      {error ? (
        <div className="settings-callout settings-callout-danger" role="alert">
          <Icon name="alert" size={16} />
          <p className="settings-callout-text">{error}</p>
        </div>
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
  const browse = () =>
    open({ directory: false, multiple: false, defaultPath: a.path ?? undefined }).then(
      (p) => {
        if (typeof p === "string") onSetPath(a.id, p);
      },
      () => undefined,
    );
  return (
    <article className="settings-agent">
      <header className="settings-agent-head">
        <AgentMark id={a.id} size={28} />
        <div className="settings-agent-id">
          <span className="settings-agent-name">{a.name}</span>
          {a.version ? <span className="mono settings-agent-version">{a.version}</span> : null}
        </div>
        <span className={`settings-status${a.installed ? " is-ok" : ""}`}>
          {a.installed ? <Icon name="check" size={12} /> : <Icon name="alert" size={12} />}
          {a.installed ? "Terdeteksi" : "Tidak terdeteksi"}
        </span>
      </header>
      {a.path || a.login ? (
        <dl className="settings-agent-meta">
          {a.path ? (
            <div>
              <dt>Path</dt>
              <dd className="mono settings-path" title={a.path}>
                {a.path}
              </dd>
            </div>
          ) : null}
          {a.login ? (
            <div>
              <dt>Login</dt>
              <dd>
                {a.login.method}
                {a.login.detail ? ` · ${a.login.detail}` : ""}
              </dd>
            </div>
          ) : null}
        </dl>
      ) : null}
      {a.api_key_env.length > 0 ? (
        <div className="settings-callout settings-callout-warn">
          <Icon name="alert" size={14} />
          <p className="settings-callout-text">
            {a.api_key_env.join(", ")} ada di lingkunganmu. agentdeck tidak meneruskan {a.api_key_env.join(", ")} ke
            agen; agen memakai login CLI-nya sendiri.
          </p>
        </div>
      ) : null}
      {!a.installed && a.install ? (
        <div className="settings-command">
          <span className="settings-command-label">Pasang lewat terminal, lalu buka Settings lagi</span>
          <InstallHint command={a.install} />
        </div>
      ) : null}
      {detailed ? (
        <form className="settings-path-form" onSubmit={submit}>
          <label className="settings-command-label" htmlFor={`path-${a.id}`}>
            Path manual {a.name}
          </label>
          <div className="settings-inline">
            <input
              key={a.path_override ?? ""}
              id={`path-${a.id}`}
              name="path"
              className="input mono"
              defaultValue={a.path_override ?? ""}
              placeholder={a.path ?? "Deteksi otomatis lewat PATH"}
              spellCheck={false}
              autoComplete="off"
            />
            <button type="button" className="btn btn-small" onClick={() => void browse()}>
              <Icon name="folder" size={12} />
              Pilih…
            </button>
            <button type="submit" className="btn btn-small">
              Simpan path
            </button>
            {a.path_override ? (
              <button type="button" className="btn btn-small" onClick={() => onSetPath(a.id, null)}>
                Deteksi otomatis
              </button>
            ) : null}
          </div>
        </form>
      ) : null}
    </article>
  );
}

export function CollabSection({ settings, onChange }: { settings: Settings; onChange: (p: Patch) => void }) {
  return (
    <>
      <Group title="Izin">
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
      </Group>
      <Group title="Batas">
        {LIMITS.map((l) => (
          <Stepper
            key={l.key}
            label={l.label}
            value={settings.limits[l.key]}
            range={l.range}
            onChange={(v) => onChange({ limits: { [l.key]: v } })}
          />
        ))}
      </Group>
    </>
  );
}

export function TerminalSection({ settings, onChange }: { settings: Settings; onChange: (p: Patch) => void }) {
  const t = settings.terminal;
  const set = (p: Partial<Settings["terminal"]>) => onChange({ terminal: p });
  return (
    <>
      <Group title="Teks">
        <FontField value={t.font_family} onChange={(font_family) => set({ font_family })} />
        <Stepper
          label="Ukuran font"
          hint="⌘= ⌘- ⌘0 di pane"
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
      </Group>
      <Group title="Tampilan">
        <SelectRow
          label="Tema terminal"
          hint="Warna latar dan teks pane. Warna ANSI tetap bawaan."
          value={t.theme}
          options={THEMES}
          onChange={(theme) => set({ theme })}
        />
        <Stepper
          label="Scrollback (baris)"
          value={t.scrollback}
          range={SCROLLBACK}
          onChange={(scrollback) => set({ scrollback })}
        />
      </Group>
      <Group title="Aksesibilitas">
        <Switch
          label="Mode pembaca layar"
          hint="Isi terminal dibacakan oleh VoiceOver; sedikit lebih lambat."
          checked={t.screen_reader}
          onChange={(screen_reader) => set({ screen_reader })}
        />
      </Group>
    </>
  );
}

export function ShortcutsSection() {
  return (
    <Group>
      {SHORTCUT_LIST.map((s) => (
        <div key={s.action} className="settings-row">
          <span className="settings-label">{s.action}</span>
          <span className="settings-keys">
            {s.keys.map((k) => (
              <kbd key={k} className="settings-kbd">
                {k}
              </kbd>
            ))}
          </span>
        </div>
      ))}
    </Group>
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
    <>
      <div className="settings-about">
        <img src={appIcon} alt="" width={56} height={56} />
        <div>
          <p className="settings-about-name">agentdeck</p>
          <p className="settings-note">
            Versi <span className="mono">{version ?? "—"}</span> · Apache License 2.0
          </p>
        </div>
      </div>
      <Group title="Pasang atau perbarui">
        <div className="settings-row settings-row-block settings-command">
          <span className="settings-command-label">
            Jalankan di Terminal. Skrip membangun agentdeck dari kode sumber dan memasangnya ke Applications; prasyarat
            yang belum ada hanya ditampilkan, tidak dipasang diam-diam.
          </span>
          <InstallHint command={INSTALL_COMMAND} />
        </div>
      </Group>
      <Group title="Diagnostik">
        <div className="settings-row">
          <span className="settings-row-text">
            <span className="settings-label">Folder log</span>
            <span className="settings-hint">Log tidak pernah berisi token atau kredensial agen.</span>
          </span>
          <button type="button" className="btn btn-small" onClick={onOpenLogs}>
            <Icon name="folder" size={12} />
            Buka folder log
          </button>
        </div>
      </Group>
      {error ? (
        <div className="settings-callout settings-callout-danger" role="alert">
          <Icon name="alert" size={16} />
          <p className="settings-callout-text">{error}</p>
        </div>
      ) : null}
      <p className="settings-note">
        agentdeck tidak berafiliasi dengan Anthropic maupun OpenAI. Claude Code dan Codex memakai login dan kuota
        paketmu sendiri. Dokumentasi ada di folder docs/ repositori.
      </p>
    </>
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
    <label className="settings-row settings-row-click">
      <span className="settings-row-text">
        <span className="settings-label">{label}</span>
        <span id={hintId} className="settings-hint">
          {hint}
        </span>
      </span>
      <input
        type="checkbox"
        role="switch"
        className="switch"
        checked={checked}
        aria-describedby={hintId}
        onChange={(e) => onChange(e.target.checked)}
      />
    </label>
  );
}

function SelectRow<T extends string>({
  label,
  hint,
  value,
  options,
  onChange,
}: {
  label: string;
  hint: string;
  value: T;
  options: { id: T; label: string }[];
  onChange: (v: T) => void;
}) {
  const id = useId();
  return (
    <div className="settings-row">
      <span className="settings-row-text">
        <label htmlFor={id} className="settings-label">
          {label}
        </label>
        <span id={`${id}-hint`} className="settings-hint">
          {hint}
        </span>
      </span>
      <span className="select">
        <select
          id={id}
          className="input"
          value={value}
          aria-describedby={`${id}-hint`}
          onChange={(e) => onChange(e.target.value as T)}
        >
          {options.map((o) => (
            <option key={o.id} value={o.id}>
              {o.label}
            </option>
          ))}
        </select>
        <Icon name="chevronDown" size={14} />
      </span>
    </div>
  );
}

/** Number with − / + buttons. Typed text outside the range is refused in place and never sent. */
export function Stepper({
  label,
  hint,
  value,
  range,
  onChange,
}: {
  label: string;
  hint?: string;
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
    <div className="settings-row">
      <span className="settings-row-text">
        <label htmlFor={id} className="settings-label">
          {label}
        </label>
        <span
          id={`${id}-hint`}
          className={error ? "settings-hint danger" : "settings-hint"}
          role={error ? "alert" : undefined}
        >
          {error ?? (hint ? `${hint} · ${range.min}–${range.max}` : `${range.min}–${range.max}`)}
        </span>
      </span>
      <div className={`stepper${error ? " is-invalid" : ""}`}>
        <button type="button" aria-label={`Kurangi ${label}`} disabled={value <= range.min} onClick={() => press(-1)}>
          −
        </button>
        <input
          id={id}
          className="mono"
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
        <button type="button" aria-label={`Tambah ${label}`} disabled={value >= range.max} onClick={() => press(1)}>
          +
        </button>
      </div>
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
    <div className="settings-row">
      <span className="settings-row-text">
        <label htmlFor={id} className="settings-label">
          Font
        </label>
        <span
          id={`${id}-hint`}
          className={empty ? "settings-hint danger" : "settings-hint"}
          role={empty ? "alert" : undefined}
        >
          {empty ? "Font tidak boleh kosong." : "Nama font CSS, dipisah koma. Font sistem saja."}
        </span>
      </span>
      <input
        id={id}
        className="input mono settings-font"
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
    </div>
  );
}
