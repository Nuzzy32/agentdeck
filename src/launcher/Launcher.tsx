import { type FormEvent, type KeyboardEvent, useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Icon, type IconName } from "../layout/Icon";
import { type AgentInfo, expandHome, type LaunchError, tildePath } from "../layout/model";
import { InstallHint } from "../layout/Pane";
import { BypassDialog } from "../layout/Shell";
import { type Counts, expand, MANY_MAX } from "../prompts/prompts";

/** Launcher choice id for a plain shell; the others are catalogue ids. */
export const SHELL = "shell";

const ICON: Record<string, IconName> = { [SHELL]: "terminal", "claude-code": "sparkles", codex: "bot" };

export function agentIcon(id: string): IconName {
  return ICON[id] ?? "terminal";
}

/** Launcher shortcuts (D-034): common Claude/Codex mixes, shown when those agents are installed. */
export const SHORTCUTS: readonly { label: string; counts: Counts }[] = [
  { label: "1 Claude", counts: { "claude-code": 1 } },
  { label: "Claude + Codex", counts: { "claude-code": 1, codex: 1 } },
  { label: "2 + 2", counts: { "claude-code": 2, codex: 2 } },
  { label: "3 + 3", counts: { "claude-code": 3, codex: 3 } },
];

/** `counts` with `id` changed by `delta`, kept within 0..MANY_MAX in total. */
export function bump(counts: Counts, id: string, delta: number): Counts {
  const total = Object.values(counts).reduce((a, b) => a + b, 0);
  const now = counts[id] ?? 0;
  const next = Math.max(0, Math.min(now + delta, now + MANY_MAX - total));
  return { ...counts, [id]: next };
}

/** What the workspace launcher opens (D-034). */
export type Many = { task: string; worktree: boolean; bypass: boolean };

/**
 * Launcher (UI-UX §4.1): program, folder, optional first task. Native modal dialog, so Esc closes and focus stays
 * inside; arrow keys move between the radio buttons natively. Enter launches (⌘Enter in the task field). A failed
 * launch keeps it open with the error, so a missing CLI never leaves a broken pane behind (PRD alur A).
 */
