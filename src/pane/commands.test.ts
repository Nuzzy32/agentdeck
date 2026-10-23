import { describe, expect, it } from "vitest";
import { parse133, step, tracker, type Mark } from "./commands";

describe("parse133", () => {
  it("reads the four marks and the exit code", () => {
    expect(parse133("A")).toEqual({ kind: "A" });
    expect(parse133("A;aid=7")).toEqual({ kind: "A" });
    expect(parse133("B")).toEqual({ kind: "B" });
    expect(parse133("C")).toEqual({ kind: "C" });
    expect(parse133("D;0")).toEqual({ kind: "D", exit: 0 });
    expect(parse133("D;130")).toEqual({ kind: "D", exit: 130 });
    expect(parse133("D")).toEqual({ kind: "D", exit: null });
    expect(parse133("D;-1")).toEqual({ kind: "D", exit: null });
    expect(parse133("P;k=i")).toBeNull();
    expect(parse133("")).toBeNull();
  });
});

describe("tracker", () => {
  const run = (marks: string) => {
    const log: string[] = [];
    let n = 0;
    const feed = tracker<number>({
      mark: () => ++n,
      drop: (m) => log.push(`drop ${m}`),
      start: (m) => log.push(`start ${m}`),
      finish: (m, exit) => log.push(`finish ${m} ${exit}`),
    });
    for (const m of marks.split(" ")) feed(parse133(m) as Mark);
    return log;
  };

  it("finishes each command on its own prompt marker", () => {
    expect(run("A B C D;1 A B C D;0 A B")).toEqual(["start 1", "finish 1 1", "start 2", "finish 2 0"]);
  });

  it("drops prompts where nothing ran", () => {
    expect(run("A B A B C D;0 A")).toEqual(["drop 1", "start 2", "finish 2 0"]);
  });

  it("ignores D without a command and repeated C", () => {
    expect(run("D;0 A D;2 C C D;3 D;4")).toEqual(["start 1", "finish 1 3"]);
  });

  it("marks the current line on C without A", () => {
    expect(run("C D;0")).toEqual(["start 1", "finish 1 0"]);
  });
});

describe("step", () => {
  const lines = [3, 40, 12];
  it("finds the nearest command line above or below", () => {
    expect(step(lines, 20, -1)).toBe(12);
    expect(step(lines, 12, -1)).toBe(3);
    expect(step(lines, 3, -1)).toBeUndefined();
    expect(step(lines, 12, 1)).toBe(40);
    expect(step(lines, 40, 1)).toBeUndefined();
    expect(step([], 0, 1)).toBeUndefined();
  });
});
