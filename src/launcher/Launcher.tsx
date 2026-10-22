import { type FormEvent, type KeyboardEvent, useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Icon, type IconName } from "../layout/Icon";
import { type AgentInfo, expandHome, type LaunchError, tildePath } from "../layout/model";
import { InstallHint } from "../layout/Pane";

/** Launcher choice id for a plain shell; the others are catalogue ids. */
export const SHELL = "shell";

const ICON: Record<string, IconName> = { [SHELL]: "terminal", "claude-code": "sparkles", codex: "bot" };

export function agentIcon(id: string): IconName {
  return ICON[id] ?? "terminal";
}

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
  onClose: () => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const [choice, setChoice] = useState(initial);
  const [folder, setFolder] = useState(tildePath(initialFolder, home));
  const [task, setTask] = useState("");
  const [error, setError] = useState<LaunchError | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    const dlg = ref.current;
    if (!dlg) return;
    dlg.showModal();
    dlg.querySelector<HTMLInputElement>("input[name=program]:checked")?.focus();
    return () => dlg.close();
  }, []);

  const submit = async (e?: FormEvent) => {
    e?.preventDefault();
    if (busy) return;
    setBusy(true);
    setError(null);
    const err = await onLaunch(choice, expandHome(folder.trim(), home), choice === SHELL ? "" : task);
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

        {choice === SHELL ? null : (
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
          <button type="submit" className="btn btn-primary" disabled={busy}>
            Jalankan
          </button>
        </div>
      </form>
    </dialog>
  );
}
