import { isValidElement, type ReactElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { shortcut } from "../layout/keys";
// @ts-expect-error no @types/node in this project (same as vite.config.ts); Vitest runs in Node.
import { readFileSync } from "node:fs";
import {
  AgentRow,
  AgentsSection,
  CollabSection,
  SettingsDialog,
  Stepper,
  TerminalSection,
} from "./SettingsModal";
import {
  type AgentDetail,
  DEFAULTS,
  FONT_SIZE,
  LIMITS,
  LINE_HEIGHT,
  loadAgents,
  merge,
  parseInRange,
  step,
  xtermTheme,
} from "./settings";

// No DOM here: hookless components are called as functions and their element trees searched; components with state
// are rendered to markup.
type Props = Record<string, unknown> & { children?: ReactNode };

function all(node: ReactNode): ReactElement<Props>[] {
  if (Array.isArray(node)) return node.flatMap(all);
  if (!isValidElement<Props>(node)) return [];
  return [node, ...all(node.props.children)];
}

function find(node: ReactNode, pred: (e: ReactElement<Props>) => boolean): ReactElement<Props> {
  const hit = all(node).find(pred);
  if (!hit) throw new Error("element not found");
  return hit;
}

function text(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(text).join("");
  return isValidElement<Props>(node) ? text(node.props.children) : "";
}

const call = (e: ReactElement<Props>, name: string, ...args: unknown[]) => (e.props[name] as (...a: unknown[]) => void)(...args);

describe("stepper", () => {
  const children = LIMITS.find((l) => l.key === "children")!.range;

  it("ranges are the MCP-TOOLS ones and the defaults sit inside them", () => {
    expect(LIMITS.map((l) => [l.key, l.range.min, l.range.max])).toEqual([
      ["children", 1, 5],
      ["global", 1, 12],
      ["spawn_rate", 1, 10],
      ["approval_timeout_s", 30, 300],
      ["loop_guard", 2, 50],
    ]);
    for (const l of LIMITS) {
      const v = DEFAULTS.limits[l.key];
      expect(v >= l.range.min && v <= l.range.max).toBe(true);
    }
  });

  it("refuses typed values outside the range, fractions and junk", () => {
    expect(parseInRange("6", children)).toEqual({ error: "Harus 1–5, bilangan bulat." });
    expect(parseInRange("0", children)).toHaveProperty("error");
    expect(parseInRange("2.5", children)).toHaveProperty("error");
    expect(parseInRange("", children)).toHaveProperty("error");
    expect(parseInRange("abc", children)).toHaveProperty("error");
    expect(parseInRange(" 5 ", children)).toEqual({ value: 5 });
    expect(parseInRange("1", children)).toEqual({ value: 1 });
    expect(parseInRange("1,5", LINE_HEIGHT)).toEqual({ value: 1.5 });
    expect(parseInRange("2.1", LINE_HEIGHT)).toEqual({ error: "Harus 1–2." });
    expect(parseInRange("25", FONT_SIZE)).toHaveProperty("error");
  });

  it("buttons stay in range without float noise", () => {
    expect(step(5, children, 1)).toBe(5);
    expect(step(1, children, -1)).toBe(1);
    expect(step(1.2, LINE_HEIGHT, 1)).toBe(1.3);
    expect(step(1.9, LINE_HEIGHT, 1)).toBe(2);
  });

  it("disables − at the minimum and + at the maximum, and shows the range", () => {
    const html = (value: number) =>
      renderToStaticMarkup(<Stepper label="Anak" value={value} range={children} onChange={() => {}} />);
    expect(html(1)).toMatch(/aria-label="Kurangi Anak" disabled/);
    expect(html(1)).not.toMatch(/aria-label="Tambah Anak" disabled/);
    expect(html(5)).toMatch(/aria-label="Tambah Anak" disabled/);
    expect(html(3)).toContain("1–5");
  });
});

describe("open and close", () => {
  const key = (k: Partial<KeyboardEvent>) =>
    shortcut({ key: "", code: "", metaKey: false, shiftKey: false, altKey: false, ctrlKey: false, ...k });

  it("⌘, opens Settings; a plain comma stays with the terminal", () => {
    expect(key({ key: ",", code: "Comma", metaKey: true })).toEqual({ type: "settings" });
    expect(key({ key: ",", code: "Comma" })).toBeNull();
    expect(key({ key: ",", code: "Comma", metaKey: true, ctrlKey: true })).toBeNull();
  });

  it("Esc (the dialog's cancel) closes, and the navigation switches sections", () => {
    const onClose = vi.fn();
    const onSection = vi.fn();
    const tree = SettingsDialog({ section: "collab", onSection, onClose, children: null });
    const dialog = find(tree, (e) => e.type === "dialog");
    const preventDefault = vi.fn();
    call(dialog, "onCancel", { preventDefault });
    expect(onClose).toHaveBeenCalledOnce();
    expect(preventDefault).toHaveBeenCalledOnce();

    const nav = all(tree).filter((e) => e.type === "button" && e.props["aria-current"] !== undefined);
    expect(nav.map((b) => text(b.props.children))).toEqual(["Kolaborasi"]);
    call(find(tree, (e) => e.type === "button" && text(e.props.children) === "Terminal"), "onClick");
    expect(onSection).toHaveBeenLastCalledWith("terminal");
  });
});

describe("Kolaborasi and Terminal save at once", () => {
  it("switches and steppers send a patch of their own key", () => {
    const onChange = vi.fn();
    const tree = CollabSection({ settings: DEFAULTS, onChange });
    call(find(tree, (e) => e.props.label === "Agen boleh membuka pane"), "onChange", true);
    expect(onChange).toHaveBeenLastCalledWith({ spawn_enabled: true });
    call(find(tree, (e) => e.props.label === "Saling kirim untuk workspace baru"), "onChange", true);
    expect(onChange).toHaveBeenLastCalledWith({ peer_default: true });
    call(find(tree, (e) => e.props.label === "Pesan antar agen tanpa masukanmu"), "onChange", 20);
    expect(onChange).toHaveBeenLastCalledWith({ limits: { loop_guard: 20 } });
  });

  it("theme radio and merge", () => {
    const onChange = vi.fn();
    const tree = TerminalSection({ settings: DEFAULTS, onChange });
    const checked = all(tree).filter((e) => e.type === "input" && e.props.checked === true);
    expect(checked.map((e) => e.props.value)).toEqual(["system"]);
    call(find(tree, (e) => e.type === "input" && e.props.value === "light"), "onChange");
    expect(onChange).toHaveBeenLastCalledWith({ terminal: { theme: "light" } });
    const next = merge(DEFAULTS, { terminal: { theme: "light" } });
    expect(next.terminal).toEqual({ ...DEFAULTS.terminal, theme: "light" });
    expect(next.limits).toEqual(DEFAULTS.limits);
  });
});

const claude: AgentDetail = {
  id: "claude-code",
  name: "Claude Code",
  installed: true,
  version: "2.1.0",
  path: "/opt/homebrew/bin/claude",
  path_override: "/opt/homebrew/bin/claude",
  install: "npm i -g @anthropic-ai/claude-code",
  login: { method: "Claude.ai", detail: "Pro" },
  api_key_env: ["ANTHROPIC_API_KEY"],
};

describe("Agen", () => {
  it("uses agents_info when the core has it", async () => {
    const invoke = vi.fn(async () => [claude]);
    const view = await loadAgents(invoke as never);
    expect(view).toEqual({ agents: [claude], detailed: true });
    expect(invoke).toHaveBeenCalledWith("agents_info");
  });

  it("falls back to agents_list without path, login, or the manual path field", async () => {
    const invoke = vi.fn(async (cmd: string) => {
      if (cmd === "agents_info") throw new Error("unknown command agents_info");
      return [{ id: "codex", name: "Codex", installed: false, version: null, install: "npm i -g @openai/codex" }];
    });
    const view = await loadAgents(invoke as never);
    expect(view.detailed).toBe(false);
    expect(view.agents[0]).toMatchObject({ id: "codex", path: null, login: null, api_key_env: [] });
    const html = renderToStaticMarkup(<AgentsSection view={view} error={null} onSetPath={() => {}} />);
    expect(html).toContain("Tidak terdeteksi");
    expect(html).toContain("npm i -g @openai/codex");
    expect(html).not.toContain("Simpan path");
    expect(html).toContain("belum tersedia");
  });

  it("shows detection, version, path, login, the API key notice, and the manual path", () => {
    const html = renderToStaticMarkup(
      <AgentsSection view={{ agents: [claude], detailed: true }} error={null} onSetPath={() => {}} />,
    );
    for (const s of ["Terdeteksi", "2.1.0", "/opt/homebrew/bin/claude", "Login: Claude.ai · Pro", "Simpan path"]) {
      expect(html).toContain(s);
    }
    expect(html).toContain("agentdeck tidak meneruskan ANTHROPIC_API_KEY");
    const onSetPath = vi.fn();
    const row = AgentRow({ agent: claude, detailed: true, onSetPath });
    call(find(row, (e) => e.type === "button" && text(e.props.children) === "Deteksi otomatis"), "onClick");
    expect(onSetPath).toHaveBeenCalledWith("claude-code", null);
    expect(renderToStaticMarkup(<AgentsSection view={null} error={null} onSetPath={() => {}} />)).toContain(
      "Memeriksa agen",
    );
  });
});

describe("terminal theme", () => {
  it("xterm colours come from the --term tokens", () => {
    const vals: Record<string, string> = { "--term-bg": "#0e1116", "--term-fg": "#e6e9ef", "--term-accent": "#5b9dff" };
    expect(xtermTheme((n) => vals[n] ?? "")).toEqual({
      background: "#0e1116",
      foreground: "#e6e9ef",
      cursor: "#e6e9ef",
      cursorAccent: "#0e1116",
      selectionBackground: "#5b9dff4d",
    });
  });

  it("Gelap and Terang pin the app's own dark and light tokens; Sistem follows them", () => {
    // Vitest turns CSS imports (also ?raw) into empty strings, so the file is read directly.
    const tokens = String(readFileSync(new URL("../styles/tokens.css", import.meta.url), "utf8"));
    const block = (selector: string) => {
      const i = tokens.indexOf(selector);
      return tokens.slice(i, tokens.indexOf("}", i));
    };
    const value = (css: string, name: string) => new RegExp(`${name}:\\s*([^;]+);`).exec(css)?.[1];
    const light = block(":root {");
    const dark = block("@media (prefers-color-scheme: dark)");
    for (const [theme, app] of [
      ["dark", dark],
      ["light", light],
    ] as const) {
      const term = block(`[data-term-theme="${theme}"]`);
      for (const t of ["bg", "fg", "accent"]) {
        const want = value(app, `--${t}`);
        expect(want).toMatch(/^#[0-9a-f]{6}$/);
        expect(value(term, `--term-${t}`)).toBe(want);
      }
    }
    expect(tokens).toMatch(/--term-bg: var\(--bg\);/);
  });
});
