//! Headless `vt100` screen per pane (ARCHITECTURE §2): text for `pane_read`, bracketed paste state, last output
//! time, and whether the agent's input box is empty before a paste (§5.2 point 4, D-019).

use std::num::NonZeroU16;
use std::time::Instant;

/// `pane_read` takes at most 400 lines (MCP-TOOLS), so more scrollback is never read.
const SCROLLBACK: usize = 400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputState {
    Empty,
    HasText,
    /// Not sure (unknown agent, mid-redraw, multi-row input, a card instead of the prompt); the caller waits.
    Unknown,
}

pub struct Screen {
    parser: vt100::Parser,
    last_output: Option<Instant>,
}

fn size(n: u16) -> NonZeroU16 {
    NonZeroU16::new(n).unwrap_or(NonZeroU16::MIN)
}

impl Screen {
    pub fn new(rows: u16, cols: u16) -> Self {
        Self {
            parser: vt100::Parser::new(size(rows), size(cols), SCROLLBACK),
            last_output: None,
        }
    }

    pub fn process(&mut self, bytes: &[u8]) {
        self.process_at(bytes, Instant::now());
    }

    /// Like [`Screen::process`] with an explicit clock, for tests.
    pub fn process_at(&mut self, bytes: &[u8], now: Instant) {
        if bytes.is_empty() {
            return;
        }
        self.parser.process(bytes);
        self.last_output = Some(now);
    }

    pub fn last_output(&self) -> Option<Instant> {
        self.last_output
    }

