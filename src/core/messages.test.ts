import { afterEach, describe, expect, it, vi } from "vitest";
import { type CollabClient, createCollab, EMPTY, type LoopGuard, type MessageRow, pendingByPane } from "./messages";

const row = (id: string, state: MessageRow["state"], to = "p1"): MessageRow => ({
  id,
  ts: 0,
  workspace: "1",
  from: "p2",
  to,
  state,
  reason: null,
  text: id,
});

const flush = () => new Promise((r) => setTimeout(r, 0));

/** A core that has every command; `rows` and `guard` are what it currently holds. */
function fake(init: { rows?: MessageRow[]; guard?: LoopGuard; peer?: boolean } = {}) {
  const core = { rows: init.rows ?? [], guard: init.guard ?? { count: 0, limit: 12, paused: false }, peer: init.peer ?? false };
  let onMessages: ((ws: string) => void) | undefined;
  let onGuard: ((e: LoopGuard & { workspace: string }) => void) | undefined;
  const client = {
    messagesList: vi.fn(async (_ws: string) => core.rows),
    messageDrop: vi.fn(async (_id: string) => {}),
    loopGuardGet: vi.fn(async (_ws: string) => core.guard),
    loopGuardContinue: vi.fn(async (_ws: string) => {}),
    loopGuardStop: vi.fn(async (_ws: string) => {}),
    setPeer: vi.fn(async (_ws: string, _on: boolean) => {}),
    getPeer: vi.fn(async (_ws: string) => core.peer),
    onMessagesChanged: vi.fn(async (cb: (ws: string) => void) => {
      onMessages = cb;
      return () => (onMessages = undefined);
    }),
    onLoopGuard: vi.fn(async (cb: (e: LoopGuard & { workspace: string }) => void) => {
      onGuard = cb;
      return () => (onGuard = undefined);
    }),
  } satisfies CollabClient;
  return {
    core,
    client,
    emitMessages: (ws: string) => onMessages?.(ws),
    emitGuard: (e: LoopGuard & { workspace: string }) => onGuard?.(e),
  };
}

afterEach(() => vi.restoreAllMocks());

describe("loop guard", () => {
  it("the ribbon state follows the loop-guard event, per workspace", async () => {
    const f = fake();
    const c = createCollab(f.client);
    c.start();
    await flush();
    f.emitGuard({ workspace: "1", count: 12, limit: 12, paused: true });
    expect(c.get("1").guard).toEqual({ count: 12, limit: 12, paused: true });
    expect(c.get("2").guard.paused).toBe(false);
    f.emitGuard({ workspace: "1", count: 0, limit: 12, paused: false });
    expect(c.get("1").guard.paused).toBe(false);
  });

  it("Lanjutkan and Hentikan call their own command for that workspace, then reload", async () => {
    const f = fake({ guard: { count: 12, limit: 12, paused: true } });
    const c = createCollab(f.client);
    await c.load("1");
    expect(c.get("1").guard.paused).toBe(true);

    f.core.guard = { count: 0, limit: 12, paused: false };
    await c.continueLoop("1");
    expect(f.client.loopGuardContinue).toHaveBeenCalledWith("1");
    expect(f.client.loopGuardStop).not.toHaveBeenCalled();
    expect(c.get("1").guard.paused).toBe(false);

    await c.stopLoop("1");
    expect(f.client.loopGuardStop).toHaveBeenCalledWith("1");
  });
});

describe("messages", () => {
  it("reloads a followed workspace when messages-changed names it, and only then", async () => {
    const f = fake({ rows: [row("m1", "queued")] });
    const c = createCollab(f.client);
    c.start();
    await c.load("1");
    expect(c.get("1").messages.map((m) => m.id)).toEqual(["m1"]);

    f.core.rows = [row("m1", "delivered"), row("m2", "queued")];
    f.emitMessages("1");
    await flush();
    expect(c.get("1").messages.map((m) => m.state)).toEqual(["delivered", "queued"]);

    f.emitMessages("2");
    await flush();
    expect(f.client.messagesList).toHaveBeenCalledTimes(2);
  });

  it("a slow older reply does not overwrite a newer one", async () => {
    const f = fake();
    let first: (rows: MessageRow[]) => void = () => {};
    f.client.messagesList.mockImplementationOnce(() => new Promise((r) => (first = r)));
    const c = createCollab(f.client);
    c.start();
    const slow = c.load("1");
    await flush();
    f.core.rows = [row("m2", "queued")];
    f.emitMessages("1");
    await flush();
    first([row("m1", "queued")]);
    await slow;
    expect(c.get("1").messages.map((m) => m.id)).toEqual(["m2"]);
  });

  it("Buang drops by id and reloads", async () => {
    const f = fake({ rows: [row("m1", "queued")] });
    const c = createCollab(f.client);
    await c.load("1");
    f.core.rows = [row("m1", "dropped")];
    await c.drop("1", "m1");
    expect(f.client.messageDrop).toHaveBeenCalledWith("m1");
    expect(c.get("1").messages[0].state).toBe("dropped");
  });

  it("counts waiting (queued, paused) messages per target pane for the chip", () => {
    const rows = [row("a", "queued"), row("b", "paused"), row("c", "delivered"), row("d", "dropped"), row("e", "queued", "p3")];
    expect(pendingByPane(rows)).toEqual({ p1: 2, p3: 1 });
  });
});

describe("Saling kirim", () => {
  it("turning on asks first; a no leaves it off", async () => {
    const f = fake();
    const c = createCollab(f.client);
    await c.requestPeer("1", true, async () => false);
    expect(f.client.setPeer).not.toHaveBeenCalled();
    expect(c.get("1").peer).toBe(false);

    await c.requestPeer("1", true, async () => true);
    expect(f.client.setPeer).toHaveBeenCalledWith("1", true);
    expect(c.get("1").peer).toBe(true);
  });

  it("turning off does not ask", async () => {
    const f = fake({ peer: true });
    const c = createCollab(f.client);
    await c.load("1");
    const confirm = vi.fn(async () => true);
    await c.requestPeer("1", false, confirm);
    expect(confirm).not.toHaveBeenCalled();
    expect(f.client.setPeer).toHaveBeenCalledWith("1", false);
    expect(c.get("1").peer).toBe(false);
  });
});

describe("core without these commands", () => {
  it("keeps the empty state, warns once per command, and rejects actions", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const f = fake();
    const missing = (cmd: string) => async () => Promise.reject(`command ${cmd} not found`);
    const client: CollabClient = {
      ...f.client,
      messagesList: missing("messages_list"),
      loopGuardGet: missing("loop_guard_get"),
      getPeer: missing("workspace_get_peer"),
      loopGuardContinue: missing("loop_guard_continue"),
    };
    const c = createCollab(client);
    c.start();
    await c.load("1");
    await c.load("1");
    f.emitMessages("1");
    await flush();
    expect(c.get("1")).toEqual(EMPTY);
    expect(warn).toHaveBeenCalledTimes(3);
    await expect(c.continueLoop("1")).rejects.toContain("loop_guard_continue");
  });

  it("stopping before listen resolves still unsubscribes", async () => {
    const f = fake();
    const c = createCollab(f.client);
    const stop = c.start();
    stop();
    await flush();
    f.emitGuard({ workspace: "1", count: 12, limit: 12, paused: true });
    expect(c.get("1").guard.paused).toBe(false);
  });
});