export function Launcher({
  title,
  agents,
  initial,
  folder: initialFolder,
  recents,
  home,
  onLaunch,
  onLaunchMany,
  onClose,
}: {
  title: string;
  /** null while `agents_list` is still running. */
  agents: AgentInfo[] | null;
  initial: string;
  /** Absolute paths. */
  folder: string;
  recents: string[];
  home: string | null;
  /** Resolves to an error to show, or null once the pane is open. */
  onLaunch: (choice: string, cwd: string, task: string) => Promise<LaunchError | null>;
  /** Workspace launcher only (D-034): several panes at once, with the worktree and bypass switches. */
  onLaunchMany?: (programs: string[], cwd: string, opts: Many) => Promise<LaunchError | null>;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const [choice, setChoice] = useState(initial);
  const [folder, setFolder] = useState(tildePath(initialFolder, home));
  const [task, setTask] = useState("");
  const [error, setError] = useState<LaunchError | null>(null);
  const [busy, setBusy] = useState(false);
  const many = onLaunchMany !== undefined;
  const [counts, setCounts] = useState<Counts>({ [initial]: 1 });
  const [worktree, setWorktree] = useState(false);
  // Off every time the launcher opens; never remembered (D-034).
  const [bypass, setBypass] = useState(false);
  const [askBypass, setAskBypass] = useState(false);

  useEffect(() => {
    const dlg = ref.current;
    if (!dlg) return;
    dlg.showModal();
    dlg.querySelector<HTMLElement>("input[name=program]:checked, .launcher-count button")?.focus();
    return () => dlg.close();
  }, []);

  const submit = async (e?: FormEvent) => {
    e?.preventDefault();
    if (busy) return;
    setBusy(true);
    setError(null);
    const cwd = expandHome(folder.trim(), home);
    const err = onLaunchMany
      ? await onLaunchMany(expand(counts, programs.map((p) => p.id)), cwd, {
          task: oneAgent ? task : "",
          worktree: worktree && agentCount > 0,
          bypass: bypass && agentCount > 0,
        })
      : await onLaunch(choice, cwd, choice === SHELL ? "" : task);
    setBusy(false);
    setError(err);
  };

  const onKey = (e: KeyboardEvent<HTMLFormElement>) => {
    if (e.key !== "Enter" || e.shiftKey || e.altKey || e.ctrlKey) return;
    const inTask = e.target instanceof HTMLTextAreaElement;
    if (inTask ? e.metaKey : !(e.target instanceof HTMLButtonElement)) {
      e.preventDefault();
      void submit();
    }
  };

  const pick = async () => {
    const dir = await open({ directory: true, defaultPath: expandHome(folder, home) }).catch(() => null);
    if (typeof dir === "string") setFolder(tildePath(dir, home));
  };

  const programs: AgentInfo[] = [
    { id: SHELL, name: "Shell", installed: true, version: null, install: "" },
    ...(agents ?? []),
  ];
  const total = Object.values(counts).reduce((a, b) => a + b, 0);
  const agentCount = total - (counts[SHELL] ?? 0);
  const oneAgent = many ? agentCount === 1 && total === 1 : choice !== SHELL;
  const installed = (id: string) => programs.some((p) => p.id === id && p.installed);
  const shortcuts = SHORTCUTS.filter((s) => Object.keys(s.counts).every(installed));

  return (
    <dialog
      ref={ref}
      className="dialog launcher"
      aria-labelledby="launcher-title"
      onCancel={(e) => {
        e.preventDefault();
        onClose();
      }}
    >
      <form onSubmit={(e) => void submit(e)} onKeyDown={onKey}>
        <p id="launcher-title" className="dialog-title">
          {title}
        </p>
        {many ? (
          <fieldset className="launcher-programs">
            <legend className="launcher-field">Terminal sekaligus</legend>
            {shortcuts.length > 0 ? (
              <div className="launcher-chips" role="group" aria-label="Pintasan jumlah terminal">
                {shortcuts.map((s) => (
                  <button key={s.label} type="button" className="btn btn-small" onClick={() => setCounts(s.counts)}>
                    {s.label}
                  </button>
                ))}
              </div>
            ) : null}
            {programs.map((p) => (
              <div key={p.id} className={`launcher-program${p.installed ? "" : " launcher-program-off"}`}>
                <div className="launcher-program-label launcher-count">
                  <Icon name={agentIcon(p.id)} />
                  <span className="launcher-program-name">{p.name}</span>
                  <span className="faint launcher-program-meta">
                    {p.installed ? (p.version ?? "") : "Belum terpasang"}
                  </span>
                  <button
                    type="button"
                    className="btn btn-small"
                    aria-label={`Kurangi ${p.name}`}
                    disabled={!p.installed || (counts[p.id] ?? 0) === 0}
                    onClick={() => setCounts((c) => bump(c, p.id, -1))}
                  >
                    −
                  </button>
                  <span className="mono launcher-count-n" aria-live="polite" aria-label={`${p.name}: ${counts[p.id] ?? 0}`}>
                    {counts[p.id] ?? 0}
                  </span>
                  <button
                    type="button"
                    className="btn btn-small"
                    aria-label={`Tambah ${p.name}`}
                    disabled={!p.installed || total >= MANY_MAX}
                    onClick={() => setCounts((c) => bump(c, p.id, 1))}
                  >
                    +
                  </button>
                </div>
                {p.installed ? null : <InstallHint command={p.install} />}
              </div>
            ))}
            {agents === null ? <p className="faint launcher-note">Memeriksa agen yang terpasang…</p> : null}
            <p className="faint launcher-note">
              {total} terminal, paling banyak {MANY_MAX}.
            </p>
          </fieldset>
        ) : (
        <fieldset className="launcher-programs">
            <legend className="sr-only">Program</legend>
            {programs.map((p) => (
              <div key={p.id} className={`launcher-program${p.installed ? "" : " launcher-program-off"}`}>
                <label className="launcher-program-label">
                  <input
                    type="radio"
                    name="program"
                    value={p.id}
                    checked={choice === p.id}
                    disabled={!p.installed}
                    onChange={() => setChoice(p.id)}
                  />
                  <Icon name={agentIcon(p.id)} />
                  <span className="launcher-program-name">{p.name}</span>
                  <span className="faint launcher-program-meta">
                    {p.installed ? (p.version ?? "") : "Belum terpasang"}
                  </span>
                </label>
                {p.installed ? null : <InstallHint command={p.install} />}
              </div>
            ))}
            {agents === null ? <p className="faint launcher-note">Memeriksa agen yang terpasang…</p> : null}
          </fieldset>
        )}

        <label className="launcher-field" htmlFor="launcher-folder">
          Folder
        </label>
        <div className="launcher-row">
          <input
            id="launcher-folder"
            className="mono input"
            value={folder}
            spellCheck={false}
            autoComplete="off"
            onChange={(e) => setFolder(e.target.value)}
          />
          <button type="button" className="btn" onClick={() => void pick()}>
            <Icon name="folder" size={12} />
            Pilih…
          </button>
        </div>
        {recents.length > 0 ? (
          <ul className="launcher-recents" aria-label="Folder terakhir">
            {recents.map((f) => (
              <li key={f}>
                <button type="button" className="link-btn mono" onClick={() => setFolder(tildePath(f, home))}>
                  {tildePath(f, home)}
                </button>
              </li>
            ))}
          </ul>
        ) : null}

        {many ? (
          <div className="launcher-toggles">
            <label className="launcher-toggle">
              <input
                type="checkbox"
                checked={worktree}
                disabled={agentCount === 0}
                onChange={(e) => setWorktree(e.target.checked)}
              />
              <span>
                Worktree baru per terminal agen <span className="faint">(folder harus repo git)</span>
              </span>
            </label>
            <label className="launcher-toggle launcher-toggle-danger">
              <input
                type="checkbox"
                checked={bypass}
                disabled={agentCount === 0}
                onChange={(e) => (e.target.checked ? setAskBypass(true) : setBypass(false))}
              />
              <span>Lewati izin di semua terminal agen</span>
            </label>
          </div>
        ) : null}

        {!oneAgent ? null : (
          <>
            <label className="launcher-field" htmlFor="launcher-task">
              Tugas awal <span className="faint">(opsional, ⌘Enter menjalankan)</span>
            </label>
            <textarea
              id="launcher-task"
              className="mono input"
              rows={4}
              value={task}
              spellCheck={false}
              onChange={(e) => setTask(e.target.value)}
            />
          </>
        )}

        {error ? (
          <div className="launcher-error" role="alert">
            <span>{error.message}</span>
            {error.install ? <InstallHint command={error.install} /> : null}
          </div>
        ) : null}

        <div className="dialog-actions">
          <button type="button" className="btn" onClick={onClose}>
            Batal
          </button>
          <button type="submit" className="btn btn-primary" disabled={busy || (many && total === 0)}>
            Jalankan
          </button>
        </div>
      </form>
      {askBypass ? (
        <BypassDialog
          onConfirm={() => {
            setBypass(true);
            setAskBypass(false);
          }}
          onCancel={() => setAskBypass(false)}
        />
      ) : null}
    </dialog>
  );
}
