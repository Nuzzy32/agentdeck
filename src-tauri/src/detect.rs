//! Agent state from the screen, for agents that do not report OSC 7501 (D-035).
//!
//! Ported from herdr (https://github.com/herdrdev/herdr) at commit 6b526626f66e22c08962a7f39a83fc9ac2ebda62:
//! `src/detect/manifest.rs` (rule schema, validation, matching, regions) and `src/pane/agent_detection.rs` (the
//! working → idle hold). Licensed under the Apache License, Version 2.0; see `herdr/LICENSE`.
//! **Changed by agentdeck:** only the bundled manifests are used (no remote catalogue, override files, cache, or
//! explain output); `version` is kept as text; the debounce is reduced to the startup grace and the working → idle
//! hold; types and names follow agentdeck. Rule data in `herdr/manifests/` is copied unchanged.

use std::sync::OnceLock;
use std::time::{Duration, Instant};

use regex::Regex;
use serde::Deserialize;

/// Highest `min_engine_version` this engine understands (herdr engine v3: `top_non_empty_lines`).
pub const ENGINE_VERSION: u32 = 3;
/// How often a pane is checked; while an idle is being confirmed, [`PENDING_POLL`] instead.
pub const POLL: Duration = Duration::from_millis(300);
pub const PENDING_POLL: Duration = Duration::from_millis(100);
/// No reading for this long after the agent starts: its splash screen is not a state.
pub const STARTUP_GRACE: Duration = Duration::from_secs(3);
const PENDING_IDLE_CONFIRMATIONS: u8 = 3;
const PENDING_IDLE_CAP: Duration = Duration::from_millis(700);

const MAX_RULES_PER_MANIFEST: usize = 128;
const MAX_GATE_DEPTH: usize = 8;
const MAX_TOTAL_GATES: usize = 512;
const MAX_MATCHERS_PER_GATE: usize = 32;
const MAX_TOTAL_MATCHERS: usize = 1024;
const MAX_MATCHER_CHARS: usize = 512;
const TOP_NON_EMPTY_LINES_ENGINE_VERSION: u32 = 3;
const MAX_TOP_REGION_LINE_COUNT: usize = u16::MAX as usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Idle,
    Working,
    Blocked,
    Unknown,
}

