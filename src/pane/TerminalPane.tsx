import { useContext, useEffect, useRef } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { TerminalContext, xtermTheme } from "../settings/settings";
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
  onActivity,
}: {
  id: number;
  focused: boolean;
  fontSize: number;
  /** Output heuristic for the status dot: busy on output, quiet after a second without (ARCHITECTURE §6). */
  onActivity: (busy: boolean) => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const activityCb = useRef(onActivity);
  activityCb.current = onActivity;
  const term = useRef<{ t: Terminal; fit: FitAddon } | null>(null);
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
    });
    const fit = new FitAddon();
    t.loadAddon(fit);
    t.open(el);
    try {
      const gl = new WebglAddon();
      // Disposing the addon falls back to the DOM renderer.
      gl.onContextLoss(() => gl.dispose());
      t.loadAddon(gl);
    } catch {
      // No WebGL2: the DOM renderer stays.
    }
    term.current = { t, fit };
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

  return (
    <div ref={host} className="pane-term" data-term-theme={cfg.theme === "system" ? undefined : cfg.theme} />
  );
}
