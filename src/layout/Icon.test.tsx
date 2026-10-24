import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { AgentMark } from "./Icon";

describe("AgentMark", () => {
  it("uses the provider artwork for Claude Code and Codex", () => {
    for (const id of ["claude-code", "codex"]) {
      const out = renderToStaticMarkup(<AgentMark id={id} />);
      expect(out).toContain('class="agent-mark agent-mark-logo"');
      expect(out).toContain('class="agent-logo"');
    }
  });

  it("keeps the Lucide fallback for shells and unknown agents", () => {
    expect(renderToStaticMarkup(<AgentMark id={null} />)).not.toContain("agent-mark-logo");
    expect(renderToStaticMarkup(<AgentMark id="gemini" />)).not.toContain("agent-mark-logo");
  });
});
