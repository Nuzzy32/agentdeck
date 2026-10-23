import { type FormEvent, type KeyboardEvent, useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Icon } from "../layout/Icon";
import { expandHome, type LaunchError, tildePath } from "../layout/model";
import { InstallHint } from "../layout/Pane";
import type { Prompt } from "../settings/settings";
import { type Preset, savePrompt, search } from "./prompts";
import "./prompts.css";

type Mode =
  | { kind: "list" }
  | { kind: "edit"; at: number | null; name: string; text: string; error: string | null }
  | { kind: "preset"; preset: Preset; folder: string; error: LaunchError | null; busy: boolean };

/**
 * Prompt palette (⌘P, D-032): a saved prompt is pasted into the focused pane without Enter; a session preset opens a
 * new workspace. Prompts are added, changed and deleted here too. Native modal dialog: Esc goes back, then closes.
 */
export function Palette({
  prompts,
  target,
  folder: initialFolder,
  recents,
  home,
  onPaste,
  onSave,
  onPreset,
  onClose,
}: {
  prompts: Prompt[];
  /** Label of the focused running pane a prompt goes to, or null when there is none. */
  target: string | null;
  /** Absolute path the preset's folder field starts with. */
  folder: string;
  recents: string[];
  home: string | null;
  onPaste: (text: string) => void;
  /** The whole list after an add, change or delete; the core validates and stores it. */
  onSave: (prompts: Prompt[]) => void;
  /** Resolves to an error to show, or null once the workspace is open. */
  onPreset: (preset: Preset, cwd: string) => Promise<LaunchError | null>;
  onClose: () => void;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const [query, setQuery] = useState("");
  const [mode, setMode] = useState<Mode>({ kind: "list" });
  const items = search(query, prompts);

  useEffect(() => {
    const dlg = ref.current;
    if (!dlg) return;
    dlg.showModal();
    return () => dlg.close();
  }, []);

  // Each mode starts in its first field.
  useEffect(() => {
    ref.current?.querySelector<HTMLElement>("[data-autofocus]")?.focus();
  }, [mode.kind]);

  const choosePreset = (preset: Preset) =>
    setMode({ kind: "preset", preset, folder: tildePath(initialFolder, home), error: null, busy: false });

  const activate = (i: number) => {
    const item = items[i];
    if (!item) return;
    if (item.kind === "preset") choosePreset(item.preset);
    else if (target) onPaste(item.prompt.text);
  };

  /** Arrow keys walk the search field and the rows; Enter in the field takes the first row. */
  const onListKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const rows = [...(ref.current?.querySelectorAll<HTMLElement>("[data-row]:not(:disabled)") ?? [])];
    const field = ref.current?.querySelector<HTMLElement>("[data-autofocus]");
    const at = rows.indexOf(document.activeElement as HTMLElement);
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const next = at + (e.key === "ArrowDown" ? 1 : -1);
      (next < 0 ? field : rows[Math.min(next, rows.length - 1)])?.focus();
    } else if (e.key === "Enter" && e.target === field) {
      e.preventDefault();
      activate(0);
    }
  };

  const save = (e?: Pick<FormEvent, "preventDefault">) => {
    e?.preventDefault();
    if (mode.kind !== "edit") return;
    const next = savePrompt(prompts, mode.at, { name: mode.name, text: mode.text });
    if ("error" in next) return setMode({ ...mode, error: next.error });
    onSave(next);
    setMode({ kind: "list" });
  };

  const launch = async (e?: FormEvent) => {
    e?.preventDefault();
    if (mode.kind !== "preset" || mode.busy) return;
    setMode({ ...mode, busy: true, error: null });
    const error = await onPreset(mode.preset, expandHome(mode.folder.trim(), home));
    setMode((m) => (m.kind === "preset" ? { ...m, busy: false, error } : m));
  };

  const pick = async () => {
    if (mode.kind !== "preset") return;
    const dir = await open({ directory: true, defaultPath: expandHome(mode.folder, home) }).catch(() => null);
    if (typeof dir === "string") setMode((m) => (m.kind === "preset" ? { ...m, folder: tildePath(dir, home) } : m));
  };

  return (
    <dialog
      ref={ref}
      className="dialog palette"
      aria-labelledby="palette-title"
      onCancel={(e) => {
        e.preventDefault();
        if (mode.kind === "list") onClose();
        else setMode({ kind: "list" });
      }}
    >
      {mode.kind === "list" ? (
        <div className="palette-body" onKeyDown={onListKey}>
          <p id="palette-title" className="dialog-title">
            Prompt dan preset
          </p>
          <input
            data-autofocus
            className="input"
            placeholder="Cari prompt atau preset"
            aria-label="Cari prompt atau preset"
            value={query}
            spellCheck={false}
            autoComplete="off"
            onChange={(e) => setQuery(e.target.value)}
          />
          <div className="palette-head">
            <span className="palette-label">Prompt</span>
            <span className="faint palette-target">
              {target ? `ditempel ke ${target}, tanpa Enter` : "fokuskan pane yang berjalan untuk menempel"}
            </span>
            <button
              type="button"
              className="link-btn"
              onClick={() => setMode({ kind: "edit", at: null, name: "", text: "", error: null })}
            >
              Prompt baru
            </button>
          </div>
          <ul className="palette-list" aria-label="Prompt">
            {items.flatMap((item) =>
              item.kind === "prompt"
                ? [
                    <li key={item.prompt.name} className="palette-row">
                      <button
                        type="button"
                        data-row
                        className="palette-item"
                        disabled={!target}
                        onClick={() => onPaste(item.prompt.text)}
                      >
                        <span className="palette-name">{item.prompt.name}</span>
                        <span className="faint mono palette-preview">{item.prompt.text.trim().split("\n")[0]}</span>
                      </button>
                      <button
                        type="button"
                        className="btn btn-small"
                        onClick={() => setMode({ kind: "edit", at: item.index, ...item.prompt, error: null })}
                      >
                        Ubah
                      </button>
                      <button
                        type="button"
                        className="btn btn-small"
                        aria-label={`Hapus ${item.prompt.name}`}
                        onClick={() => onSave(prompts.filter((_, i) => i !== item.index))}
                      >
                        Hapus
                      </button>
                    </li>,
                  ]
                : [],
            )}
          </ul>
          {prompts.length === 0 ? (
            <p className="faint palette-note">Belum ada prompt. Simpan teks yang sering kamu kirim ke agen.</p>
          ) : items.every((i) => i.kind !== "prompt") ? (
            <p className="faint palette-note">Tidak ada prompt yang cocok.</p>
          ) : null}

          <div className="palette-head">
            <span className="palette-label">Preset sesi</span>
            <span className="faint palette-target">workspace baru</span>
          </div>
          <ul className="palette-list" aria-label="Preset sesi">
            {items.flatMap((item) =>
              item.kind === "preset"
                ? [
                    <li key={item.preset.id} className="palette-row">
                      <button type="button" data-row className="palette-item" onClick={() => choosePreset(item.preset)}>
                        <span className="palette-name">{item.preset.name}</span>
                        <span className="faint palette-preview">{item.preset.detail}</span>
                      </button>
                    </li>,
                  ]
                : [],
            )}
          </ul>
        </div>
      ) : mode.kind === "edit" ? (
        <form className="palette-body" onSubmit={save} onKeyDown={(e) => e.key === "Enter" && e.metaKey && save(e)}>
          <p id="palette-title" className="dialog-title">
            {mode.at === null ? "Prompt baru" : "Ubah prompt"}
          </p>
          <label className="launcher-field" htmlFor="palette-name">
            Nama
          </label>
          <input
            id="palette-name"
            data-autofocus
            className="input"
            value={mode.name}
            spellCheck={false}
            autoComplete="off"
            onChange={(e) => setMode({ ...mode, name: e.target.value, error: null })}
          />
          <label className="launcher-field" htmlFor="palette-text">
            Teks <span className="faint">(⌘Enter menyimpan)</span>
          </label>
          <textarea
            id="palette-text"
            className="mono input"
            rows={8}
            value={mode.text}
            spellCheck={false}
            onChange={(e) => setMode({ ...mode, text: e.target.value, error: null })}
          />
          {mode.error ? (
            <div className="launcher-error" role="alert">
              {mode.error}
            </div>
          ) : null}
          <div className="dialog-actions">
            <button type="button" className="btn" onClick={() => setMode({ kind: "list" })}>
              Batal
            </button>
            <button type="submit" className="btn btn-primary">
              Simpan
            </button>
          </div>
        </form>
      ) : (
        <form className="palette-body" onSubmit={(e) => void launch(e)}>
          <p id="palette-title" className="dialog-title">
            Preset {mode.preset.name}
          </p>
          <p className="dialog-text">{mode.preset.detail}, di workspace baru.</p>
          <label className="launcher-field" htmlFor="palette-folder">
            Folder
          </label>
          <div className="launcher-row">
            <input
              id="palette-folder"
              data-autofocus
              className="mono input"
              value={mode.folder}
              spellCheck={false}
              autoComplete="off"
              onChange={(e) => setMode({ ...mode, folder: e.target.value })}
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
                  <button
                    type="button"
                    className="link-btn mono"
                    onClick={() => setMode({ ...mode, folder: tildePath(f, home) })}
                  >
                    {tildePath(f, home)}
                  </button>
                </li>
              ))}
            </ul>
          ) : null}
          {mode.error ? (
            <div className="launcher-error" role="alert">
              <span>{mode.error.message}</span>
              {mode.error.install ? <InstallHint command={mode.error.install} /> : null}
            </div>
          ) : null}
          <div className="dialog-actions">
            <button type="button" className="btn" onClick={() => setMode({ kind: "list" })}>
              Kembali
            </button>
            <button type="submit" className="btn btn-primary" disabled={mode.busy}>
              Buka
            </button>
          </div>
        </form>
      )}
    </dialog>
  );
}
