// Messages, loop guard and the "Saling kirim" switch of a workspace (T2.8, T2.9). The core owns the queue and the
// guard; this module only reads them and forwards the person's choices. `CollabClient` is the one seam to the core,
// so tests swap in a fake. A command the core does not have yet leaves the workspace in its empty state.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { LOOP_LIMIT } from "../layout/model";

export type MessageState = "queued" | "delivered" | "paused" | "dropped";

/** One message as `messages_list` returns it. */
export type MessageRow = {
  id: string;
  /** Milliseconds since the epoch. */
  ts: number;
  workspace: string;
  from: string;
  to: string;
  state: MessageState;
  reason: string | null;
  text: string;
  /** Queued or paused for 10 minutes or more (MSG-07). */
  held: boolean;
};

export type LoopGuard = { count: number; limit: number; paused: boolean };

export type CollabClient = {
  messagesList(workspace: string): Promise<MessageRow[]>;
  messageDrop(id: string): Promise<void>;
  /** "Kirim sekarang"; rejects while the loop guard holds the message. */
  messageForce(id: string): Promise<void>;
  loopGuardGet(workspace: string): Promise<LoopGuard>;
  loopGuardContinue(workspace: string): Promise<void>;
  loopGuardStop(workspace: string): Promise<void>;
  setPeer(workspace: string, on: boolean): Promise<void>;
  getPeer(workspace: string): Promise<boolean>;
  /** Each resolves to an unsubscribe function. */
  onMessagesChanged(cb: (workspace: string) => void): Promise<() => void>;
  onLoopGuard(cb: (e: LoopGuard & { workspace: string }) => void): Promise<() => void>;
};

export const tauriClient: CollabClient = {
  messagesList: (workspace) => invoke("messages_list", { workspace }),
  messageDrop: (id) => invoke("message_drop", { id }),
  messageForce: (id) => invoke("message_force", { id }),
  loopGuardGet: (workspace) => invoke("loop_guard_get", { workspace }),
  loopGuardContinue: (workspace) => invoke("loop_guard_continue", { workspace }),
  loopGuardStop: (workspace) => invoke("loop_guard_stop", { workspace }),
  setPeer: (workspace, on) => invoke("workspace_set_peer", { workspace, on }),
  getPeer: (workspace) => invoke("workspace_get_peer", { workspace }),
  onMessagesChanged: (cb) => listen<{ workspace: string }>("agentdeck://messages-changed", (e) => cb(e.payload.workspace)),
  onLoopGuard: (cb) => listen<LoopGuard & { workspace: string }>("agentdeck://loop-guard", (e) => cb(e.payload)),
};

export type WorkspaceCollab = { messages: MessageRow[]; guard: LoopGuard; peer: boolean };

export const EMPTY: WorkspaceCollab = {
  messages: [],
  guard: { count: 0, limit: LOOP_LIMIT, paused: false },
  peer: false,
};

/** Messages the person can still discard; they also make up the "N pesan" chip of the target pane. */
export const isWaiting = (m: Pick<MessageRow, "state">) => m.state === "queued" || m.state === "paused";

/** Waiting messages per target pane. */
export function pendingByPane(messages: MessageRow[]): Record<string, number> {
  const out: Record<string, number> = {};
  for (const m of messages) if (isWaiting(m)) out[m.to] = (out[m.to] ?? 0) + 1;
  return out;
}

export type Collab = ReturnType<typeof createCollab>;

/** `held` turns on with time alone, without a messages-changed event: workspaces with waiting messages reload this often. */
export const HELD_REFRESH_MS = 30_000;

/**
 * Per-workspace state for `useSyncExternalStore`: snapshots are replaced, never mutated. Reads that fail (command not
 * there yet) keep the empty state and warn once per command; actions reject so the caller can say why.
 */
export function createCollab(client: CollabClient) {
  const state = new Map<string, WorkspaceCollab>();
  const listeners = new Set<() => void>();
  const warned = new Set<string>();
  // Latest messages request per workspace, so a slow older reply cannot overwrite a newer one.
  const seq = new Map<string, number>();

  const get = (ws: string) => state.get(ws) ?? EMPTY;
  const set = (ws: string, patch: Partial<WorkspaceCollab>) => {
    state.set(ws, { ...get(ws), ...patch });
    for (const l of listeners) l();
  };
  const quiet =
    (cmd: string) =>
    (e: unknown): undefined => {
      if (warned.has(cmd)) return;
      warned.add(cmd);
      console.warn(`agentdeck: ${cmd} unavailable:`, e);
    };

  const loadMessages = async (ws: string) => {
    const n = (seq.get(ws) ?? 0) + 1;
    seq.set(ws, n);
    const messages = await client.messagesList(ws).catch(quiet("messages_list"));
    if (messages && seq.get(ws) === n) set(ws, { messages });
  };
  const loadGuard = async (ws: string) => {
    const guard = await client.loopGuardGet(ws).catch(quiet("loop_guard_get"));
    if (guard) set(ws, { guard });
  };

  /** Everything for one workspace; also marks it as one whose message events are followed. */
  const load = async (ws: string) => {
    if (!state.has(ws)) state.set(ws, EMPTY);
    const peer = client.getPeer(ws).then(
      (on) => set(ws, { peer: on }),
      (e: unknown) => quiet("workspace_get_peer")(e),
    );
    await Promise.all([loadMessages(ws), loadGuard(ws), peer]);
  };

  return {
    get,
    load,
    subscribe(l: () => void) {
      listeners.add(l);
      return () => void listeners.delete(l);
    },
    /** Follows the core's events until the returned function is called (safe while `listen` is still pending). */
    start(): () => void {
      let stopped = false;
      const offs: (() => void)[] = [];
      const keep = (p: Promise<() => void>) =>
        p.then(
          (off) => (stopped ? off() : offs.push(off)),
          (e: unknown) => quiet("listen")(e),
        );
      keep(client.onMessagesChanged((ws) => void (state.has(ws) && loadMessages(ws))));
      keep(client.onLoopGuard(({ workspace, count, limit, paused }) => set(workspace, { guard: { count, limit, paused } })));
      const timer = setInterval(() => {
        for (const [ws, s] of state) if (s.messages.some(isWaiting)) void loadMessages(ws);
      }, HELD_REFRESH_MS);
      return () => {
        stopped = true;
        clearInterval(timer);
        for (const off of offs.splice(0)) off();
      };
    },
    async continueLoop(ws: string) {
      await client.loopGuardContinue(ws);
      await load(ws);
    },
    async stopLoop(ws: string) {
      await client.loopGuardStop(ws);
      await load(ws);
    },
    async drop(ws: string, id: string) {
      await client.messageDrop(id);
      await loadMessages(ws);
    },
    /** "Kirim sekarang" (the panel already asked the person). */
    async force(ws: string, id: string) {
      await client.messageForce(id);
      await loadMessages(ws);
    },
    /** Turning it on asks first (`confirm`); turning it off does not. */
    async requestPeer(ws: string, on: boolean, confirm: () => Promise<boolean>) {
      if (on && !(await confirm())) return;
      await client.setPeer(ws, on);
      set(ws, { peer: on });
    },
  };
}
