import { useContext, useEffect, useRef, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { FitAddon } from "@xterm/addon-fit";
import { SearchAddon } from "@xterm/addon-search";
import { WebglAddon } from "@xterm/addon-webgl";
import { type IMarker, Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { TerminalContext, xtermTheme } from "../settings/settings";
import { parse133, step, tracker } from "./commands";
import "./commands.css";
import { FindBar } from "./FindBar";
import { activity, cumulativeAcker, inputClock, serial, writeQueue } from "./io";

/** A CSS variable as resolved on `el`, where the pane's `data-term-theme` applies. */
function reader(el: Element = document.documentElement): (name: string) => string {
  const style = getComputedStyle(el);
  return (name) => style.getPropertyValue(name).trim();
}

/**
 * Terminal cell size in CSS pixels for the split limit (UI-UX §2). Mirrors xterm.js: the measured glyph is snapped to
 * device pixels (width floored, height ceiled), then the height is scaled by the line height and floored.
 */
export function measureCell(fontSize: number, fontFamily: string, lineHeight: number): { w: number; h: number } {
  const dpr = window.devicePixelRatio || 1;
  const ctx = document.createElement("canvas").getContext("2d");
  if (!ctx) return { w: fontSize * 0.6, h: Math.floor(fontSize * 1.2 * lineHeight) };
  ctx.font = `${fontSize}px ${fontFamily}`;
  const m = ctx.measureText("W");
  const charH = m.fontBoundingBoxAscent + m.fontBoundingBoxDescent || fontSize * 1.2;
  return { w: Math.floor(m.width * dpr) / dpr, h: Math.floor(Math.ceil(charH * dpr) * lineHeight) / dpr };
}

/** DOM events that mean the person is typing or pasting; see inputClock. */
const INPUT_EVENTS = ["keydown", "paste", "input", "compositionend"];

/**
 * xterm.js view of a core pane. The PTY is created by the caller (StrictMode runs this effect twice), so mounting only
 * attaches and unmounting only detaches: the process keeps running and the core buffers its output for replay.
 */
export function TerminalPane({
  id,
  focused,
  fontSize,
  findReq = 0,
  onActivity,
}: {
  id: number;
  focused: boolean;
  fontSize: number;
  /** ⌘F: a new value opens the find bar (or focuses it again). */
  findReq?: number;
  /** Output heuristic for the status dot: busy on output, quiet after a second without (ARCHITECTURE §6). */
  onActivity: (busy: boolean) => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const activityCb = useRef(onActivity);
  activityCb.current = onActivity;
  const term = useRef<{ t: Terminal; fit: FitAddon; search: SearchAddon } | null>(null);
  const [finding, setFinding] = useState(false);
  // Font, scrollback, theme (TERM-07). Read once at creation; later changes go through the effects below without
  // recreating the terminal.
  const cfg = useContext(TerminalContext);
  const initial = useRef({ fontSize, cfg });

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const { fontSize: size, cfg: c } = initial.current;
    const t = new Terminal({
      fontFamily: c.font_family,
      fontSize: size,
      lineHeight: c.line_height,
      theme: xtermTheme(reader(el)),
      scrollback: c.scrollback,
      screenReaderMode: c.screen_reader,
      // The search addon's match decorations (⌘F) are still a proposed xterm.js API.
      allowProposedApi: true,
    });
    const fit = new FitAddon();
    t.loadAddon(fit);
    const search = new SearchAddon();
    t.loadAddon(search);
    t.open(el);
    try {
      const gl = new WebglAddon();
      // Disposing the addon falls back to the DOM renderer.
      gl.onContextLoss(() => gl.dispose());
      t.loadAddon(gl);
    } catch {
      // No WebGL2: the DOM renderer stays.
    }
    term.current = { t, fit, search };
    let live = true;

    const ack = cumulativeAcker((total) => {
      if (live) invoke("pane_ack", { id, total }).catch(() => undefined);
    });
    const busy = activity((b) => activityCb.current(b));
    const ch = new Channel<ArrayBuffer>();
    ch.onmessage = (buf) => {
      if (!live) return;
      busy.mark();
      const bytes = new Uint8Array(buf);
      t.write(bytes, () => ack(bytes.length));
    };
    serial(id, () => invoke("pane_attach", { id, onData: ch })).catch((e: unknown) => {
      if (live) t.write(`\r\n[agentdeck] ${String(e)}\r\n`);
    });

    // Command blocks (D-031). Only the zsh integration of a shell pane sends OSC 133, so agent panes get no marks.
    const commands = new Set<IMarker>();
    const feed = tracker<IMarker>({
      mark: () => t.registerMarker(0),
      drop: (m) => m.dispose(),
      start: (m) => {
        commands.add(m);
        m.onDispose(() => commands.delete(m));
      },
      finish: (m, exit) => {
        const state = exit === 0 ? "cmd-ok" : exit === null ? "cmd-unknown" : "cmd-fail";
        t.registerDecoration({ marker: m, width: 1 })?.onRender((d) => {
          d.classList.add("cmd-mark", state);
          d.title = exit === null ? "Kode exit tidak diketahui" : `Kode exit ${exit}`;
        });
      },
    });
    const osc = t.parser.registerOscHandler(133, (data) => {
      const mark = parse133(data);
      if (mark) feed(mark);
      return true;
    });
    // ⌘↑/⌘↓ jump to the previous/next command (xterm.js does nothing with them). From the bottom the reference is the
    // cursor; after a jump that could not scroll (target already in the last screen) it is the line jumped to.
    let last: { line: number; viewportY: number } | null = null;
    t.attachCustomKeyEventHandler((e) => {
      if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return true;
      if (!e.metaKey || e.altKey || e.ctrlKey || e.shiftKey) return true;
      if (e.type !== "keydown") return false;
      const buf = t.buffer.active;
      if (buf.type !== "normal") return false;
      const atBottom = buf.viewportY === buf.baseY;
      const from = last?.viewportY === buf.viewportY ? last.line : atBottom ? buf.baseY + buf.cursorY : buf.viewportY;
      const line = step([...commands].map((m) => m.line), from, e.key === "ArrowUp" ? -1 : 1);
      if (line === undefined) {
        if (e.key === "ArrowDown") t.scrollToBottom();
        last = null;
        return false;
      }
      t.scrollToLine(line);
      last = { line, viewportY: buf.viewportY };
      return false;
    });

    const clock = inputClock();
    for (const ev of INPUT_EVENTS) el.addEventListener(ev, clock.mark, true);
    const write = writeQueue((data, typed) => invoke("pane_write", { id, data, typed }));
    const onData = t.onData((data) => write(data, clock.typed()));
    const onResize = t.onResize(({ rows, cols }) => {
      serial(id, () => invoke("pane_resize", { id, rows, cols })).catch(() => undefined);
    });
    // Fires once on observe too, which sizes the PTY to the pane.
    const ro = new ResizeObserver(() => fit.fit());
    ro.observe(el);
    const scheme = matchMedia("(prefers-color-scheme: dark)");
    const onScheme = () => {
      t.options.theme = xtermTheme(reader(el));
    };
    scheme.addEventListener("change", onScheme);

    return () => {
      live = false;
      busy.dispose();
      osc.dispose();
      scheme.removeEventListener("change", onScheme);
      ro.disconnect();
      onResize.dispose();
      onData.dispose();
      for (const ev of INPUT_EVENTS) el.removeEventListener(ev, clock.mark, true);
      serial(id, () => invoke("pane_detach", { id })).catch(() => undefined);
      t.dispose();
      term.current = null;
    };
  }, [id]);

  useEffect(() => {
    if (focused) term.current?.t.focus();
  }, [focused]);

  useEffect(() => {
    if (findReq > 0) setFinding(true);
  }, [findReq]);

  const closeFind = () => {
    term.current?.search.clearDecorations();
    setFinding(false);
    term.current?.t.focus();
  };

  // Decorations need #rrggbb, which the tokens are: other matches outlined in the accent, the active one in amber.
  const findOptions = () => {
    const token = reader(host.current ?? undefined);
    const accent = token("--term-accent");
    const active = token("--warn");
    return {
      decorations: {
        matchBorder: accent,
        matchOverviewRuler: accent,
        activeMatchBorder: active,
        activeMatchColorOverviewRuler: active,
      },
    };
  };

  useEffect(() => {
    const cur = term.current;
    if (!cur || cur.t.options.fontSize === fontSize) return;
    cur.t.options.fontSize = fontSize;
    cur.fit.fit();
  }, [fontSize]);

  useEffect(() => {
    const cur = term.current;
    if (!cur) return;
    const o = cur.t.options;
    if (o.fontFamily !== cfg.font_family || o.lineHeight !== cfg.line_height) {
      o.fontFamily = cfg.font_family;
      o.lineHeight = cfg.line_height;
      cur.fit.fit();
    }
    if (o.scrollback !== cfg.scrollback) o.scrollback = cfg.scrollback;
    if (o.screenReaderMode !== cfg.screen_reader) o.screenReaderMode = cfg.screen_reader;
  }, [cfg.font_family, cfg.line_height, cfg.scrollback, cfg.screen_reader]);

  // The attribute is already on the host when this runs, so the computed colours are the new theme's.
  useEffect(() => {
    const cur = term.current;
    if (cur && host.current) cur.t.options.theme = xtermTheme(reader(host.current));
  }, [cfg.theme]);

  const search = term.current?.search;
  return (
    <>
      <div ref={host} className="pane-term" data-term-theme={cfg.theme === "system" ? undefined : cfg.theme} />
      {finding && search ? (
        <FindBar search={search} options={findOptions} focusReq={findReq} onClose={closeFind} />
      ) : null}
    </>
  );
}
