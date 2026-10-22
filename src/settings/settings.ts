// settings.json as the core keeps it (src-tauri/src/settings.rs). The core validates every value; the ranges here only
// let the UI refuse a value in place, before asking.
import { createContext, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ITheme } from "@xterm/xterm";
import type { AgentInfo as ListedAgent } from "../layout/model";

export type Theme = "system" | "dark" | "light";

export type TerminalSettings = {
  font_family: string;
  font_size: number;
  line_height: number;
  scrollback: number;
  theme: Theme;
  screen_reader: boolean;
};

export type Limits = {
  children: number;
  global: number;
  spawn_rate: number;
  approval_timeout_s: number;
  loop_guard: number;
};

export type Settings = {
  spawn_enabled: boolean;
  peer_default: boolean;
  limits: Limits;
  terminal: TerminalSettings;
};

/** A partial Settings for `settings_set`. */
export type Patch = { [K in keyof Settings]?: Settings[K] extends object ? Partial<Settings[K]> : Settings[K] };

export const DEFAULTS: Settings = {
  spawn_enabled: false,
  peer_default: false,
  limits: { children: 3, global: 8, spawn_rate: 5, approval_timeout_s: 120, loop_guard: 12 },
  terminal: {
    font_family: '"SF Mono", Menlo, monospace',
    font_size: 13,
    line_height: 1.2,
    scrollback: 10000,
    theme: "system",
    screen_reader: false,
  },
};

export type Range = { min: number; max: number; step: number };

/** MCP-TOOLS "Batas bawaan", in Settings › Kolaborasi order. */
export const LIMITS: { key: keyof Limits; label: string; range: Range }[] = [
  { key: "children", label: "Anak hidup per pane", range: { min: 1, max: 5, step: 1 } },
  { key: "global", label: "Pane agen dibuka agen (semua workspace)", range: { min: 1, max: 12, step: 1 } },
  { key: "spawn_rate", label: "Spawn per 10 menit", range: { min: 1, max: 10, step: 1 } },
  { key: "approval_timeout_s", label: "Batas waktu kartu persetujuan (detik)", range: { min: 30, max: 300, step: 10 } },
  { key: "loop_guard", label: "Pesan antar agen tanpa masukanmu", range: { min: 2, max: 50, step: 1 } },
];

export const FONT_SIZE: Range = { min: 10, max: 24, step: 1 };
export const LINE_HEIGHT: Range = { min: 1, max: 2, step: 0.1 };
export const SCROLLBACK: Range = { min: 0, max: 100000, step: 1000 };

/** Typed text of a stepper: the number, or why it is refused. */
export function parseInRange(text: string, r: Range): { value: number } | { error: string } {
  const t = text.trim().replace(",", ".");
  const n = t === "" ? Number.NaN : Number(t);
  const inRange = Number.isFinite(n) && n >= r.min && n <= r.max;
  if (!inRange || (Number.isInteger(r.step) && !Number.isInteger(n))) {
    return { error: `Harus ${r.min}–${r.max}${Number.isInteger(r.step) ? ", bilangan bulat" : ""}.` };
  }
  return { value: n };
}

/** One stepper press, kept in range and free of float noise (1.2 + 0.1). */
export function step(value: number, r: Range, dir: 1 | -1): number {
  const next = Math.round((value + dir * r.step) * 1000) / 1000;
  return Math.min(r.max, Math.max(r.min, next));
}

export function merge(s: Settings, p: Patch): Settings {
  return {
    ...s,
    ...p,
    limits: { ...s.limits, ...p.limits },
    terminal: { ...s.terminal, ...p.terminal },
  } as Settings;
}

/** Terminal settings for every TerminalPane, provided by App. */
export const TerminalContext = createContext<TerminalSettings>(DEFAULTS.terminal);

/**
 * xterm.js colours from the `--term-*` tokens (tokens.css), read where the pane's theme applies; the 16 ANSI colours
 * stay xterm.js defaults.
 */
export function xtermTheme(token: (name: string) => string): ITheme {
  const bg = token("--term-bg");
  const fg = token("--term-fg");
  // Tokens are #rrggbb, so a hex alpha suffix gives a translucent selection.
  const selection = `${token("--term-accent")}4d`;
  return { background: bg, foreground: fg, cursor: fg, cursorAccent: bg, selectionBackground: selection };
}

/** The core's settings, live: changes from any window arrive through `agentdeck://settings-changed`. */
export function useSettings(onError: (text: string) => void) {
  const [settings, setSettings] = useState<Settings>(DEFAULTS);
  const reload = () => invoke<Settings>("settings_get").then(setSettings, (e: unknown) => onError(String(e)));

  useEffect(() => {
    void reload();
    let stop: (() => void) | null = null;
    let live = true;
    listen<Settings>("agentdeck://settings-changed", (e) => setSettings(e.payload)).then(
      (un) => (live ? (stop = un) : un()),
      () => undefined,
    );
    return () => {
      live = false;
      stop?.();
    };
  }, []);

  /** Shown at once; a refusal from the core puts the saved values back. */
  const update = (patch: Patch) => {
    setSettings((s) => merge(s, patch));
    invoke<Settings>("settings_set", { patch }).then(setSettings, (e: unknown) => {
      onError(`Pengaturan tidak disimpan: ${String(e)}`);
      void reload();
    });
  };

  return { settings, update };
}

/** `agents_info` (catalogue session, T4.4/T4.5). */
export type AgentDetail = {
  id: string;
  name: string;
  installed: boolean;
  version: string | null;
  path: string | null;
  path_override: string | null;
  install: string;
  login: { method: string; detail: string | null } | null;
  api_key_env: string[];
};

export type AgentsView = { agents: AgentDetail[]; detailed: boolean };

type Invoke = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

/** `agents_info`, or the launcher's `agents_list` when the core does not have it yet (no path, login, or override). */
export async function loadAgents(call: Invoke = invoke): Promise<AgentsView> {
  try {
    return { agents: await call<AgentDetail[]>("agents_info"), detailed: true };
  } catch {
    const list = await call<ListedAgent[]>("agents_list");
    return {
      agents: list.map((a) => ({ ...a, path: null, path_override: null, login: null, api_key_env: [] })),
      detailed: false,
    };
  }
}
