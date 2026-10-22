import { isValidElement, type ReactElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import { EMPTY, type LoopGuard } from "../core/messages";
import { LoopRibbon, StatusBar } from "./Shell";

// LoopRibbon and StatusBar have no hooks, so calling them gives their element tree; clicks are tested by calling a
// button's onClick, as there is no DOM in these tests.
type Button = ReactElement<{ onClick?: () => void; disabled?: boolean; children?: ReactNode }>;

function text(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(text).join("");
  if (isValidElement<{ children?: ReactNode }>(node)) return text(node.props.children);
  return "";
}

function buttons(node: ReactNode): Button[] {
  if (Array.isArray(node)) return node.flatMap(buttons);
  if (!isValidElement<{ children?: ReactNode }>(node)) return [];
  return node.type === "button" ? [node as Button] : buttons(node.props.children);
}

const button = (node: ReactNode, label: string) => {
  const b = buttons(node).find((x) => text(x.props.children).startsWith(label));
  if (!b) throw new Error(`no button ${label}`);
  return b;
};

const paused: LoopGuard = { count: 12, limit: 12, paused: true };

describe("LoopRibbon", () => {
  const ribbon = (guard: LoopGuard) => {
    const props = { guard, onContinue: vi.fn(), onStop: vi.fn(), onShowLog: vi.fn() };
    return { props, tree: LoopRibbon(props) };
  };

  it("appears only while paused, with N from the limit, inside a polite live region", () => {
    const shown = renderToStaticMarkup(LoopRibbon({ ...ribbon(paused).props, guard: { ...paused, limit: 20 } }));
    expect(shown).toContain('aria-live="polite"');
    expect(shown).toContain("Agen sudah saling mengirim 20 pesan tanpa masukanmu. Antrean dijeda.");

    const hidden = renderToStaticMarkup(ribbon({ count: 3, limit: 12, paused: false }).tree);
    expect(hidden).toBe('<div aria-live="polite"></div>');
  });

  it("each button calls its own action", () => {
    const { props, tree } = ribbon(paused);
    button(tree, "Lanjutkan").props.onClick?.();
    expect(props.onContinue).toHaveBeenCalledOnce();
    expect(props.onStop).not.toHaveBeenCalled();
    button(tree, "Hentikan dan buang antrean").props.onClick?.();
    expect(props.onStop).toHaveBeenCalledOnce();
    button(tree, "Lihat log").props.onClick?.();
    expect(props.onShowLog).toHaveBeenCalledOnce();
  });
});

describe("StatusBar", () => {
  const bar = (guard: LoopGuard, peer: boolean | null, onPeer = vi.fn(), spawn = false, onSpawnSettings = vi.fn()) =>
    StatusBar({ guard, peer, spawn, onPeer, onSpawnSettings });

  it("turns the loop counter amber at 75% of the limit", () => {
    const at = (count: number) => renderToStaticMarkup(bar({ count, limit: 12, paused: false }, false));
    expect(at(8)).not.toContain("status-item warn");
    expect(at(9)).toContain("status-item warn");
    expect(at(9)).toContain("Pesan antar agen 9/12");
    expect(renderToStaticMarkup(bar({ count: 15, limit: 20, paused: false }, false))).toContain("status-item warn");
  });

  it("Saling kirim asks for the opposite state", () => {
    const onPeer = vi.fn();
    button(bar(EMPTY.guard, false, onPeer), "Saling kirim: mati").props.onClick?.();
    expect(onPeer).toHaveBeenLastCalledWith(true);
    button(bar(EMPTY.guard, true, onPeer), "Saling kirim: aktif").props.onClick?.();
    expect(onPeer).toHaveBeenLastCalledWith(false);
  });

  it("Agen membuka pane shows the Settings switch and opens Settings › Kolaborasi", () => {
    const open = vi.fn();
    expect(renderToStaticMarkup(bar(EMPTY.guard, null, vi.fn(), true))).toContain("Agen membuka pane: aktif");
    button(bar(EMPTY.guard, null, vi.fn(), false, open), "Agen membuka pane: mati").props.onClick?.();
    expect(open).toHaveBeenCalledOnce();
  });

  it("without a workspace or the core's data, shows the empty state and a disabled switch", () => {
    const html = renderToStaticMarkup(bar(EMPTY.guard, null));
    expect(html).toContain("Saling kirim: mati");
    expect(html).toContain("Pesan antar agen 0/12");
    expect(button(bar(EMPTY.guard, null), "Saling kirim").props.disabled).toBe(true);
  });
});
