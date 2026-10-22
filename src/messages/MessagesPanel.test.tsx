import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
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
  });
});

describe("detail", () => {
  const detail = (m: PanelMessage) =>
    renderToStaticMarkup(<MessageDetail id="d" m={m} onCopy={noop} onDiscard={noop} />);

  it("offers Buang only for queued and paused; Salin always", () => {
    const withDiscard = ALL.filter((m) => detail(m).includes(">Buang<")).map((m) => m.state);
    expect(withDiscard).toEqual(["queued", "paused"]);
    for (const m of ALL) expect(detail(m)).toContain(">Salin<");
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
