import { useEffect, useMemo, useRef, useState } from "react";
import { Channel, invoke } from "@tauri-apps/api/core";
import { homeDir } from "@tauri-apps/api/path";
import "./layout/layout.css";
import { FONT, nextFontSize, shortcut } from "./layout/keys";
import { MESSAGES_BREAKPOINT, panelOpen, type PanelOverride, SIDEBAR_BREAKPOINT, togglePanel } from "./layout/logic";
import {
  type AgentInfo,
  applyActivity,
  applyEvent,
  type LaunchError,
  type Pane,
  type PaneEvent,
  paneTitle,
  pushRecent,
  STARTING_MS,
  tildePath,
  type Workspace,
} from "./layout/model";
import { PaneView } from "./layout/Pane";
import {
  CloseDialog,
  EmptyState,
  MessagesPanel,
  Sidebar,
  StatusBar,
  TitleBar,
  type Toast,
  Toasts,
  type WorkspaceView,
} from "./layout/Shell";
import { SplitView } from "./layout/SplitView";
import { canSplit, type Dir, leaves, neighbor, type Rect, rects, remove, setRatio, type Side, split } from "./layout/tree";
import { Launcher, SHELL } from "./launcher/Launcher";
import { serial } from "./pane/io";
import { measureCell } from "./pane/TerminalPane";

function useWindowWidth(): number {
  const [width, setWidth] = useState(window.innerWidth);
  useEffect(() => {
    const onResize = () => setWidth(window.innerWidth);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);
  return width;
}

const TOAST_MS = 4000;

function basename(path: string): string {
  return path.split("/").filter(Boolean).pop() ?? "/";
}

/** Where the launcher's pane goes: a new workspace, or a split of `target` in workspace `ws`. */
type LauncherTarget = { kind: "workspace" } | { kind: "split"; dir: Dir; target: string; ws: number };

const LAUNCHER_TITLE = { workspace: "Workspace baru", row: "Split kanan", col: "Split bawah" };

// The last five launch folders are a per-machine convenience, so browser storage is enough.
const RECENTS_KEY = "agentdeck.recentFolders";

function loadRecents(): string[] {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(RECENTS_KEY) ?? "[]");
    return Array.isArray(v) ? v.filter((f): f is string => typeof f === "string").slice(0, 5) : [];
  } catch {
    return [];
  }
}

function saveRecents(list: string[]) {
  try {
    localStorage.setItem(RECENTS_KEY, JSON.stringify(list));
  } catch {
    // Storage off: the list just does not survive a restart.
  }
}

/** `pane_create` rejects with a `LaunchError`; anything else (IPC failure) becomes one. */
function toLaunchError(e: unknown): LaunchError {
  if (typeof e === "object" && e !== null && "message" in e && typeof e.message === "string") {
    return { message: e.message, install: "install" in e && typeof e.install === "string" ? e.install : null };
  }
  return { message: String(e), install: null };
}

