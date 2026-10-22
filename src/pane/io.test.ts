import { describe, expect, it, vi } from "vitest";
import { activity, cumulativeAcker, inputClock, QUIET_MS, serial, TYPED_WINDOW_MS, writeQueue } from "./io";

const tick = () => new Promise<void>((r) => setTimeout(r, 0));

describe("cumulativeAcker", () => {
  it("sends one running total per turn, never a delta", async () => {
    const sent: number[] = [];
    const ack = cumulativeAcker((t) => sent.push(t));
    ack(10);
    ack(20);
    ack(5);
    expect(sent).toEqual([]);
    await Promise.resolve();
    expect(sent).toEqual([35]);
    ack(1);
    await Promise.resolve();
    expect(sent).toEqual([35, 36]);
  });
});

describe("inputClock", () => {
  it("counts onData as typing only shortly after a DOM input event", () => {
    let now = 1000;
    const clock = inputClock(() => now);
    expect(clock.typed()).toBe(false); // reply with no input event yet
    clock.mark();
    now += 5;
    expect(clock.typed()).toBe(true); // keystroke, or IME data one task later
    now += TYPED_WINDOW_MS;
    expect(clock.typed()).toBe(false); // DA/DSR reply long after the last keystroke
  });
});

describe("writeQueue", () => {
  it("keeps order, batches while a write is in flight, and keeps the typed mark", async () => {
    const calls: [string, boolean][] = [];
    let release = () => {};
    const write = writeQueue((data, typed) => {
      calls.push([data, typed]);
      return new Promise<void>((r) => (release = r));
    });
    write("a", true);
    write("\x1b[?1;2c", false);
    write("b", true);
    expect(calls).toEqual([["a", true]]);
    release();
    await tick();
    expect(calls).toEqual([
      ["a", true],
      ["\x1b[?1;2cb", true],
    ]);
    release();
    await tick();
    write("\x1b[0n", false);
    expect(calls[2]).toEqual(["\x1b[0n", false]);
  });

  it("goes on after a failed write", async () => {
    const calls: string[] = [];
    const write = writeQueue((data) => {
      calls.push(data);
      return Promise.reject(new Error("no such pane"));
    });
    write("a", true);
    write("b", true);
    await tick();
    expect(calls).toEqual(["a", "b"]);
  });
});

describe("serial", () => {
  it("runs ops for one pane in call order even if an earlier one is slower or fails", async () => {
    const order: string[] = [];
    const slow = serial(1, () => new Promise<void>((r) => setTimeout(() => (order.push("attach"), r()), 20)));
    const failing = serial(1, () => (order.push("detach"), Promise.reject(new Error("x"))));
    const last = serial(1, async () => order.push("attach2"));
    await Promise.allSettled([slow, failing, last]);
    expect(order).toEqual(["attach", "detach", "attach2"]);
  });
});

describe("activity", () => {
  it("reports busy on output and quiet one second after the last chunk", () => {
    vi.useFakeTimers();
    try {
      const seen: boolean[] = [];
      const a = activity((b) => seen.push(b), () => Date.now());
      a.mark();
      vi.advanceTimersByTime(600);
      a.mark(); // keeps it busy without another report
      vi.advanceTimersByTime(600);
      expect(seen).toEqual([true]);
      vi.advanceTimersByTime(QUIET_MS);
      expect(seen).toEqual([true, false]);
      a.mark();
      a.dispose();
      expect(seen).toEqual([true, false, true, false]);
      vi.advanceTimersByTime(5 * QUIET_MS);
      expect(seen).toHaveLength(4);
    } finally {
      vi.useRealTimers();
    }
  });
});
