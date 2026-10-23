// Command blocks from OSC 133 (D-031): the shell integration marks prompt start (A), prompt end (B), command start
// (C) and command finished with its exit code (D;<code>). Kept free of xterm.js so the logic can be tested.

export type Mark = { kind: "A" | "B" | "C" } | { kind: "D"; exit: number | null };

/** The payload after `133;`, e.g. `D;1` or `A;aid=7`; anything else is ignored. */
export function parse133(data: string): Mark | null {
  const [kind, code] = data.split(";");
  if (kind === "A" || kind === "B" || kind === "C") return { kind };
  if (kind !== "D") return null;
  return { kind: "D", exit: code !== undefined && /^\d+$/.test(code) ? Number(code) : null };
}

/**
 * Turns marks into commands: `mark()` places a marker on the prompt line at A, a prompt where nothing ran (empty
 * Enter, ^C) is `drop`ped at the next A, `start` fires at C and `finish` at D with the exit code.
 */
export function tracker<M>(api: {
  mark(): M;
  drop(m: M): void;
  start(m: M): void;
  finish(m: M, exit: number | null): void;
}): (mark: Mark) => void {
  let prompt: M | null = null;
  let ran = false;
  return (mark) => {
    if (mark.kind === "A") {
      if (prompt !== null && !ran) api.drop(prompt);
      prompt = api.mark();
      ran = false;
    } else if (mark.kind === "C") {
      // C without A: a shell that marks only commands; the marker goes on the current line.
      prompt ??= api.mark();
      if (!ran) api.start(prompt);
      ran = true;
    } else if (mark.kind === "D" && prompt !== null && ran) {
      api.finish(prompt, mark.exit);
      prompt = null;
      ran = false;
    }
  };
}

/** The nearest command line before (`-1`) or after (`1`) buffer line `from`, if any. */
export function step(lines: number[], from: number, dir: 1 | -1): number | undefined {
  let best: number | undefined;
  for (const l of lines) {
    if (dir < 0 ? l < from && (best === undefined || l > best) : l > from && (best === undefined || l < best)) {
      best = l;
    }
  }
  return best;
}