/** Workspaces, split trees of shell and agent panes, launcher, status, and the UI-UX §7 shortcuts (T1.5, T1.7). */
function App() {
  const width = useWindowWidth();
  const [home, setHome] = useState<string | null>(null);
  const [panes, setPanes] = useState<Record<string, Pane>>({});
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [activeId, setActiveId] = useState<number | null>(null);
  const [closing, setClosing] = useState<string | null>(null);
  const [toasts, setToasts] = useState<Toast[]>([]);
  const [fontSize, setFontSize] = useState(FONT.base);
  const [sidebar, setSidebar] = useState<PanelOverride>(null);
  const [messages, setMessages] = useState<PanelOverride>(null);
  const [agents, setAgents] = useState<AgentInfo[] | null>(null);
  const [launcher, setLauncher] = useState<LauncherTarget | null>(null);
  // The launcher preselects what was launched last.
  const [choice, setChoice] = useState(SHELL);
  const [recents, setRecents] = useState(loadRecents);
  const area = useRef<Rect | null>(null);
  const ids = useRef({ pane: 1, ws: 1, toast: 1, spawn: 1 });
  // One pane creation at a time: a held ⌘D must not spawn shells against a layout that is about to change.
  const creating = useRef(false);

  const refreshAgents = () =>
    invoke<AgentInfo[]>("agents_list").then(setAgents, (e: unknown) => toast(`Daftar agen tidak terbaca: ${String(e)}`));

  useEffect(() => {
    homeDir().then(setHome, (e: unknown) => console.error("homeDir", e));
    void refreshAgents();
  }, []);

  const active = workspaces.find((ws) => ws.id === activeId) ?? null;
  const cell = useMemo(() => measureCell(fontSize), [fontSize]);
  const sidebarOpen = panelOpen(width, SIDEBAR_BREAKPOINT, sidebar);
  const messagesOpen = active !== null && panelOpen(width, MESSAGES_BREAKPOINT, messages);

  const tilde = (path: string) => tildePath(path, home);
  const views: WorkspaceView[] = workspaces.map((ws) => ({
    id: ws.id,
    name: ws.name,
    folder: tilde(ws.folder),
    panes: leaves(ws.tree).flatMap((id) => panes[id] ?? []),
  }));
  const activeView = views.find((v) => v.id === activeId) ?? null;

  const toast = (text: string) => {
    const id = ids.current.toast++;
    setToasts((list) => [...list, { id, text }].slice(-3));
    setTimeout(() => setToasts((list) => list.filter((t) => t.id !== id)), TOAST_MS);
  };

  const updateWs = (id: number, fn: (ws: Workspace) => Workspace) =>
    setWorkspaces((list) => list.map((ws) => (ws.id === id ? fn(ws) : ws)));

  // The PTY is created here, in an event handler, not in a TerminalPane effect that StrictMode would run twice.
  // Events are routed by pane id and spawn number, so a restarted pane ignores whatever its old process still sends.
  const startPty = async (base: Pane, task: string): Promise<LaunchError | null> => {
    const spawn = ids.current.spawn++;
    const pane: Pane = {
      ...base,
      spawn,
      ptyId: null,
      status: base.agentId ? "starting" : "unknown",
      official: false,
      startedAt: Date.now(),
      kind: undefined,
      msg: undefined,
      error: false,
      exitCode: undefined,
      failed: undefined,
    };
    const update = (fn: (p: Pane) => Pane) =>
      setPanes((all) => {
        const p = all[pane.id];
        return p?.spawn === spawn ? { ...all, [pane.id]: fn(p) } : all;
      });
    // In state before the invoke, so an event that beats the reply (fast exit) still lands.
    setPanes((all) => ({ ...all, [pane.id]: pane }));
    const ch = new Channel<PaneEvent>();
    ch.onmessage = (ev) => update((p) => applyEvent(p, ev));
    try {
      const ptyId = await invoke<number>("pane_create", {
        agent: pane.agentId,
        cwd: pane.cwd,
        task: task || null,
        rows: 24,
        cols: 80,
        onEvent: ch,
      });
      update((p) => ({ ...p, ptyId }));
      // An agent that never reports leaves `starting` for the output heuristic.
      if (pane.agentId) {
        setTimeout(() => update((p) => (p.status === "starting" ? applyActivity(p, false, Date.now()) : p)), STARTING_MS);
      }
      return null;
    } catch (e) {
      const failed = toLaunchError(e);
      update((p) => ({ ...p, failed }));
      return failed;
    }
  };

  const agentName = (id: string | null) => (id ? (agents?.find((a) => a.id === id)?.name ?? id) : "Shell");

  /** Launcher submit: opens the pane, then places it. On error nothing is placed and the launcher shows why. */
  const launch = async (pick: string, cwd: string, task: string): Promise<LaunchError | null> => {
    const target = launcher;
    if (!target || creating.current) return null;
    creating.current = true;
    try {
      const agentId = pick === SHELL ? null : pick;
      const n = ids.current.pane++;
      const id = `p${n}`;
      const err = await startPty(
        {
          id,
          agentId,
          agent: agentName(agentId),
          title: paneTitle(agentId ? task : "", cwd),
          folder: tilde(cwd),
          cwd,
          status: "unknown",
          official: false,
          startedAt: 0,
          pending: 0,
          ptyId: null,
          spawn: 0,
        },
        task,
      );
      if (err) {
        setPanes(({ [id]: _, ...others }) => others);
        // Nothing else creates panes meanwhile (`creating`), so the id can be reused.
        if (ids.current.pane === n + 1) ids.current.pane = n;
        return err;
      }
      if (target.kind === "workspace") {
        const wsId = ids.current.ws++;
        setWorkspaces((list) => {
          const base = basename(cwd);
          let name = base;
          for (let k = 2; list.some((ws) => ws.name === name); k++) name = `${base} ${k}`;
          return [...list, { id: wsId, name, folder: cwd, tree: { pane: id }, focused: id, zoomed: null }];
        });
        setActiveId(wsId);
      } else {
        updateWs(target.ws, (w) =>
          leaves(w.tree).includes(target.target)
            ? { ...w, tree: split(w.tree, target.target, target.dir, id), focused: id, zoomed: null }
            : w,
        );
      }
      const next = pushRecent(recents, cwd);
      setRecents(next);
      saveRecents(next);
      setChoice(pick);
      setLauncher(null);
      return null;
    } finally {
      creating.current = false;
    }
  };

  const openLauncher = (target: LauncherTarget, pick?: string) => {
    if (pick) setChoice(pick);
    void refreshAgents();
    setLauncher(target);
  };

  const newWorkspace = (pick?: string) => openLauncher({ kind: "workspace" }, pick);

  const splitPane = (dir: Dir, target = active?.focused) => {
    const ws = active;
    if (!ws || !target || !area.current) return;
    const r = rects(ws.tree, area.current).get(target);
    if (!r || !canSplit(r, dir, cell)) {
      toast("Pane terlalu kecil untuk dibagi (minimal 40 kolom × 8 baris).");
      return;
    }
    openLauncher({ kind: "split", dir, target, ws: ws.id });
  };

  /** Exited or failed pane: the same program again (without the first task), or a shell in its folder. */
  const restart = (id: string, asShell: boolean) => {
    const pane = panes[id];
    if (!pane) return;
    const old = pane.ptyId;
    if (old !== null) serial(old, () => invoke("pane_close", { id: old })).catch(() => undefined);
    void startPty(asShell ? { ...pane, agentId: null, agent: "Shell", title: basename(pane.cwd) } : pane, "");
  };

  const setActivity = (id: string, busy: boolean) =>
    setPanes((all) => {
      const p = all[id];
      const next = p && applyActivity(p, busy, Date.now());
      return next && next !== p ? { ...all, [id]: next } : all;
    });

  const closePane = (id: string) => {
    setClosing(null);
    const ws = workspaces.find((w) => leaves(w.tree).includes(id));
    const pane = panes[id];
    if (!ws || !pane) return;
    const before = leaves(ws.tree);
    const tree = remove(ws.tree, id);
    if (tree) {
      const after = leaves(tree);
      const focused = ws.focused === id ? after[Math.min(before.indexOf(id), after.length - 1)] : ws.focused;
      updateWs(ws.id, (w) => ({ ...w, tree, focused, zoomed: w.zoomed === id ? null : w.zoomed }));
    } else {
      // Last pane: the workspace goes too, and the next one (or the previous) becomes active.
      const i = workspaces.indexOf(ws);
      const rest = workspaces.filter((w) => w !== ws);
      setWorkspaces(rest);
      if (activeId === ws.id) setActiveId(rest[Math.min(i, rest.length - 1)]?.id ?? null);
    }
    setPanes(({ [id]: _, ...others }) => others);
    const ptyId = pane.ptyId;
    if (ptyId === null) return;
    serial(ptyId, () => invoke("pane_close", { id: ptyId })).catch((e: unknown) =>
      toast(`${pane.agent} di ${pane.id} tidak bisa dihentikan: ${String(e)}`),
    );
  };

  // Nothing left to stop in an exited or failed pane, so it closes without asking.
  const requestClose = (id = active?.focused) => {
    if (!id) return;
    const pane = panes[id];
    if (pane && (pane.status === "exited" || pane.failed)) closePane(id);
    else setClosing(id);
  };

  const focusPane = (id: string) => {
    if (!active) return;
    updateWs(active.id, (w) => ({ ...w, focused: id, zoomed: w.zoomed !== null && w.zoomed !== id ? null : w.zoomed }));
  };

  const moveFocus = (side: Side) => {
    if (!active || active.zoomed) return;
    const next = neighbor(active.tree, active.focused, side);
    if (next) focusPane(next);
  };

  const toggleZoom = (id = active?.focused) => {
    if (!active || !id) return;
    updateWs(active.id, (w) => ({ ...w, focused: id, zoomed: w.zoomed === id ? null : id }));
  };

  const selectWorkspace = (index: number) => {
    const ws = workspaces[index];
    if (ws) setActiveId(ws.id);
  };

  const stepWorkspace = (delta: 1 | -1) => {
    if (workspaces.length === 0) return;
    const i = workspaces.findIndex((ws) => ws.id === activeId);
    selectWorkspace((i + delta + workspaces.length) % workspaces.length);
  };

  const toggleSidebar = () => setSidebar(togglePanel(width, SIDEBAR_BREAKPOINT, sidebar));
  const toggleMessages = () => setMessages(togglePanel(width, MESSAGES_BREAKPOINT, messages));

  useEffect(() => {
    // Capture phase, so a shortcut never reaches xterm.js; unmapped keys (⌘C, ⌘V, Ctrl, ⌥) pass through untouched.
    const onKey = (e: KeyboardEvent) => {
      if (closing !== null || launcher !== null) return;
      const action = shortcut(e);
      if (!action) return;
      e.preventDefault();
      e.stopPropagation();
      switch (action.type) {
        case "newWorkspace":
          return newWorkspace();
        case "split":
          return splitPane(action.dir);
        case "close":
          return requestClose();
        case "focus":
          return moveFocus(action.side);
        case "workspace":
          return selectWorkspace(action.index);
        case "workspaceStep":
          return stepWorkspace(action.delta);
        case "zoom":
          return toggleZoom();
        case "sidebar":
          return toggleSidebar();
        case "messages":
          return toggleMessages();
        case "font":
          return setFontSize((size) => nextFontSize(size, action.delta));
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  });

  const closingPane = closing ? panes[closing] : undefined;

  return (
    <div className="app">
      <TitleBar
        workspace={activeView}
        sidebarOpen={sidebarOpen}
        messagesOpen={messagesOpen}
        onToggleSidebar={toggleSidebar}
        onToggleMessages={toggleMessages}
      />
      <div className="main">
        {sidebarOpen ? (
          <Sidebar
            workspaces={views}
            active={activeView}
            focusedPane={active?.focused ?? null}
            onNewWorkspace={() => newWorkspace()}
            onSelectWorkspace={setActiveId}
            onFocusPane={focusPane}
          />
        ) : null}
        <main className="workspace">
          {active ? (
            // Keyed by workspace: switching unmounts the other workspace's panes, which detaches (not closes) them;
            // coming back attaches again and the core replays their buffer.
            <SplitView
              key={active.id}
              tree={active.tree}
              zoomed={active.zoomed}
              cell={cell}
              onSize={(r) => {
                area.current = r;
              }}
              onRatio={(path, ratio) => updateWs(active.id, (w) => ({ ...w, tree: setRatio(w.tree, path, ratio) }))}
              renderPane={(id) => {
                const pane = panes[id];
                return pane ? (
                  <PaneView
                    pane={pane}
                    focused={id === active.focused}
                    zoomed={id === active.zoomed}
                    fontSize={fontSize}
                    actions={{
                      onFocus: () => focusPane(id),
                      onSplit: (dir) => splitPane(dir, id),
                      onZoom: () => toggleZoom(id),
                      onClose: () => requestClose(id),
                      onActivity: (busy) => setActivity(id, busy),
                      onRestart: () => restart(id, false),
                      onShellHere: () => restart(id, true),
                    }}
                  />
                ) : null;
              }}
            />
          ) : (
            <EmptyState agents={agents} onStart={newWorkspace} />
          )}
        </main>
        {activeView && messagesOpen ? <MessagesPanel workspace={activeView} messages={[]} /> : null}
      </div>
      <StatusBar loopCount={0} />
      {closingPane ? (
        <CloseDialog
          pane={closingPane}
          childCount={Object.values(panes).filter((p) => p.parent === closingPane.id).length}
          onConfirm={() => closePane(closingPane.id)}
          onCancel={() => setClosing(null)}
        />
      ) : null}
      {launcher ? (
        <Launcher
          title={LAUNCHER_TITLE[launcher.kind === "workspace" ? "workspace" : launcher.dir]}
          agents={agents}
          initial={choice}
          folder={
            (launcher.kind === "split" ? panes[launcher.target]?.cwd : undefined) ??
            (active ? (panes[active.focused]?.cwd ?? active.folder) : (home ?? ""))
          }
          recents={recents}
          home={home}
          onLaunch={launch}
          onClose={() => setLauncher(null)}
        />
      ) : null}
      <Toasts toasts={toasts} onDismiss={(id) => setToasts((list) => list.filter((t) => t.id !== id))} />
    </div>
  );
}

export default App;
