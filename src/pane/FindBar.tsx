// Find in pane (TERM-03, ⌘F): a small bar over the top right of the terminal, driving xterm.js's search addon. It sits
// outside the terminal's host element, so typing here never counts as typing into the pane (delivery waits for that).
import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import type { ISearchOptions, SearchAddon } from "@xterm/addon-search";
import { IconButton } from "../layout/Icon";

export type Results = { index: number; count: number };

/** "3/12", "Tidak ada" for no match, "?/12" while the active match is past the highlight limit. */
export function findLabel(r: Results | null): string {
  if (!r) return "";
  if (r.count === 0) return "Tidak ada";
  return `${r.index < 0 ? "?" : r.index + 1}/${r.count}`;
}

export function FindBar({
  search,
  options,
  focusReq,
  onClose,
}: {
  search: SearchAddon;
  /** Match colours come from the pane's tokens at search time. */
  options: () => ISearchOptions;
  /** Changes on every ⌘F: focus and select the text again. */
  focusReq: number;
  onClose: () => void;
}) {
  const input = useRef<HTMLInputElement>(null);
  const [text, setText] = useState("");
  const [results, setResults] = useState<Results | null>(null);

  useEffect(() => {
    const sub = search.onDidChangeResults(({ resultIndex, resultCount }) =>
      setResults({ index: resultIndex, count: resultCount }),
    );
    return () => sub.dispose();
  }, [search]);

  useEffect(() => {
    input.current?.focus();
    input.current?.select();
  }, [focusReq]);

  const go = (back: boolean, q = text, incremental = false) => {
    if (!q) {
      search.clearDecorations();
      setResults(null);
      return;
    }
    const o = { ...options(), incremental };
    if (back) search.findPrevious(q, o);
    else search.findNext(q, o);
  };

  const onKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      e.preventDefault();
      go(e.shiftKey);
    } else if (e.key === "Escape") {
      e.preventDefault();
      onClose();
    }
  };

  return (
    <div className="find-bar" role="search">
      <input
        ref={input}
        className="find-input"
        type="text"
        value={text}
        placeholder="Cari"
        aria-label="Cari di pane"
        spellCheck={false}
        onChange={(e) => {
          setText(e.target.value);
          go(false, e.target.value, true);
        }}
        onKeyDown={onKey}
      />
      <span className="find-count mono" aria-live="polite">
        {findLabel(results)}
      </span>
      <IconButton icon="chevronUp" label="Sebelumnya (⇧Enter)" onClick={() => go(true)} />
      <IconButton icon="chevronDown" label="Berikutnya (Enter)" onClick={() => go(false)} />
      <IconButton icon="x" label="Tutup (Esc)" onClick={onClose} />
    </div>
  );
}
