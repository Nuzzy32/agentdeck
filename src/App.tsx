import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { homeDir } from "@tauri-apps/api/path";
import "./layout/layout.css";
import { FONT, nextFontSize, shortcut } from "./layout/keys";
import { MESSAGES_BREAKPOINT, panelOpen, type PanelOverride, SIDEBAR_BREAKPOINT, togglePanel } from "./layout/logic";
import type { Pane, Workspace } from "./layout/model";
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

/** Workspaces, split trees of real shell panes, focus and the UI-UX §7 shortcuts (T1.5). */
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
  const area = useRef<Rect | null>(null);
  const ids = useRef({ pane: 1, ws: 1, toast: 1 });
  // One pane creation at a time: a held ⌘D must not spawn shells against a layout that is about to change.
  const creating = useRef(false);

  useEffect(() => {
    homeDir().then(setHome, (e: unknown) => console.error("homeDir", e));
  }, []);

  const active = workspaces.find((ws) => ws.id === activeId) ?? null;
  const cell = useMemo(() => measureCell(fontSize), [fontSize]);
  const sidebarOpen = panelOpen(width, SIDEBAR_BREAKPOINT, sidebar);
  const messagesOpen = active !== null && panelOpen(width, MESSAGES_BREAKPOINT, messages);

  const tilde = (path: string) =>
    home && path === home ? "~" : home && path.startsWith(`${home}/`) ? `~${path.slice(home.length)}` : path;
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
  const createShell = async (folder: string): Promise<Pane | null> => {
    try {
      const ptyId = await invoke<number>("pane_create_shell", { cwd: folder, rows: 24, cols: 80 });
      const pane: Pane = {
        id: `p${ids.current.pane++}`,
        agent: "Shell",
        title: basename(folder),
        folder: tilde(folder),
        status: "unknown",
        pending: 0,
        ptyId,
      };
      setPanes((all) => ({ ...all, [pane.id]: pane }));
      return pane;
    } catch (e) {
      toast(`Shell tidak bisa dijalankan di ${tilde(folder)}: ${String(e)}`);
      return null;
    }
  };

  const once = async (op: () => Promise<void>) => {
    if (creating.current) return;
    creating.current = true;
    try {
      await op();
    } finally {
      creating.current = false;
    }
  };

  // ponytail: new workspaces open in the active workspace's folder (or ~) until the launcher's folder picker (T1.7).
  const newWorkspace = () =>
    once(async () => {
      const folder = active?.folder ?? home;
      if (!folder) return;
      const pane = await createShell(folder);
      if (!pane) return;
      const id = ids.current.ws++;
      setWorkspaces((list) => {
        const base = basename(folder);
        let name = base;
        for (let n = 2; list.some((ws) => ws.name === name); n++) name = `${base} ${n}`;
        return [...list, { id, name, folder, tree: { pane: pane.id }, focused: pane.id, zoomed: null }];
      });
      setActiveId(id);
    });

  const splitPane = (dir: Dir, target = active?.focused) =>
    once(async () => {
      const ws = active;
      if (!ws || !target || !area.current) return;
      const r = rects(ws.tree, area.current).get(target);
      if (!r || !canSplit(r, dir, cell)) {
        toast("Pane terlalu kecil untuk dibagi (minimal 40 kolom × 8 baris).");
        return;
      }
      const pane = await createShell(ws.folder);
      if (!pane) return;
      updateWs(ws.id, (w) => ({ ...w, tree: split(w.tree, target, dir, pane.id), focused: pane.id, zoomed: null }));
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
    serial(pane.ptyId, () => invoke("pane_close", { id: pane.ptyId })).catch((e: unknown) =>
      toast(`${pane.agent} di ${pane.id} tidak bisa dihentikan: ${String(e)}`),
    );
  };

  // ponytail: a shell pane always counts as running (no exit event from the core yet), so ⌘W always asks; skip
  // the dialog for exited panes once T1.7 reports exits.
  const requestClose = (id = active?.focused) => {
    if (id) setClosing(id);
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
      if (closing !== null) return;
      const action = shortcut(e);
      if (!action) return;
      e.preventDefault();
      e.stopPropagation();
      switch (action.type) {
        case "newWorkspace":
          return void newWorkspace();
        case "split":
          return void splitPane(action.dir);
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
            onNewWorkspace={() => void newWorkspace()}
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
                      onSplit: (dir) => void splitPane(dir, id),
                      onZoom: () => toggleZoom(id),
                      onClose: () => requestClose(id),
                    }}
                  />
                ) : null;
              }}
            />
          ) : (
            <EmptyState onStart={() => void newWorkspace()} />
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
      <Toasts toasts={toasts} onDismiss={(id) => setToasts((list) => list.filter((t) => t.id !== id))} />
    </div>
  );
}

export default App;