/// Screen snapshot plus the last OSC title and OSC 9 progress payload; empty strings when there are none.
#[derive(Debug, Clone, Copy)]
pub struct Input<'a> {
    pub screen: &'a str,
    pub osc_title: &'a str,
    pub osc_progress: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detection {
    pub state: State,
    /// Id of the winning rule; `None` for the known-agent idle fallback.
    pub rule: Option<String>,
    /// A transcript viewer or menu is open: keep the previous state.
    pub skip: bool,
    pub visible_idle: bool,
    pub visible_blocker: bool,
    pub visible_working: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    id: String,
    version: Option<String>,
    min_engine_version: Option<u32>,
    #[serde(rename = "updated_at")]
    _updated_at: Option<String>,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    rules: Vec<RawRule>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRule {
    id: String,
    state: Option<State>,
    #[serde(default)]
    priority: i32,
    #[serde(default = "default_region")]
    region: String,
    #[serde(default)]
    visible_idle: bool,
    #[serde(default)]
    visible_blocker: bool,
    #[serde(default)]
    visible_working: bool,
    #[serde(default)]
    skip_state_update: bool,
    #[serde(default)]
    all: Vec<RawGate>,
    #[serde(default)]
    any: Vec<RawGate>,
    #[serde(default, rename = "not")]
    not_gate: Vec<RawGate>,
    #[serde(default)]
    contains: Vec<String>,
    #[serde(default)]
    regex: Vec<String>,
    #[serde(default)]
    line_regex: Vec<String>,
}

impl RawRule {
    /// The rule's own matchers as its top gate (herdr `manifest_gate_from_rule`).
    fn gate(&self) -> RawGate {
        RawGate {
            all: self.all.clone(),
            any: self.any.clone(),
            not_gate: self.not_gate.clone(),
            contains: self.contains.clone(),
            regex: self.regex.clone(),
            line_regex: self.line_regex.clone(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct RawGate {
    #[serde(default)]
    all: Vec<RawGate>,
    #[serde(default)]
    any: Vec<RawGate>,
    #[serde(default, rename = "not")]
    not_gate: Vec<RawGate>,
    #[serde(default)]
    contains: Vec<String>,
    #[serde(default)]
    regex: Vec<String>,
    #[serde(default)]
    line_regex: Vec<String>,
}

fn default_region() -> String {
    "whole_recent".to_owned()
}

#[derive(Debug)]
struct Gate {
    all: Vec<Gate>,
    any: Vec<Gate>,
    not_gate: Vec<Gate>,
    /// Lowercased: `contains` is case-insensitive.
    contains: Vec<String>,
    regex: Vec<Regex>,
    line_regex: Vec<Regex>,
}

#[derive(Debug)]
struct Rule {
    id: String,
    state: State,
    priority: i32,
    region: String,
    visible_idle: bool,
    visible_blocker: bool,
    visible_working: bool,
    skip: bool,
    gate: Gate,
}

/// One agent's validated, compiled rules.
#[derive(Debug)]
pub struct Manifest {
    pub id: String,
    pub aliases: Vec<String>,
    pub version: Option<String>,
    rules: Vec<Rule>,
}

impl Manifest {
    /// Parses, validates against herdr's limits, and compiles one manifest.
    pub fn parse(src: &str) -> Result<Self, String> {
        let raw: RawManifest = toml::from_str(src).map_err(|e| e.to_string())?;
        if raw.min_engine_version.is_some_and(|v| v > ENGINE_VERSION) {
            return Err(format!(
                "manifest needs engine version {:?}, this engine is {ENGINE_VERSION}",
                raw.min_engine_version
            ));
        }
        validate(&raw)?;
        let rules = raw
            .rules
            .iter()
            .map(|r| {
                Ok(Rule {
                    id: r.id.clone(),
                    state: r.state.unwrap_or(State::Unknown),
                    priority: r.priority,
                    region: r.region.trim().to_owned(),
                    visible_idle: r.visible_idle,
                    visible_blocker: r.visible_blocker,
                    visible_working: r.visible_working,
                    skip: r.skip_state_update,
                    gate: compile(&r.gate())
                        .map_err(|e| format!("rule {} could not be compiled: {e}", r.id))?,
                })
            })
            .collect::<Result<_, String>>()?;
        Ok(Self {
            id: raw.id,
            aliases: raw.aliases,
            version: raw.version,
            rules,
        })
    }

    /// Every rule is evaluated; the highest priority wins, the earlier rule on a tie. No match: idle.
    pub fn detect(&self, input: Input<'_>) -> Detection {
        let mut best: Option<&Rule> = None;
        for rule in &self.rules {
            if !matches(&rule.gate, region(input, &rule.region)) {
                continue;
            }
            if best.is_none_or(|b| b.priority < rule.priority) {
                best = Some(rule);
            }
        }
        match best {
            None => Detection {
                state: State::Idle,
                rule: None,
                skip: false,
                visible_idle: false,
                visible_blocker: false,
                visible_working: false,
            },
            Some(r) => Detection {
                state: r.state,
                rule: Some(r.id.clone()),
                skip: r.skip,
                visible_idle: r.visible_idle && r.state == State::Idle,
                visible_blocker: r.visible_blocker && r.state == State::Blocked,
                visible_working: r.visible_working && r.state == State::Working,
            },
        }
    }

    fn answers_to(&self, name: &str) -> bool {
        self.id == name || self.aliases.iter().any(|a| a == name)
    }
}

const BUNDLED: [&str; 22] = [
    include_str!("../herdr/manifests/amp.toml"),
    include_str!("../herdr/manifests/antigravity.toml"),
    include_str!("../herdr/manifests/claude.toml"),
    include_str!("../herdr/manifests/cline.toml"),
    include_str!("../herdr/manifests/codex.toml"),
    include_str!("../herdr/manifests/cursor.toml"),
    include_str!("../herdr/manifests/devin.toml"),
    include_str!("../herdr/manifests/droid.toml"),
    include_str!("../herdr/manifests/gemini.toml"),
    include_str!("../herdr/manifests/github-copilot.toml"),
    include_str!("../herdr/manifests/grok.toml"),
    include_str!("../herdr/manifests/hermes.toml"),
    include_str!("../herdr/manifests/kilo.toml"),
    include_str!("../herdr/manifests/kimi.toml"),
    include_str!("../herdr/manifests/kiro.toml"),
    include_str!("../herdr/manifests/letta.toml"),
    include_str!("../herdr/manifests/maki.toml"),
    include_str!("../herdr/manifests/muse.toml"),
    include_str!("../herdr/manifests/opencode.toml"),
    include_str!("../herdr/manifests/pi.toml"),
    include_str!("../herdr/manifests/qodercli.toml"),
    include_str!("../herdr/manifests/qwen.toml"),
];

fn bundled() -> &'static [Manifest] {
    static ALL: OnceLock<Vec<Manifest>> = OnceLock::new();
    ALL.get_or_init(|| {
        BUNDLED
            .iter()
            .filter_map(|src| {
                Manifest::parse(src)
                    .map_err(|e| eprintln!("agentdeck: herdr manifest skipped: {e}"))
                    .ok()
            })
            .collect()
    })
}

/// The bundled manifest for `name` (its id or an alias), e.g. `gemini`, `claude-code`.
pub fn manifest(name: &str) -> Option<&'static Manifest> {
    bundled().iter().find(|m| m.answers_to(name))
}

/// Ids of the bundled manifests, for catalogue error messages.
pub fn ids() -> Vec<&'static str> {
    bundled().iter().map(|m| m.id.as_str()).collect()
}

// ---- validation (herdr limits) ----

#[derive(Default)]
struct Complexity {
    gates: usize,
    matchers: usize,
}

fn validate(m: &RawManifest) -> Result<(), String> {
    if m.rules.is_empty() {
        return Err("manifest must contain at least one rule".into());
    }
    if m.rules.len() > MAX_RULES_PER_MANIFEST {
        return Err(format!(
            "manifest contains {} rules, max is {MAX_RULES_PER_MANIFEST}",
            m.rules.len()
        ));
    }
    let mut c = Complexity::default();
    for rule in &m.rules {
        if rule.id.trim().is_empty() {
            return Err("manifest rule id must not be empty".into());
        }
        if rule.skip_state_update {
            if rule.state != Some(State::Unknown) {
                return Err(format!(
                    "rule {} uses skip_state_update without state = \"unknown\"",
                    rule.id
                ));
            }
            if rule.visible_idle || rule.visible_blocker || rule.visible_working {
                return Err(format!(
                    "rule {} uses skip_state_update with visible state evidence",
                    rule.id
                ));
            }
        }
        validate_region_name(&rule.region)
            .map_err(|e| format!("rule {} uses invalid region: {e}", rule.id))?;
        if rule.region.trim().starts_with("top_non_empty_lines(")
            && m.min_engine_version
                .is_some_and(|v| v < TOP_NON_EMPTY_LINES_ENGINE_VERSION)
        {
            return Err(format!(
                "rule {} uses top_non_empty_lines but min_engine_version is below {TOP_NON_EMPTY_LINES_ENGINE_VERSION}",
                rule.id
            ));
        }
        validate_gate(&rule.gate(), "rule", 0, &mut c)
            .map_err(|e| format!("rule {} has invalid matcher gates: {e}", rule.id))?;
    }
    Ok(())
}

fn validate_gate(g: &RawGate, ctx: &str, depth: usize, c: &mut Complexity) -> Result<(), String> {
    if depth > MAX_GATE_DEPTH {
        return Err(format!("{ctx} exceeds max gate depth {MAX_GATE_DEPTH}"));
    }
    c.gates += 1;
    if c.gates > MAX_TOTAL_GATES {
        return Err(format!("manifest exceeds max gate count {MAX_TOTAL_GATES}"));
    }
    validate_matchers(g, ctx, c)?;
    if !has_positive(g) {
        return Err(format!("{ctx} must contain a positive matcher"));
    }
    for nested in &g.all {
        validate_gate(nested, "all gate", depth + 1, c)?;
    }
    for nested in &g.any {
        validate_gate(nested, "any gate", depth + 1, c)?;
    }
    for nested in &g.not_gate {
        if !has_any(nested) {
            return Err(format!("{ctx} contains an empty not gate"));
        }
        validate_not(nested, depth + 1, c)?;
    }
    Ok(())
}

fn validate_not(g: &RawGate, depth: usize, c: &mut Complexity) -> Result<(), String> {
    if depth > MAX_GATE_DEPTH {
        return Err(format!("not gate exceeds max gate depth {MAX_GATE_DEPTH}"));
    }
    c.gates += 1;
    if c.gates > MAX_TOTAL_GATES {
        return Err(format!("manifest exceeds max gate count {MAX_TOTAL_GATES}"));
    }
    validate_matchers(g, "not gate", c)?;
    if !has_any(g) {
        return Err("not gate must contain a matcher".into());
    }
    for nested in &g.all {
        validate_gate(nested, "not all gate", depth + 1, c)?;
    }
    for nested in &g.any {
        validate_gate(nested, "not any gate", depth + 1, c)?;
    }
    for nested in &g.not_gate {
        validate_not(nested, depth + 1, c)?;
    }
    Ok(())
}

fn validate_matchers(g: &RawGate, ctx: &str, c: &mut Complexity) -> Result<(), String> {
    let n = g.contains.len() + g.regex.len() + g.line_regex.len();
    if n > MAX_MATCHERS_PER_GATE {
        return Err(format!(
            "{ctx} has {n} direct matchers, max is {MAX_MATCHERS_PER_GATE}"
        ));
    }
    c.matchers += n;
    if c.matchers > MAX_TOTAL_MATCHERS {
        return Err(format!(
            "manifest exceeds max matcher count {MAX_TOTAL_MATCHERS}"
        ));
    }
    if g.contains
        .iter()
        .chain(&g.regex)
        .chain(&g.line_regex)
        .any(|v| v.chars().count() > MAX_MATCHER_CHARS)
    {
        return Err(format!(
            "{ctx} matcher exceeds max length {MAX_MATCHER_CHARS}"
        ));
    }
    for p in g.regex.iter().chain(&g.line_regex) {
        Regex::new(p).map_err(|e| format!("{ctx} contains invalid pattern {p:?}: {e}"))?;
    }
    Ok(())
}

fn has_positive(g: &RawGate) -> bool {
    !g.contains.is_empty()
        || !g.regex.is_empty()
        || !g.line_regex.is_empty()
        || !g.all.is_empty()
        || !g.any.is_empty()
}

fn has_any(g: &RawGate) -> bool {
    has_positive(g) || !g.not_gate.is_empty()
}

fn validate_region_name(spec: &str) -> Result<(), String> {
    let t = spec.trim();
    match t {
        "whole_recent"
        | "after_last_prompt_marker"
        | "before_current_prompt_marker"
        | "whole_recent_without_current_prompt_marker"
        | "current_prompt_block_marker"
        | "after_current_prompt_block_marker"
        | "prompt_box_body"
        | "above_prompt_box"
        | "last_non_empty_above_prompt_box"
        | "after_last_horizontal_rule"
        | "osc_title"
        | "osc_progress" => Ok(()),
        _ if region_count(t, "bottom_lines").is_some()
            || region_count(t, "bottom_non_empty_lines").is_some()
            || top_region_count(t).is_some() =>
        {
            Ok(())
        }
        _ => Err(t.to_owned()),
    }
}

// ---- matching ----

fn compile(g: &RawGate) -> Result<Gate, String> {
    let re = |ps: &[String]| -> Result<Vec<Regex>, String> {
        ps.iter()
            .map(|p| Regex::new(p).map_err(|e| e.to_string()))
            .collect()
    };
    Ok(Gate {
        all: g.all.iter().map(compile).collect::<Result<_, _>>()?,
        any: g.any.iter().map(compile).collect::<Result<_, _>>()?,
        not_gate: g.not_gate.iter().map(compile).collect::<Result<_, _>>()?,
        contains: g.contains.iter().map(|n| n.to_lowercase()).collect(),
        regex: re(&g.regex)?,
        line_regex: re(&g.line_regex)?,
    })
}

fn matches(g: &Gate, text: &str) -> bool {
    gate_matches(g, text, &text.to_lowercase())
}

/// Everything in a gate must hold: every needle, every regex on the whole region, every line regex on some line,
/// every `all`, at least one `any` (when present), and no `not`.
fn gate_matches(g: &Gate, text: &str, lower: &str) -> bool {
    g.contains.iter().all(|n| lower.contains(n.as_str()))
        && g.regex.iter().all(|r| r.is_match(text))
        && g.line_regex
            .iter()
            .all(|r| text.lines().any(|l| r.is_match(l)))
        && g.all.iter().all(|n| gate_matches(n, text, lower))
        && (g.any.is_empty() || g.any.iter().any(|n| gate_matches(n, text, lower)))
        && !g.not_gate.iter().any(|n| gate_matches(n, text, lower))
}

// ---- regions ----

fn region<'a>(input: Input<'a>, spec: &str) -> &'a str {
    let t = spec.trim();
    let content = input.screen;
    match t {
        "osc_title" => input.osc_title,
        "osc_progress" => input.osc_progress,
        "whole_recent" => content,
        "after_last_prompt_marker" => after_last_prompt_marker(content),
        "before_current_prompt_marker" => before_current_prompt_marker(content),
        "whole_recent_without_current_prompt_marker" => {
            let lines: Vec<&str> = content.lines().collect();
            if current_prompt_index(&lines).is_some() {
                ""
            } else {
                content
            }
        }
        "current_prompt_block_marker" => current_prompt_block_marker(content).unwrap_or(""),
        "after_current_prompt_block_marker" => {
            after_current_prompt_block_marker(content).unwrap_or("")
        }
        "prompt_box_body" => prompt_box_body(content).unwrap_or(""),
        "above_prompt_box" => above_prompt_box(content),
        "last_non_empty_above_prompt_box" => last_non_empty_line(above_prompt_box(content)),
        "after_last_horizontal_rule" => after_last_horizontal_rule(content),
        _ => {
            if let Some(n) = region_count(t, "bottom_lines") {
                let lines: Vec<&str> = content.lines().collect();
                return slice_from(content, &lines, lines.len().saturating_sub(n));
            }
            if let Some(n) = region_count(t, "bottom_non_empty_lines") {
                return bottom_non_empty_lines(content, n);
            }
            if let Some(n) = top_region_count(t) {
                return top_non_empty_lines(content, n);
            }
            ""
        }
    }
}