    /// A zero dimension is clamped to 1.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.parser.screen_mut().set_size(size(rows), size(cols));
    }

    pub fn bracketed_paste(&self) -> bool {
        self.parser.screen().bracketed_paste()
    }

    pub fn alternate_screen(&self) -> bool {
        self.parser.screen().alternate_screen()
    }

    /// The last `lines` rows of scrollback + screen as plain text, trailing blank rows dropped. The alternate screen
    /// has no scrollback, so there it returns at most the screen height. On the input row (see [`Screen::prompt_row`])
    /// dim cells are left out: they are Claude's suggestion or Codex's placeholder, not typed text.
    pub fn read(&mut self, lines: usize) -> String {
        let s = self.parser.screen_mut();
        let (rows, cols) = s.size();
        let rows = usize::from(rows.get());
        let mut out: Vec<String> = Vec::with_capacity(s.scrollback_len() + rows);
        // Offset `top` shows scrollback line `len - top` at the top of the view.
        let mut top = s.scrollback_len();
        while top > 0 {
            s.set_scrollback(top);
            let take = top.min(rows);
            out.extend(s.rows(0, cols.get()).take(take));
            top -= take;
        }
        s.set_scrollback(0);
        let live = out.len();
        out.extend(s.rows(0, cols.get()));
        if let Some(row) = prompt_row(s) {
            out[live + usize::from(row)] = (0..cols.get())
                .filter_map(|c| s.cell(row, c))
                .map(|c| match c.contents() {
                    _ if c.is_wide_continuation() => "",
                    _ if c.dim() => " ",
                    "" => " ",
                    t => t,
                })
                .collect();
        }
        while out.last().is_some_and(|l| l.trim().is_empty()) {
            out.pop();
        }
        let tail = out.split_off(out.len().saturating_sub(lines));
        tail.iter()
            .map(|l| l.trim_end())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Input box state for a catalogue agent id. Both TUIs park the real cursor on the prompt row (`❯` Claude Code,
    /// `›` Codex) and draw placeholders and Claude's suggestion dim (`SGR 2`), so: any non-blank, non-dim cell after
    /// the prompt glyph is text. The row below must close the box (Claude's `─` border, Codex's blank row); otherwise
    /// the input may continue there and the state is Unknown.
    pub fn input_state(&self, agent: &str) -> InputState {
        let s = self.parser.screen();
        let glyph = match agent {
            "claude-code" => "❯",
            "codex" => "›",
            _ => return InputState::Unknown,
        };
        if s.hide_cursor() {
            return InputState::Unknown; // both TUIs hide the cursor while redrawing
        }
        let (row, _) = s.cursor_position();
        let Some(first) = s.cell(row, 0) else {
            return InputState::Unknown;
        };
        // Dim `›` is Codex history; inverse `›` is a selected card option.
        if first.contents() != glyph || first.dim() || first.inverse() {
            return InputState::Unknown;
        }
        let (_, cols) = s.size();
        let blank = |c: &vt100::Cell| c.contents().trim().is_empty();
        let closed = match agent {
            "claude-code" => s.cell(row + 1, 0).is_some_and(|c| c.contents() == "─"),
            _ => (0..cols.get()).all(|c| s.cell(row + 1, c).is_none_or(blank)),
        };
        if !closed {
            return InputState::Unknown;
        }
        let typed = (1..cols.get())
            .filter_map(|c| s.cell(row, c))
            .any(|c| !blank(c) && !c.dim());
        if typed {
            InputState::HasText
        } else {
            InputState::Empty
        }
    }
}

/// The cursor row when it starts with a non-dim, non-inverse prompt glyph (`❯` Claude Code, `›` Codex), as in
/// [`Screen::input_state`]. Dim `›` is Codex history and inverse `›` a selected card option, so those are not it.
fn prompt_row(s: &vt100::Screen) -> Option<u16> {
    let (row, _) = s.cursor_position();
    let first = s.cell(row, 0)?;
    (matches!(first.contents(), "❯" | "›") && !first.dim() && !first.inverse()).then_some(row)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const ALT_ON: &[u8] = b"\x1b[?1049h";
    const BP_ON: &[u8] = b"\x1b[?2004h";

    fn screen(rows: u16, cols: u16, chunks: &[&[u8]]) -> Screen {
        let mut s = Screen::new(rows, cols);
        for c in chunks {
            s.process(c);
        }
        s
    }

    fn numbered(n: usize) -> Vec<u8> {
        (1..=n)
            .map(|i| format!("line {i}\r\n"))
            .collect::<String>()
            .into_bytes()
    }

    #[test]
    fn read_includes_scrollback_on_main_screen() {
        let mut s = screen(5, 20, &[&numbered(12)]);
        assert!(!s.alternate_screen());
        assert_eq!(s.read(3), "line 10\nline 11\nline 12");
        let all = s.read(400);
        assert!(all.starts_with("line 1\n") && all.ends_with("line 12"));
        assert_eq!(all.lines().count(), 12);
        // Reading leaves the live view in place.
        assert_eq!(s.parser.screen().scrollback(), 0);
    }

    #[test]
    fn read_on_alternate_screen_is_capped_at_height() {
        let mut s = screen(
            5,
            20,
            &[&numbered(12), ALT_ON, b"\x1b[H", &numbered(4), b"bottom"],
        );
        assert!(s.alternate_screen());
        let text = s.read(400);
        assert_eq!(text, "line 1\nline 2\nline 3\nline 4\nbottom");
        assert_eq!(s.read(2), "line 4\nbottom");
    }

    #[test]
    fn read_strips_colour() {
        let mut s = screen(3, 20, &[b"\x1b[31mred\x1b[0m \x1b[2mdim\x1b[22m"]);
        assert_eq!(s.read(10), "red dim");
    }

    #[test]
    fn resize_changes_size_and_clamps_zero() {
        let mut s = screen(5, 20, &[]);
        s.resize(10, 40);
        assert_eq!(s.parser.screen().size(), (size(10), size(40)));
        s.resize(0, 0);
        assert_eq!(s.parser.screen().size(), (size(1), size(1)));
    }

    #[test]
    fn resize_narrower_truncates_rows() {
        let mut s = screen(3, 20, &[b"abcdefghij"]);
        s.resize(3, 4);
        assert_eq!(s.read(10), "abcd");
    }

    #[test]
    fn bracketed_paste_on_off() {
        let mut s = screen(3, 20, &[]);
        assert!(!s.bracketed_paste());
        s.process(BP_ON);
        assert!(s.bracketed_paste());
        s.process(b"\x1b[?2004l");
        assert!(!s.bracketed_paste());
    }

    #[test]
    fn last_output_uses_given_clock_and_ignores_empty_chunks() {
        let mut s = Screen::new(3, 20);
        assert_eq!(s.last_output(), None);
        let t0 = Instant::now();
        s.process_at(b"x", t0);
        s.process_at(b"", t0 + Duration::from_secs(5));
        assert_eq!(s.last_output(), Some(t0));
        s.process_at(b"y", t0 + Duration::from_secs(9));
        assert_eq!(s.last_output(), Some(t0 + Duration::from_secs(9)));
    }

    // Input box fixtures follow the byte shapes in the S1/S2 logs (Claude Code 2.1.306, Codex 0.163.0) on an 8x40
    // screen: Claude draws `─` border / `❯ ` + NBSP / border; Codex draws bold `›`, dim placeholder, blank row.
    const CLAUDE_BOX: &[u8] = "\x1b[4;1H\x1b[38;5;244m────────\x1b[39m\x1b[5;1H❯\u{a0}\x1b[6;1H\x1b[38;5;244m────────\x1b[39m\
        \x1b[8;3H\x1b[2mctx 53k/1.0M 5%\x1b[22m"
        .as_bytes();
    const CODEX_BOX: &[u8] =
        "\x1b[3;1H\x1b[2m›\x1b[22m old prompt\x1b[6;1H\x1b[1m›\x1b[22m \x1b[8;3Hfor shortcuts"
            .as_bytes();

    fn claude(extra: &[u8]) -> Screen {
        screen(8, 40, &[ALT_ON, BP_ON, CLAUDE_BOX, extra])
    }

    fn codex(extra: &[u8]) -> Screen {
        screen(8, 40, &[ALT_ON, BP_ON, CODEX_BOX, extra])
    }

    #[test]
    fn claude_empty_input() {
        assert_eq!(
            claude(b"\x1b[5;3H").input_state("claude-code"),
            InputState::Empty
        );
    }

    #[test]
    fn claude_dim_suggestion_counts_as_empty() {
        // Exact shape from s2-claude.bin: hide, write dim suggestion, park cursor after `❯ `, show.
        let s = claude(b"\x1b[?25l\x1b[5;3H\x1b[2mcatat hasil spike ini\x1b[22m\x1b[5;3H\x1b[?25h");
        assert_eq!(s.input_state("claude-code"), InputState::Empty);
    }

    #[test]
    fn claude_typed_text() {
        assert_eq!(
            claude(b"\x1b[5;3Hhello\x1b[5;8H").input_state("claude-code"),
            InputState::HasText
        );
        // Typed text in front of a dim suggestion is still text.
        let s = claude(b"\x1b[5;3Hfix\x1b[2m the bug\x1b[22m\x1b[5;6H");
        assert_eq!(s.input_state("claude-code"), InputState::HasText);
    }

    #[test]
    fn claude_unknown_cases() {
        // Mid-redraw (cursor hidden).
        assert_eq!(
            claude(b"\x1b[5;3H\x1b[?25l").input_state("claude-code"),
            InputState::Unknown
        );
        // Cursor not on the prompt row.
        assert_eq!(
            claude(b"\x1b[2;3H").input_state("claude-code"),
            InputState::Unknown
        );
        // Input spans two rows: the row below the prompt is not the border.
        let s = screen(
            8,
            40,
            &[
                ALT_ON,
                "\x1b[5;1H❯\u{a0}\x1b[6;3Hsecond line\x1b[5;3H".as_bytes(),
            ],
        );
        assert_eq!(s.input_state("claude-code"), InputState::Unknown);
        // Wrong agent id for this screen, and unknown agents.
        assert_eq!(
            claude(b"\x1b[5;3H").input_state("codex"),
            InputState::Unknown
        );
        assert_eq!(
            claude(b"\x1b[5;3H").input_state("gemini"),
            InputState::Unknown
        );
    }

    #[test]
    fn codex_dim_placeholder_counts_as_empty() {
        // Exact shape from s1-codex.bin.
        let s = codex(b"\x1b[6;3H\x1b[22m\x1b[2m\x1b[2mAsk Codex to do anything\x1b[22m\x1b[6;3H");
        assert_eq!(s.input_state("codex"), InputState::Empty);
        assert_eq!(codex(b"\x1b[6;3H").input_state("codex"), InputState::Empty);
    }

    #[test]
    fn read_drops_dim_suggestion_on_prompt_row_only() {
        // (a) Suggestion alone: the prompt row reads as just the glyph; dim status line elsewhere stays.
        let mut s =
            claude(b"\x1b[?25l\x1b[5;3H\x1b[2mTry \"fix lint errors\"\x1b[22m\x1b[5;3H\x1b[?25h");
        let text = s.read(10);
        assert!(!text.contains("fix lint"), "{text}");
        assert!(text.lines().any(|l| l == "❯"), "{text}");
        assert!(text.ends_with("ctx 53k/1.0M 5%"), "{text}");
        // (b) Typed text before the suggestion is kept, the dim tail dropped.
        let mut s = claude(b"\x1b[5;3Hfix\x1b[2m the bug\x1b[22m\x1b[5;6H");
        assert!(s.read(10).lines().any(|l| l == "❯\u{a0}fix"));
        // (c) Dim text on other rows is kept, including a dim `›` history row and when the cursor is elsewhere.
        let mut s = codex(b"\x1b[6;3H\x1b[2mAsk Codex to do anything\x1b[22m\x1b[6;3H");
        let text = s.read(10);
        assert!(
            text.contains("› old prompt") && !text.contains("Ask Codex"),
            "{text}"
        );
        let mut s = claude(b"\x1b[5;3H\x1b[2mdim\x1b[22m\x1b[2;1H");
        assert!(s.read(10).contains("❯\u{a0}dim"));
    }

    #[test]
    fn codex_typed_text() {
        assert_eq!(
            codex(b"\x1b[6;3Hfix the bug\x1b[6;14H").input_state("codex"),
            InputState::HasText
        );
    }

    #[test]
    fn codex_unknown_cases() {
        // Dim `›` is a history row, not the composer.
        assert_eq!(
            codex(b"\x1b[3;3H").input_state("codex"),
            InputState::Unknown
        );
        // Input continues on the next row.
        assert_eq!(
            codex(b"\x1b[6;3Hone\x1b[7;3Htwo\x1b[6;6H").input_state("codex"),
            InputState::Unknown
        );
        // Approval card: selected option is drawn inverse (s5-codex-perm.bin).
        let s = screen(
            8,
            40,
            &[
                ALT_ON,
                "\x1b[3;1H\x1b[7m› 1. Yes, proceed (y)\x1b[27m\x1b[3;1H".as_bytes(),
            ],
        );
        assert_eq!(s.input_state("codex"), InputState::Unknown);
    }
}
