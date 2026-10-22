import { describe, expect, it } from "vitest";
import { nearLimit, panelOpen, SIDEBAR_BREAKPOINT as BP, splitMiddle, togglePanel } from "./logic";

describe("panelOpen", () => {
  it("follows the window width without an override", () => {
    expect(panelOpen(1440, BP, null)).toBe(true);
    expect(panelOpen(BP, BP, null)).toBe(true);
    expect(panelOpen(900, BP, null)).toBe(false);
  });

  it("keeps a manual toggle on the same side of the breakpoint", () => {
    const hidden = togglePanel(1440, BP, null);
    expect(panelOpen(1440, BP, hidden)).toBe(false);
    expect(panelOpen(1200, BP, hidden)).toBe(false);
    const shown = togglePanel(1440, BP, hidden);
    expect(panelOpen(1440, BP, shown)).toBe(true);
  });

  it("returns to automatic once the window crosses the breakpoint", () => {
    const shownWhileNarrow = togglePanel(900, BP, null);
    expect(panelOpen(900, BP, shownWhileNarrow)).toBe(true);
    expect(panelOpen(1440, BP, shownWhileNarrow)).toBe(true);
    expect(panelOpen(900, BP, togglePanel(1440, BP, shownWhileNarrow))).toBe(false);
  });
});

describe("splitMiddle", () => {
  it("keeps the last segment visible", () => {
    expect(splitMiddle("~/dev/app")).toEqual({ head: "~/dev", tail: "/app" });
    expect(splitMiddle("~/dev/app/")).toEqual({ head: "~/dev", tail: "/app/" });
  });

  it("does not split paths without an inner separator", () => {
    expect(splitMiddle("~")).toEqual({ head: "", tail: "~" });
    expect(splitMiddle("/usr")).toEqual({ head: "", tail: "/usr" });
  });

  it("caps a long last segment at 16 characters", () => {
    const path = "~/x/a-very-long-repository-name";
    const { head, tail } = splitMiddle(path);
    expect(tail).toHaveLength(16);
    expect(head + tail).toBe(path);
  });
});

describe("nearLimit", () => {
  it("turns on at 75%", () => {
    expect(nearLimit(8, 12)).toBe(false);
    expect(nearLimit(9, 12)).toBe(true);
    expect(nearLimit(0, 0)).toBe(false);
  });
});
