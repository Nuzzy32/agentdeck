import { useEffect, useRef } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { type ITheme, Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { cumulativeAcker, inputClock, serial, writeQueue } from "./io";

function token(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

/** Terminal colours from tokens.css (UI-UX §6); the 16 ANSI colours stay xterm.js defaults. */
function theme(): ITheme {
  return {
    background: token("--bg"),
    foreground: token("--fg"),
    cursor: token("--fg"),
    cursorAccent: token("--bg"),
    // Tokens are #rrggbb, so a hex alpha suffix gives a translucent selection.
    selectionBackground: `${token("--accent")}4d`,
  };
}

/** DOM events that mean the person is typing or pasting; see inputClock. */
const INPUT_EVENTS = ["keydown", "paste", "input", "compositionend"];

/**
 * xterm.js view of a core pane. The PTY is created by the caller (StrictMode runs this effect twice), so mounting only
 * attaches and unmounting only detaches: the process keeps running and the core buffers its output for replay.
 */
export function TerminalPane({ id, focused }: { id: number; focused: boolean }) {
  const host = useRef<HTMLDivElement>(null);
  const term = useRef<Terminal | null>(null);

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const t = new Terminal({
      fontFamily: token("--font-mono"),
      fontSize: parseFloat(token("--text-ui")),
      lineHeight: 1.2,
      theme: theme(),
      scrollback: 10000,
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
    term.current = t;
    let live = true;

    const ack = cumulativeAcker((total) => {
      if (live) invoke("pane_ack", { id, total }).catch(() => undefined);
    });
    const ch = new Channel<ArrayBuffer>();
    ch.onmessage = (buf) => {
      if (!live) return;
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
      t.options.theme = theme();
    };
    scheme.addEventListener("change", onScheme);

    return () => {
      live = false;
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
    if (focused) term.current?.focus();
  }, [focused]);

  return <div ref={host} className="pane-term" />;
}