fn region_count(spec: &str, name: &str) -> Option<usize> {
    spec.strip_prefix(name)?
        .strip_prefix('(')?
        .strip_suffix(')')?
        .parse()
        .ok()
}

fn top_region_count(spec: &str) -> Option<usize> {
    let n = spec
        .strip_prefix("top_non_empty_lines")?
        .strip_prefix('(')?
        .strip_suffix(')')?;
    if n.starts_with('0') || !n.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    n.parse::<usize>()
        .ok()
        .filter(|n| *n <= MAX_TOP_REGION_LINE_COUNT)
}

fn bottom_non_empty_lines(content: &str, count: usize) -> &str {
    let lines: Vec<&str> = content.lines().collect();
    let Some(start) = lines
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, l)| !l.trim().is_empty())
        .take(count)
        .last()
        .map(|(i, _)| i)
    else {
        return "";
    };
    slice_from(content, &lines, start)
}

fn top_non_empty_lines(content: &str, count: usize) -> &str {
    let lines: Vec<&str> = content.lines().collect();
    let Some(end) = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
        .take(count)
        .last()
        .map(|(i, _)| i)
    else {
        return "";
    };
    &content[..line_start(content, &lines, end + 1)]
}

/// Codex prompt line: `›` alone or `› …`.
fn prompt_line(line: &str) -> bool {
    line == "›" || line.starts_with("› ")
}

