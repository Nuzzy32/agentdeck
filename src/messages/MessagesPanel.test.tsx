import { isValidElement, type ReactNode } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it, vi } from "vitest";
import type { MessageState } from "../core/messages";
import { clock, MessageDetail, MessagesPanel, type PanelMessage, visible, waited } from "./MessagesPanel";

const msg = (id: string, state: MessageState, text = `text ${id}`): PanelMessage => ({
  id,
  ts: Date.parse("2026-10-22T14:09:03.120Z"),
  from: "p2",
  to: "p1",
  state,
  reason: state === "dropped" ? "target agent exited" : null,
  text,
  held: false,
  wait: null,
});

const ALL = [msg("m1", "delivered"), msg("m2", "queued"), msg("m3", "paused"), msg("m4", "dropped")];
const noop = () => {};
const panel = (messages: PanelMessage[]) =>
  renderToStaticMarkup(
    <MessagesPanel
      title="api"
      messages={messages}
      panes={[{ id: "p2", agent: "codex", title: "review: auth" }]}
      now={Date.parse("2026-10-22T14:12:30Z")}
      onCopy={noop}
      onDiscard={noop}
      onForce={noop}
    />,
  );

describe("filter", () => {
  it("Semua keeps every message in order; Menunggu keeps queued and paused", () => {
    expect(visible(ALL, "all")).toBe(ALL);
    expect(visible(ALL, "waiting").map((m) => m.id)).toEqual(["m2", "m3"]);
  });
});

describe("panel", () => {
  it("shows the empty state when the workspace has no messages", () => {
    const html = panel([]);
    expect(html).toContain("Belum ada pesan antar agen di workspace ini. Pesan muncul saat agen memakai pane_send.");
    expect(html).not.toContain("<ul");
  });

  it("renders a row per message with status text, waiting time, chips and first line", () => {
    const html = panel([...ALL.slice(0, 3), msg("m4", "dropped", "line one\nline two")]);
    for (const text of ["terkirim", "menunggu 3 mnt", "dijeda", "dibuang", "line one"]) expect(html).toContain(text);
    expect(html).not.toContain("line two");
    expect(html).toContain('title="p2 · codex: review: auth"');
    expect(html).toContain('aria-pressed="true">Semua');
    expect(html.match(/aria-expanded="false"/g)).toHaveLength(4);
    expect(html).not.toContain("tertahan");
  });

  it("flags held rows with the chip", () => {
    const html = panel([{ ...msg("m2", "queued"), held: true }, msg("m3", "queued")]);
    expect(html.match(/tertahan &gt; 10 menit/g)).toHaveLength(1);
  });
});

type Props = { children?: ReactNode; role?: string; onClick?: () => void; onKeyDown?: (e: { key: string }) => void };

/** Props of the first element in a tree that matches (MessageDetail has no hooks, so it can be called directly). */
function find(node: ReactNode, hit: (type: unknown, p: Props) => boolean): Props | undefined {
  if (Array.isArray(node)) {
    for (const n of node) {
      const found = find(n, hit);
      if (found) return found;
    }
    return undefined;
  }
  if (!isValidElement<Props>(node)) return undefined;
  return hit(node.type, node.props) ? node.props : find(node.props.children, hit);
}
const button = (node: ReactNode, label: string) => find(node, (t, p) => t === "button" && p.children === label);

describe("detail", () => {
  const props = (m: PanelMessage, confirming = false) => ({
    id: "d",
    m,
    confirming,
    onCopy: vi.fn(),
    onDiscard: vi.fn(),
    onAskForce: vi.fn(),
    onForce: vi.fn(),
  });
  const detail = (m: PanelMessage, confirming = false) =>
    renderToStaticMarkup(<MessageDetail {...props(m, confirming)} />);

  it("offers Buang only for queued and paused; Salin always", () => {
    const withDiscard = ALL.filter((m) => detail(m).includes(">Buang<")).map((m) => m.state);
    expect(withDiscard).toEqual(["queued", "paused"]);
    for (const m of ALL) expect(detail(m)).toContain(">Salin<");
  });

  it("offers Kirim sekarang only for queued; paused waits for Lanjutkan", () => {
    const withForce = ALL.filter((m) => detail(m).includes(">Kirim sekarang<")).map((m) => m.state);
    expect(withForce).toEqual(["queued"]);
    expect(detail(msg("m3", "paused"), true)).not.toContain("Kirim");
  });

  it("Kirim sekarang asks first; only Kirim forces, Batal and Escape do not", () => {
    const p = props(msg("m2", "queued"));
    button(MessageDetail(p), "Kirim sekarang")?.onClick?.();
    expect(p.onAskForce).toHaveBeenCalledWith("m2");
    expect(p.onForce).not.toHaveBeenCalled();

    const asking = props(msg("m2", "queued"), true);
    const html = renderToStaticMarkup(<MessageDetail {...asking} />);
    expect(html).toContain("Teks yang sedang diketik di sana bisa ikut terkirim.");
    expect(html).not.toContain(">Kirim sekarang<");
    expect(html).not.toContain(">Buang<");
    const tree = MessageDetail(asking);
    button(tree, "Batal")?.onClick?.();
    expect(asking.onAskForce).toHaveBeenLastCalledWith(null);
    expect(asking.onForce).not.toHaveBeenCalled();
    find(tree, (_, p) => p.role === "group")?.onKeyDown?.({ key: "Escape" });
    expect(asking.onAskForce).toHaveBeenCalledTimes(2);
    expect(asking.onForce).not.toHaveBeenCalled();
    button(tree, "Kirim")?.onClick?.();
    expect(asking.onForce).toHaveBeenCalledWith("m2");
  });

  it("shows the full text and the drop reason", () => {
    const html = detail(msg("m4", "dropped", "a\nb <c>"));
    expect(html).toContain("a\nb &lt;c&gt;");
    expect(html).toContain("Alasan: target agent exited");
  });
});

describe("formatting", () => {
  it("waited", () => {
    const t = Date.parse("2026-10-22T14:00:00Z");
    expect(waited(t, Date.parse("2026-10-22T14:00:12Z"))).toBe("12 dtk");
    expect(waited(t, Date.parse("2026-10-22T14:03:59Z"))).toBe("3 mnt");
    expect(waited(t, Date.parse("2026-10-22T15:05:00Z"))).toBe("1 j 5 mnt");
    expect(waited(t, Date.parse("2026-10-22T13:00:00Z"))).toBe("0 dtk");
  });

  it("clock is local HH:MM", () => {
    const d = new Date(2026, 9, 22, 9, 5);
    expect(clock(d.getTime())).toBe("09:05");
    expect(clock(Number.NaN)).toBe("--:--");
  });
});
