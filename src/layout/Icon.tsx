import claudeCodeLogo from "../assets/providers/claude-code.png";
import codexLogo from "../assets/providers/codex.png";

// Icon paths from Lucide (https://lucide.dev), ISC License, Copyright (c) Lucide Contributors.
const PATHS = {
  plus: <path d="M5 12h14m-7-7v14" />,
  x: <path d="M18 6L6 18M6 6l12 12" />,
  chevronUp: <path d="m18 15l-6-6l-6 6" />,
  chevronDown: <path d="m6 9l6 6l6-6" />,
  settings: (
    <>
      <path d="M9.671 4.136a2.34 2.34 0 0 1 4.659 0a2.34 2.34 0 0 0 3.319 1.915a2.34 2.34 0 0 1 2.33 4.033a2.34 2.34 0 0 0 0 3.831a2.34 2.34 0 0 1-2.33 4.033a2.34 2.34 0 0 0-3.319 1.915a2.34 2.34 0 0 1-4.659 0a2.34 2.34 0 0 0-3.32-1.915a2.34 2.34 0 0 1-2.33-4.033a2.34 2.34 0 0 0 0-3.831A2.34 2.34 0 0 1 6.35 6.051a2.34 2.34 0 0 0 3.319-1.915" />
      <circle cx="12" cy="12" r="3" />
    </>
  ),
  splitRight: (
    <>
      <rect width="18" height="18" x="3" y="3" rx="2" />
      <path d="M12 3v18" />
    </>
  ),
  splitDown: (
    <>
      <rect width="18" height="18" x="3" y="3" rx="2" />
      <path d="M3 12h18" />
    </>
  ),
  maximize: <path d="M15 3h6v6m0-6l-7 7M3 21l7-7m-1 7H3v-6" />,
  minimize: <path d="M14 10l7-7m-1 7h-6V4M3 21l7-7m-6 0h6v6" />,
  panelLeft: (
    <>
      <rect width="18" height="18" x="3" y="3" rx="2" />
      <path d="M9 3v18" />
    </>
  ),
  panelRight: (
    <>
      <rect width="18" height="18" x="3" y="3" rx="2" />
      <path d="M15 3v18" />
    </>
  ),
  terminal: <path d="M12 19h8M4 17l6-6l-6-6" />,
  alert: (
    <>
      <circle cx="12" cy="12" r="10" />
      <path d="M12 8v4m0 4h.01" />
    </>
  ),
  copy: (
    <>
      <rect width="14" height="14" x="8" y="8" rx="2" />
      <path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2" />
    </>
  ),
  folder: (
    <path d="M20 20a2 2 0 0 0 2-2V8a2 2 0 0 0-2-2h-7.9a2 2 0 0 1-1.69-.9L9.6 3.9A2 2 0 0 0 7.93 3H4a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2Z" />
  ),
  bot: (
    <>
      <path d="M12 8V4H8" />
      <rect width="16" height="12" x="4" y="8" rx="2" />
      <path d="M2 14h2m16 0h2m-7-1v2m-6-2v2" />
    </>
  ),
  info: (
    <>
      <circle cx="12" cy="12" r="10" />
      <path d="M12 16v-4m0-4h.01" />
    </>
  ),
  keyboard: (
    <>
      <rect width="20" height="16" x="2" y="4" rx="2" />
      <path d="M6 8h.01M10 8h.01M14 8h.01M18 8h.01M8 12h.01M12 12h.01M16 12h.01M7 16h10" />
    </>
  ),
  users: (
    <>
      <path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2" />
      <circle cx="9" cy="7" r="4" />
      <path d="M22 21v-2a4 4 0 0 0-3-3.87M16 3.13a4 4 0 0 1 0 7.75" />
    </>
  ),
  check: <path d="M20 6L9 17l-5-5" />,
  download: <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4m4-5l5 5l5-5m-5 5V3" />,
  refresh: (
    <>
      <path d="M21 12a9 9 0 1 1-9-9c2.52 0 4.93 1 6.74 2.74L21 8" />
      <path d="M21 3v5h-5" />
    </>
  ),
  sparkles: (
    <>
      <path d="M11.017 2.814a1 1 0 0 1 1.966 0l1.051 5.558a2 2 0 0 0 1.594 1.594l5.558 1.051a1 1 0 0 1 0 1.966l-5.558 1.051a2 2 0 0 0-1.594 1.594l-1.051 5.558a1 1 0 0 1-1.966 0l-1.051-5.558a2 2 0 0 0-1.594-1.594l-5.558-1.051a1 1 0 0 1 0-1.966l5.558-1.051a2 2 0 0 0 1.594-1.594zM20 2v4m2-2h-4" />
      <circle cx="4" cy="20" r="2" />
    </>
  ),
};

export type IconName = keyof typeof PATHS;

export function Icon({ name, size = 14 }: { name: IconName; size?: number }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {PATHS[name]}
    </svg>
  );
}

/** Icon-only button: aria-label and native tooltip carry the same text (UI-UX §8). */
export function IconButton({
  icon,
  label,
  onClick,
  pressed,
}: {
  icon: IconName;
  label: string;
  onClick?: () => void;
  pressed?: boolean;
}) {
  return (
    <button type="button" className="icon-btn" aria-label={label} title={label} aria-pressed={pressed} onClick={onClick}>
      <Icon name={icon} />
    </button>
  );
}

const AGENT_ICON: Record<string, IconName> = { "claude-code": "sparkles", codex: "bot" };
const AGENT_LOGO: Record<string, string> = { "claude-code": claudeCodeLogo, codex: codexLogo };

/** Glyph for a catalogue agent id; shells and unknown agents get the terminal prompt. */
export function agentIcon(id: string | null): IconName {
  return (id && AGENT_ICON[id]) || "terminal";
}

/** Agent identity tile: use the provider mark for built-in agents and Lucide for shells/unknown agents. */
export function AgentMark({ id, size = 16 }: { id: string | null; size?: number }) {
  const shell = id === null || id === "shell";
  const logo = id ? AGENT_LOGO[id] : undefined;
  return (
    <span
      className={`agent-mark${shell ? " agent-mark-shell" : ""}${logo ? " agent-mark-logo" : ""}`}
      style={{ width: size, height: size }}
      aria-hidden="true"
    >
      {logo ? <img className="agent-logo" src={logo} alt="" /> : <Icon name={agentIcon(id)} size={Math.round(size * 0.66)} />}
    </span>
  );
}
