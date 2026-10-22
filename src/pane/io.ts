// Pure helpers for TerminalPane; no DOM or Tauri, so they run under Vitest.

/**
 * Cumulative ack (D-018): adds each confirmed chunk to a running total and sends at most one total per event-loop
 * turn. Microtask, not setTimeout: WebKit throttles timers in hidden windows, which would throttle the PTY.
 */
export function cumulativeAcker(send: (total: number) => void): (n: number) => void {
  let total = 0;
  let scheduled = false;
  return (n) => {
    total += n;
    if (scheduled) return;
    scheduled = true;
    queueMicrotask(() => {
      scheduled = false;
      send(total);
    });
  };
}

/** How long after a keyboard/paste/input DOM event `onData` still counts as the person's own input. */
export const TYPED_WINDOW_MS = 100;

/**
 * Tells real typing from xterm.js replies (ARCHITECTURE §5.2). Both reach `onData`; only typing follows a DOM
 * keyboard/paste/input event. A window instead of a synchronous flag because IME composition delivers its data in a
 * later task. A reply that lands inside the window counts as typing, which only delays message delivery.
 */
export function inputClock(now: () => number = () => performance.now()) {
  let last = -Infinity;
  return {
    mark: () => {
      last = now();
    },
    typed: () => now() - last < TYPED_WINDOW_MS,
  };
}

/**
 * Serialises writes to one pane: Tauri runs async commands concurrently, so two invokes in flight can reach the PTY
 * out of order. Data that arrives while a write is in flight is batched into the next one; the batch is typed if any
 * part was.
 */
export function writeQueue(send: (data: string, typed: boolean) => Promise<unknown>): (data: string, typed: boolean) => void {
  let buf = "";
  let bufTyped = false;
  let busy = false;
  const flush = async () => {
    busy = true;
    while (buf) {
      const data = buf;
      const typed = bufTyped;
      buf = "";
      bufTyped = false;
      await send(data, typed).catch(() => undefined);
    }
    busy = false;
  };
  return (data, typed) => {
    buf += data;
    bufTyped ||= typed;
    if (!busy) void flush();
  };
}

const chains = new Map<number, Promise<unknown>>();

/**
 * Runs `op` after earlier ops for the same pane settle. Keeps attach/detach/resize in call order: under StrictMode a
 * detach overtaking the following attach would leave the pane without a WebView.
 */
export function serial<T>(id: number, op: () => Promise<T>): Promise<T> {
  const next = (chains.get(id) ?? Promise.resolve()).then(op, op);
  chains.set(
    id,
    next.catch(() => undefined),
  );
  return next;
}

/** Output within this long counts as `working` for the status heuristic (ARCHITECTURE §6). */
export const QUIET_MS = 1000;

/**
 * Status heuristic input: `onChange(true)` on the first output after a quiet spell, `onChange(false)` once QUIET_MS
 * pass without output. One timer per pane however much output flows. `dispose` reports quiet if still busy.
 */
export function activity(onChange: (busy: boolean) => void, now: () => number = () => performance.now()) {
  let busy = false;
  let last = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const check = () => {
    const left = last + QUIET_MS - now();
    if (left > 0) {
      timer = setTimeout(check, left);
      return;
    }
    timer = undefined;
    busy = false;
    onChange(false);
  };
  return {
    mark: () => {
      last = now();
      if (!busy) {
        busy = true;
        onChange(true);
      }
      timer ??= setTimeout(check, QUIET_MS);
    },
    dispose: () => {
      clearTimeout(timer);
      if (busy) onChange(false);
    },
  };
}