fn block_marker_line(line: &str) -> bool {
    line.starts_with(['•', '■', '✗', '✓'])
}

/// The last prompt line with no block marker after it.
fn current_prompt_index(lines: &[&str]) -> Option<usize> {
    let i = lines.iter().rposition(|l| prompt_line(l))?;
    (!lines[i + 1..].iter().any(|l| block_marker_line(l))).then_some(i)
}

fn after_last_prompt_marker(content: &str) -> &str {
    let lines: Vec<&str> = content.lines().collect();
    match lines.iter().rposition(|l| prompt_line(l)) {
        Some(i) => slice_from(content, &lines, i + 1),
        None => content,
    }
}

fn before_current_prompt_marker(content: &str) -> &str {
    let lines: Vec<&str> = content.lines().collect();
    match current_prompt_index(&lines) {
        Some(i) => &content[..line_start(content, &lines, i)],
        None => content,
    }
}

fn current_prompt_block_marker(content: &str) -> Option<&str> {
    let lines: Vec<&str> = content.lines().collect();
    let p = current_prompt_index(&lines)?;
    lines[..p]
        .iter()
        .rev()
        .find(|l| block_marker_line(l))
        .copied()
}

fn after_current_prompt_block_marker(content: &str) -> Option<&str> {
    let lines: Vec<&str> = content.lines().collect();
    let p = current_prompt_index(&lines)?;
    let b = lines[..p].iter().rposition(|l| block_marker_line(l))?;
    Some(slice_from(content, &lines, b))
}

