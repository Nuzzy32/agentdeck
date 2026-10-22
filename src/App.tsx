import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { homeDir } from "@tauri-apps/api/path";
import "./layout/layout.css";
import { MESSAGES_BREAKPOINT, panelOpen, type PanelOverride, SIDEBAR_BREAKPOINT, togglePanel } from "./layout/logic";
import { PaneView } from "./layout/Pane";
import { MESSAGES, WORKSPACES, type Workspace } from "./layout/sample";
import { EmptyState, MessagesPanel, Sidebar, StatusBar, TitleBar } from "./layout/Shell";

function useWindowWidth(): number {
  const [width, setWidth] = useState(window.innerWidth);
  useEffect(() => {
    const onResize = () => setWidth(window.innerWidth);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, []);
  return width;
}

// Layout from T1.2: starts without a workspace (UI-UX §4.7); any start button opens a real shell in the home folder as
// the first pane (T1.4) next to the sample panes. Split and real workspaces are T1.5.
function App() {
  const width = useWindowWidth();
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [activeName, setActiveName] = useState<string | null>(null);
  const [focusedPane, setFocusedPane] = useState<string | null>(null);
  const [sidebar, setSidebar] = useState<PanelOverride>(null);
  const [messages, setMessages] = useState<PanelOverride>(null);
  const starting = useRef(false);

  const active = workspaces.find((ws) => ws.name === activeName) ?? null;
  const sidebarOpen = panelOpen(width, SIDEBAR_BREAKPOINT, sidebar);
  const messagesOpen = active !== null && panelOpen(width, MESSAGES_BREAKPOINT, messages);
  const toggleSidebar = () => setSidebar(togglePanel(width, SIDEBAR_BREAKPOINT, sidebar));
  const toggleMessages = () => setMessages(togglePanel(width, MESSAGES_BREAKPOINT, messages));

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!e.metaKey || e.altKey || e.ctrlKey) return;
      const key = e.key.toLowerCase();
      if (key === "b" && !e.shiftKey) {
        e.preventDefault();
        toggleSidebar();
      } else if (key === "m" && e.shiftKey) {
        e.preventDefault();
        toggleMessages();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const selectWorkspace = (name: string) => {
    setActiveName(name);
    setFocusedPane(workspaces.find((ws) => ws.name === name)?.panes[0]?.id ?? null);
  };

  // The PTY is created here, in an event handler, not in a TerminalPane effect that StrictMode would run twice.
  const start = async () => {
    if (starting.current) return;
    starting.current = true;
    const [first, ...rest] = WORKSPACES;
    let panes = first.panes;
    try {
      const ptyId = await invoke<number>("pane_create_shell", { cwd: await homeDir(), rows: 24, cols: 80 });
      const shell = { id: "p1", agent: "Shell", title: "shell", folder: "~", status: "unknown" as const, pending: 0, ptyId };
      panes = [shell, ...first.panes.slice(1)];
    } catch (e) {
      console.error("pane_create_shell", e);
    }
    setWorkspaces([{ ...first, panes }, ...rest]);
    setActiveName(first.name);
    setFocusedPane(panes[0].id);
  };

  return (
    <div className="app">
      <TitleBar
        workspace={active}
        sidebarOpen={sidebarOpen}
        messagesOpen={messagesOpen}
        onToggleSidebar={toggleSidebar}
        onToggleMessages={toggleMessages}
      />
      <div className="main">
        {sidebarOpen ? (
          <Sidebar
            workspaces={workspaces}
            active={active}
            focusedPane={focusedPane}
            onSelectWorkspace={selectWorkspace}
            onFocusPane={setFocusedPane}
          />
        ) : null}
        <main className="workspace">
          {active ? (
            active.panes.map((pane) => (
              <PaneView
                key={pane.id}
                pane={pane}
                focused={pane.id === focusedPane}
                onFocus={() => setFocusedPane(pane.id)}
              />
            ))
          ) : (
            <EmptyState onStart={start} />
          )}
        </main>
        {active && messagesOpen ? <MessagesPanel workspace={active} messages={active === workspaces[0] ? MESSAGES : []} /> : null}
      </div>
      <StatusBar />
    </div>
  );
}

export default App;
