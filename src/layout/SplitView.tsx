import { type PointerEvent as ReactPointerEvent, type ReactNode, useEffect, useRef, useState } from "react";
import { type Cell, CHROME, clampRatio, divider, type Path, type Rect, rects, type Split, splits, type Tree } from "./tree";

/**
 * Lays the split tree out with absolute positions computed by tree.ts, so a pane keeps its DOM node (and its
 * xterm.js) when the tree around it changes. While a pane is zoomed the others stay mounted but invisible.
 */
export function SplitView({
  tree,
  zoomed,
  cell,
  renderPane,
  onRatio,
  onSize,
}: {
  tree: Tree;
  zoomed: string | null;
  cell: Cell;
  renderPane: (id: string) => ReactNode;
  onRatio: (path: Path, ratio: number) => void;
  /** Reports the area size so the app can check the 40 × 8 limit before a split. */
  onSize: (size: Rect) => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState<Rect | null>(null);
  const onSizeRef = useRef(onSize);
  onSizeRef.current = onSize;

  useEffect(() => {
    const el = host.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      const r = { x: 0, y: 0, w: el.clientWidth, h: el.clientHeight };
      setSize(r);
      onSizeRef.current(r);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const panes = size ? rects(tree, size) : new Map<string, Rect>();
  const place = (r: Rect) => ({ left: r.x, top: r.y, width: r.w, height: r.h });

  const drag = (path: Path, node: Split, r: Rect) => (e: ReactPointerEvent<HTMLDivElement>) => {
    e.preventDefault();
    const el = e.currentTarget;
    el.setPointerCapture(e.pointerId);
    const origin = host.current?.getBoundingClientRect();
    if (!origin) return;
    const move = (ev: PointerEvent) => {
      const total = node.dir === "row" ? r.w : r.h;
      const pos = node.dir === "row" ? ev.clientX - origin.left - r.x : ev.clientY - origin.top - r.y;
      onRatio(path, clampRatio(node, total, pos / (total - CHROME.gap), cell));
    };
    const up = () => {
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
  };

  return (
    <div ref={host} className="split-area">
      {[...(size ? panes : [])].map(([id, r]) => {
        const hidden = zoomed !== null && zoomed !== id;
        const box = zoomed === id && size ? size : r;
        return (
          // visibility, not display: a hidden pane keeps its size, so its PTY is not resized to zero.
          <div key={id} className="split-cell" style={{ ...place(box), visibility: hidden ? "hidden" : undefined }}>
            {renderPane(id)}
          </div>
        );
      })}
      {size && zoomed === null
        ? splits(tree, size).map(({ path, node, rect }) => (
            <div
              key={`d${path}`}
              className={`split-divider split-divider-${node.dir}`}
              style={place(divider(node.dir, node.ratio, rect))}
              role="separator"
              aria-orientation={node.dir === "row" ? "vertical" : "horizontal"}
              aria-label="Seret untuk mengubah ukuran pane"
              onPointerDown={drag(path, node, rect)}
            />
          ))
        : null}
    </div>
  );
}
