import { describe, expect, it } from "vitest";
import { findLabel } from "./FindBar";

describe("findLabel (TERM-03)", () => {
  it("counts from 1, says when nothing matched, and shows ? past the highlight limit", () => {
    expect(findLabel(null)).toBe("");
    expect(findLabel({ index: 2, count: 12 })).toBe("3/12");
    expect(findLabel({ index: 0, count: 0 })).toBe("Tidak ada");
    expect(findLabel({ index: -1, count: 1000 })).toBe("?/1000");
  });
});
