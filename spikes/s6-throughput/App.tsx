// SPIKE S6 (branch spike/s6, not merged): measures PTY -> Channel -> xterm.js throughput and input latency.
import { useEffect, useRef } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { Terminal } from "@xterm/xterm";
import { WebglAddon } from "@xterm/addon-webgl";
import "@xterm/xterm/css/xterm.css";

const FLOOD = (import.meta.env.VITE_S6_FLOOD as string | undefined) ?? "yes | head -c 50000000";

function pct(xs: number[], p: number): number {
  if (xs.length === 0) return NaN;
  const s = [...xs].sort((a, b) => a - b);
  return Math.round(s[Math.min(s.length - 1, Math.floor((p / 100) * s.length))]);
}

// StrictMode runs effects twice in dev; the spike must start exactly once.
let started = false;

function App() {
  const a = useRef<HTMLDivElement>(null);
  const b = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!a.current || !b.current || started) return;
    started = true;
    const report = (line: string) => void invoke("report", { line });
    const mk = (el: HTMLDivElement) => {
      const t = new Terminal({ rows: 30, cols: 100, scrollback: 10000 });
      t.open(el);
      try {
        const gl = new WebglAddon();
        gl.onContextLoss(() => gl.dispose());
        t.loadAddon(gl);
      } catch {
        report("webgl unavailable, DOM renderer");
      }
      return t;
    };
    const ta = mk(a.current);
    const tb = mk(b.current);

    // Event-loop lag: a 16 ms timer that records how late it fires.
    const frames: number[] = [];
    let lastFrame = performance.now();
    let rafOn = true;
    const onFrame = (now: number) => {
      frames.push(now - lastFrame);
      lastFrame = now;
      if (rafOn) requestAnimationFrame(onFrame);
    };
    requestAnimationFrame(onFrame);
    const lags: number[] = [];
    let last = performance.now();
    const lagTimer = setInterval(() => {
      const now = performance.now();
      lags.push(now - last - 16);
      last = now;
    }, 16);

    // Pane B: cat echoes each char back; measure keystroke round trip during the flood.
    // Cumulative acks, coalesced to at most one per event-loop turn.
    const acker = (id: number) => {
      let total = 0;
      let scheduled = false;
      return (n: number) => {
        total += n;
        if (scheduled) return;
        scheduled = true;
        // Microtask, not setTimeout: WebKit throttles timers in hidden windows, which would throttle the PTY.
        queueMicrotask(() => {
          scheduled = false;
          invoke("pty_ack", { id, total }).catch((e) => report(`ack${id} err ${String(e)}`));
        });
      };
    };
    const ackA = acker(1);
    const ackB = acker(2);
    const pending = new Map<string, number>();
    const rtts: number[] = [];
    const chB = new Channel<ArrayBuffer>();
    chB.onmessage = (buf) => {
      const bytes = new Uint8Array(buf);
      tb.write(bytes, () => ackB(bytes.length));
      for (const ch of new TextDecoder().decode(bytes)) {
        const t0 = pending.get(ch);
        if (t0 !== undefined) {
          rtts.push(performance.now() - t0);
          pending.delete(ch);
        }
      }
    };
    const keys = "abcdefghijklmnopqrstuvwxz";
    let k = 0;

    let received = 0;
    let written = 0;
    let start = 0;
    let lastByte = 0;
    let lastWritten = 0;
    let done = false;
    const chA = new Channel<ArrayBuffer>();
    chA.onmessage = (buf) => {
      const bytes = new Uint8Array(buf);
      lastByte = performance.now();
      received += bytes.length;
      ta.write(bytes, () => {
        written += bytes.length;
        lastWritten = performance.now();
        ackA(bytes.length);
      });
    };

    void (async () => {
      await invoke("pty_spawn", { id: 2, argv: ["/bin/cat"], rows: 30, cols: 100, onData: chB });
      await new Promise((r) => setTimeout(r, 1500));
      const keyTimer = setInterval(() => {
        const ch = keys[k++ % keys.length];
        pending.set(ch, performance.now());
        invoke("pty_write", { id: 2, data: ch }).catch((e) => report(`write err ${String(e)}`));
      }, 100);
      lags.length = 0;
      frames.length = 0;
      start = performance.now();
      await invoke("pty_spawn", { id: 1, argv: ["/bin/sh", "-c", `${FLOOD}; echo; echo S6-END`], rows: 30, cols: 100, onData: chA });
      const poll = setInterval(() => {
        const idle = performance.now() - lastByte;
        if (done || received === 0 || written < received) return;
        if (idle < 1000) return;
        done = true;
        clearInterval(poll);
        clearInterval(keyTimer);
        clearInterval(lagTimer);
        const secs = (lastWritten - start) / 1000;
        report(`flood=${JSON.stringify(FLOOD)} bytes=${received} written=${written} seconds=${secs.toFixed(2)} MBps=${(received / 1e6 / secs).toFixed(1)}`);
        rafOn = false;
        report(`frame_gap_ms p50=${pct(frames, 50)} p95=${pct(frames, 95)} p99=${pct(frames, 99)} max=${pct(frames, 100)} frames=${frames.length} visible=${document.visibilityState}`);
        report(`event_loop_lag_ms p50=${pct(lags, 50)} p95=${pct(lags, 95)} max=${pct(lags, 100)} samples=${lags.length}`);
        report(`keystroke_rtt_ms p50=${pct(rtts, 50)} p95=${pct(rtts, 95)} max=${pct(rtts, 100)} samples=${rtts.length} lost=${pending.size}`);
        report("DONE");
      }, 250);
    })();
  }, []);
  return (
    <main style={{ display: "flex", gap: 8 }}>
      <div ref={a} />
      <div ref={b} />
    </main>
  );
}

export default App;