/// Claude's input box: between the second-to-last and the last horizontal rule.
fn prompt_box_top(lines: &[&str]) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, l)| horizontal_rule(l))
        .nth(1)
        .map(|(i, _)| i)
}

fn prompt_box_body(content: &str) -> Option<&str> {
    let lines: Vec<&str> = content.lines().collect();
    let top = prompt_box_top(&lines)?;
    let end = lines[top + 1..]
        .iter()
        .position(|l| horizontal_rule(l))
        .map_or(lines.len(), |r| top + 1 + r);
    Some(&content[line_start(content, &lines, top + 1)..line_start(content, &lines, end)])
}

fn above_prompt_box(content: &str) -> &str {
    let lines: Vec<&str> = content.lines().collect();
    match prompt_box_top(&lines) {
        Some(top) => &content[..line_start(content, &lines, top)],
        None => content,
    }
}

fn after_last_horizontal_rule(content: &str) -> &str {
    let (mut end, mut offset) = (0, 0);
    for line in content.lines() {
        offset += line.len() + 1;
        if horizontal_rule(line) {
            end = offset.min(content.len());
        }
    }
    &content[end..]
}

fn last_non_empty_line(content: &str) -> &str {
    content
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
}

/// A line of `─`, optionally followed by a label once there are at least three.
fn horizontal_rule(line: &str) -> bool {
    let t = line.trim();
    let n = t.chars().take_while(|&c| c == '─').count();
    if n == 0 {
        return false;
    }
    let rest = t[n * '─'.len_utf8()..].trim_start();
    rest.is_empty() || n >= 3
}

/// Byte offset of line `index`; lines are `\n`-terminated.
fn line_start(content: &str, lines: &[&str], index: usize) -> usize {
    lines[..index.min(lines.len())]
        .iter()
        .map(|l| l.len() + 1)
        .sum::<usize>()
        .min(content.len())
}

fn slice_from<'a>(content: &'a str, lines: &[&str], index: usize) -> &'a str {
    &content[line_start(content, lines, index)..]
}

// ---- smoothing (herdr `agent_detection.rs`) ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Published {
    state: State,
    visible_idle: bool,
    visible_blocker: bool,
    visible_working: bool,
}

/// Turns readings into published states: nothing during the startup grace, skip rules keep the previous state, and
/// a working → plain idle change waits for three more idle readings or 700 ms (an agent between two steps).
#[derive(Debug)]
pub struct Smoother {
    started: Instant,
    published: Option<Published>,
    pending_since: Option<Instant>,
    confirmations: u8,
}

impl Smoother {
    pub fn new(started: Instant) -> Self {
        Self {
            started,
            published: None,
            pending_since: None,
            confirmations: 0,
        }
    }

    /// Confirming an idle: poll at [`PENDING_POLL`].
    pub fn pending(&self) -> bool {
        self.pending_since.is_some()
    }

    /// Something was published; before that every reading counts, even of an unchanged screen.
    pub fn published(&self) -> bool {
        self.published.is_some()
    }

    /// The state to publish now, if it changed.
    pub fn update(&mut self, d: &Detection, now: Instant) -> Option<State> {
        if now.duration_since(self.started) < STARTUP_GRACE || d.skip {
            return None;
        }
        let next = Published {
            state: d.state,
            visible_idle: d.visible_idle,
            visible_blocker: d.visible_blocker,
            visible_working: d.visible_working,
        };
        if let Some(prev) = self.published
            && self.hold_working_to_idle(prev, next, now)
        {
            return None;
        }
        let changed = self.published.is_none_or(|p| p.state != next.state);
        self.published = Some(next);
        changed.then_some(next.state)
    }

    fn hold_working_to_idle(&mut self, prev: Published, next: Published, now: Instant) -> bool {
        let plain = prev.state == State::Working
            && next.state == State::Idle
            && !next.visible_idle
            && !next.visible_blocker;
        if !plain {
            self.pending_since = None;
            self.confirmations = 0;
            return false;
        }
        let Some(since) = self.pending_since else {
            self.pending_since = Some(now);
            self.confirmations = 0;
            return true;
        };
        self.confirmations = self.confirmations.saturating_add(1);
        if now.duration_since(since) >= PENDING_IDLE_CAP
            || self.confirmations >= PENDING_IDLE_CONFIRMATIONS
        {
            self.pending_since = None;
            self.confirmations = 0;
            return false;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(rules: &str) -> Manifest {
        Manifest::parse(&format!("id = \"codex\"\n\n{rules}")).unwrap()
    }

    fn screen(text: &str) -> Input<'_> {
        Input {
            screen: text,
            osc_title: "",
            osc_progress: "",
        }
    }

    #[test]
    fn all_bundled_manifests_parse_and_validate() {
        assert_eq!(bundled().len(), BUNDLED.len());
        for name in [
            "claude-code",
            "codex",
            "gemini",
            "opencode",
            "cursor",
            "qwen",
            "antigravity",
        ] {
            assert!(manifest(name).is_some(), "{name}");
        }
        assert!(manifest("nope").is_none());
    }

    #[test]
    fn known_agent_no_match_defaults_to_idle_fallback() {
        let m = rules(
            "[[rules]]\nid = \"test\"\nstate = \"working\"\ncontains = [\"active-marker\"]\n",
        );
        let d = m.detect(screen("unmatched-marker"));
        assert_eq!(
            (d.state, d.rule, d.visible_idle),
            (State::Idle, None, false)
        );
    }

    #[test]
    fn rule_semantics_apply_gates_priority_and_line_regex() {
        let m = rules(
            r#"
[[rules]]
id = "low_contains"
state = "idle"
priority = 1
contains = ["match"]

[[rules]]
id = "high_nested_gates"
state = "working"
priority = 10
contains = ["match"]
all = [
  { any = [{ regex = ["w[io]n"] }, { contains = ["fallback"] }] },
]
not = [
  { contains = ["blocked"] },
]

[[rules]]
id = "line_regex"
state = "blocked"
priority = 20
line_regex = ["^exact line$"]
"#,
        );
        let id = |t| m.detect(screen(t)).rule;
        assert_eq!(m.detect(screen("match win")).state, State::Working);
        assert_eq!(id("match win").as_deref(), Some("high_nested_gates"));
        assert_eq!(id("match win blocked").as_deref(), Some("low_contains"));
        assert_eq!(
            id("MATCH WIN").as_deref(),
            Some("low_contains"),
            "contains ignores case, regex does not"
        );
        let line = m.detect(screen("before\nexact line\nafter"));
        assert_eq!(
            (line.state, line.rule.as_deref()),
            (State::Blocked, Some("line_regex"))
        );
    }

    #[test]
    fn paired_alternatives_require_complete_gates_at_bottom_buffer() {
        let m = rules(
            r#"
[[rules]]
id = "paired"
state = "blocked"
region = "bottom_non_empty_lines(3)"
visible_blocker = true
any = [
  { all = [
    { contains = ["alpha heading"] },
    { regex = ['alpha\s+control\s*\z'] },
  ] },
  { all = [
    { contains = ["beta heading"] },
    { regex = ['beta\s+control\s*\z'] },
  ] },
]
"#,
        );
        for text in [
            "alpha heading\nalpha control",
            "beta heading\nbeta\ncontrol",
        ] {
            let d = m.detect(screen(text));
            assert!(d.state == State::Blocked && d.visible_blocker, "{text}");
        }
        for text in [
            "alpha heading\nbeta control",
            "beta heading\nalpha control",
            "beta control",
            "beta heading",
            "beta heading\nbeta control\nnewer content",
            "beta heading\nfiller one\nfiller two\nbeta control",
        ] {
            let d = m.detect(screen(text));
            assert!(d.rule.is_none() && !d.visible_blocker, "{text}");
        }
    }

    #[test]
    fn osc_regions_use_separate_inputs_and_share_rule_priority() {
        let m = rules(
            r#"
[[rules]]
id = "screen"
state = "idle"
priority = 10
visible_idle = true
contains = ["screen-marker"]

[[rules]]
id = "title"
state = "working"
priority = 20
region = "osc_title"
visible_working = true
regex = ['^title-marker$']

[[rules]]
id = "progress"
state = "blocked"
priority = 30
region = "osc_progress"
visible_blocker = true
regex = ['^progress-marker$']
"#,
        );
        for (screen, osc_title, osc_progress, state, rule) in [
            ("screen-marker", "", "", State::Idle, "screen"),
            ("screen-marker", "title-marker", "", State::Working, "title"),
            (
                "screen-marker",
                "title-marker",
                "progress-marker",
                State::Blocked,
                "progress",
            ),
            (
                "screen-marker title-marker progress-marker",
                "",
                "",
                State::Idle,
                "screen",
            ),
        ] {
            let d = m.detect(Input {
                screen,
                osc_title,
                osc_progress,
            });
            assert_eq!((d.state, d.rule.as_deref()), (state, Some(rule)));
            assert_eq!(d.visible_idle, state == State::Idle);
            assert_eq!(d.visible_working, state == State::Working);
            assert_eq!(d.visible_blocker, state == State::Blocked);
        }
        let swapped = m.detect(Input {
            screen: "",
            osc_title: "progress-marker",
            osc_progress: "title-marker",
        });
        assert!(swapped.rule.is_none());
    }

    #[test]
    fn skip_rule_suppresses_state_update_without_visible_state_evidence() {
        let m = rules(
            r#"
[[rules]]
id = "activity"
state = "working"
priority = 10
visible_working = true
contains = ["activity-marker"]

[[rules]]
id = "overlay"
state = "unknown"
priority = 20
skip_state_update = true
contains = ["overlay-marker"]
"#,
        );
        let d = m.detect(screen("activity-marker overlay-marker"));
        assert_eq!(
            (d.state, d.skip, d.rule.as_deref()),
            (State::Unknown, true, Some("overlay"))
        );
        assert!(!d.visible_idle && !d.visible_working && !d.visible_blocker);
    }

    #[test]
    fn screen_regions_extract_structure_without_classifying_agent_state() {
        for (text, spec, expected) in [
            ("old\n\nnew\n", "bottom_lines(2)", "\nnew\n"),
            (
                "before\n› input\nafter\n",
                "after_last_prompt_marker",
                "after\n",
            ),
            (
                "before\n› input\nafter\n",
                "before_current_prompt_marker",
                "before\n",
            ),
            (
                "before\n› input\nafter\n",
                "whole_recent_without_current_prompt_marker",
                "",
            ),
            (
                "no marker\n",
                "whole_recent_without_current_prompt_marker",
                "no marker\n",
            ),
            (
                "• old\n■ latest\n› input\n",
                "current_prompt_block_marker",
                "■ latest",
            ),
            (
                "• old\n■ latest\n› input\n",
                "after_current_prompt_block_marker",
                "■ latest\n› input\n",
            ),
            ("› old\n• new\n", "current_prompt_block_marker", ""),
            (
                "above\n\n───\nbody\n───\nfooter\n",
                "above_prompt_box",
                "above\n\n",
            ),
            (
                "above\n\n───\nbody\n───\nfooter\n",
                "last_non_empty_above_prompt_box",
                "above",
            ),
            (
                "above\n───\nbody\n───\nfooter\n",
                "prompt_box_body",
                "body\n",
            ),
            (
                "above\n───\nbody\n───\nfooter\n",
                "after_last_horizontal_rule",
                "footer\n",
            ),
            (
                "marker\nold\n\nmiddle\nmarker\nnew\n",
                "bottom_non_empty_lines(2)",
                "marker\nnew\n",
            ),
            (
                "\nmarker\nold\n\nmiddle\nmarker\nnew\n",
                "top_non_empty_lines(2)",
                "\nmarker\nold\n",
            ),
        ] {
            assert_eq!(region(screen(text), spec), expected, "region={spec}");
        }
    }

    #[test]
    fn validation_rejects_bad_manifests() {
        let bad = |rules: &str| Manifest::parse(&format!("id = \"codex\"\n\n{rules}")).is_err();
        assert!(bad(
            "[[rules]]\nid = \"typo\"\nstate = \"working\"\ncontain = [\"Working\"]\n"
        ));
        assert!(bad("[[rules]]\nid = \"empty\"\nstate = \"working\"\n"));
        assert!(bad(
            "[[rules]]\nid = \"r\"\nstate = \"working\"\nregion = \"after_last_promt_marker\"\ncontains = [\"x\"]\n"
        ));
        assert!(bad(
            "[[rules]]\nid = \"r\"\nstate = \"working\"\nregex = [\"[\"]\n"
        ));
        assert!(bad(
            "[[rules]]\nid = \"r\"\nstate = \"working\"\nany = [{ line_regex = [\"[\"] }]\n"
        ));
        // Skip rules stay neutral.
        assert!(bad(
            "[[rules]]\nid = \"r\"\nstate = \"idle\"\nskip_state_update = true\ncontains = [\"menu\"]\n"
        ));
        assert!(bad(
            "[[rules]]\nid = \"r\"\nstate = \"unknown\"\nskip_state_update = true\nvisible_blocker = true\ncontains = [\"menu\"]\n"
        ));
        assert!(Manifest::parse("id = \"codex\"\n").is_err(), "no rules");
        assert!(
            Manifest::parse("id = \"x\"\nmin_engine_version = 4\n[[rules]]\nid = \"r\"\nstate = \"idle\"\ncontains = [\"a\"]\n")
                .is_err(),
            "newer engine"
        );
        assert!(
            Manifest::parse(
                "id = \"x\"\nmin_engine_version = 2\n[[rules]]\nid = \"r\"\nstate = \"working\"\nregion = \" top_non_empty_lines(1) \"\ncontains = [\"a\"]\n"
            )
            .is_err(),
            "top_non_empty_lines needs engine 3"
        );
    }

    #[test]
    fn validation_enforces_herdr_limits() {
        let mut many = String::from("id = \"codex\"\n");
        for i in 0..129 {
            many.push_str(&format!(
                "[[rules]]\nid = \"r{i}\"\nstate = \"idle\"\ncontains = [\"ready\"]\n"
            ));
        }
        assert!(Manifest::parse(&many).is_err(), "rule count");
        let mut deep = String::from("{ contains = [\"9\"] }");
        for d in (1..=8).rev() {
            deep = format!("{{ contains = [\"{d}\"], all = [{deep}] }}");
        }
        assert!(
            Manifest::parse(&format!(
                "id = \"codex\"\n[[rules]]\nid = \"deep\"\nstate = \"idle\"\ncontains = [\"ready\"]\nall = [{deep}]\n"
            ))
            .is_err(),
            "depth"
        );
        let matchers = (0..33)
            .map(|i| format!("\"m{i}\""))
            .collect::<Vec<_>>()
            .join(", ");
        assert!(
            Manifest::parse(&format!(
                "id = \"codex\"\n[[rules]]\nid = \"many\"\nstate = \"idle\"\ncontains = [{matchers}]\n"
            ))
            .is_err(),
            "matchers per gate"
        );
        let long = "x".repeat(MAX_MATCHER_CHARS + 1);
        assert!(
            Manifest::parse(&format!(
                "id = \"codex\"\n[[rules]]\nid = \"r\"\nstate = \"idle\"\ncontains = [\"{long}\"]\n"
            ))
            .is_err(),
            "matcher length"
        );
    }

    #[test]
    fn top_non_empty_lines_requires_a_canonical_positive_bounded_count() {
        assert!(validate_region_name("top_non_empty_lines(1)").is_ok());
        assert!(validate_region_name(&format!("top_non_empty_lines({})", u16::MAX)).is_ok());
        for n in ["0", "01", "+1", "65536", "999999999999999999999999"] {
            assert!(
                validate_region_name(&format!("top_non_empty_lines({n})")).is_err(),
                "{n}"
            );
        }
    }

    /// The bundled rules on screens drawn like the real CLIs (agentdeck's own examples, not herdr's).
    #[test]
    fn bundled_rules_read_typical_screens() {
        let gemini = manifest("gemini").unwrap();
        assert_eq!(
            gemini
                .detect(screen(
                    "╭──────╮\n│ Apply this change?\n│ ● 1. Yes, allow once\n╰──────╯\n"
                ))
                .state,
            State::Blocked
        );
        assert_eq!(
            gemini
                .detect(screen("⠏ Thinking (esc to cancel, 3s)\n"))
                .state,
            State::Working
        );
        assert_eq!(
            gemini.detect(screen("> Type your message\n")).state,
            State::Idle
        );
        let opencode = manifest("opencode").unwrap();
        assert_eq!(
            opencode
                .detect(screen("△ Permission required\nbash: rm -rf build\n"))
                .state,
            State::Blocked
        );
        assert_eq!(
            opencode
                .detect(screen("working…  esc to interrupt\n"))
                .state,
            State::Working
        );
    }

    fn reading(state: State) -> Detection {
        Detection {
            state,
            rule: None,
            skip: false,
            visible_idle: false,
            visible_blocker: state == State::Blocked,
            visible_working: false,
        }
    }

    #[test]
    fn smoother_waits_out_startup_and_holds_working_to_plain_idle() {
        let t0 = Instant::now();
        let ms = |n| t0 + STARTUP_GRACE + Duration::from_millis(n);
        let mut s = Smoother::new(t0);
        assert_eq!(
            s.update(&reading(State::Working), t0),
            None,
            "startup grace"
        );
        assert_eq!(
            s.update(&reading(State::Working), ms(0)),
            Some(State::Working)
        );
        assert_eq!(
            s.update(&reading(State::Working), ms(300)),
            None,
            "unchanged"
        );
        // Three more idle readings before idle is published.
        assert_eq!(s.update(&reading(State::Idle), ms(400)), None);
        assert!(s.pending());
        assert_eq!(s.update(&reading(State::Idle), ms(500)), None);
        assert_eq!(s.update(&reading(State::Idle), ms(600)), None);
        assert_eq!(s.update(&reading(State::Idle), ms(700)), Some(State::Idle));
        assert!(!s.pending());
        // Working again in between resets the hold.
        assert_eq!(
            s.update(&reading(State::Working), ms(800)),
            Some(State::Working)
        );
        assert_eq!(s.update(&reading(State::Idle), ms(900)), None);
        assert_eq!(
            s.update(&reading(State::Working), ms(950)),
            None,
            "still working, no change"
        );
        assert!(!s.pending());
        // The 700 ms cap.
        assert_eq!(s.update(&reading(State::Idle), ms(1000)), None);
        assert_eq!(s.update(&reading(State::Idle), ms(1700)), Some(State::Idle));
    }

    #[test]
    fn smoother_publishes_blockers_and_visible_idle_at_once_and_skips_overlays() {
        let t0 = Instant::now();
        let at = t0 + STARTUP_GRACE;
        let mut s = Smoother::new(t0);
        assert_eq!(s.update(&reading(State::Working), at), Some(State::Working));
        assert_eq!(s.update(&reading(State::Blocked), at), Some(State::Blocked));
        assert_eq!(s.update(&reading(State::Working), at), Some(State::Working));
        let visible_idle = Detection {
            visible_idle: true,
            ..reading(State::Idle)
        };
        assert_eq!(s.update(&visible_idle, at), Some(State::Idle));
        let overlay = Detection {
            skip: true,
            ..reading(State::Unknown)
        };
        assert_eq!(s.update(&overlay, at), None);
    }
}
