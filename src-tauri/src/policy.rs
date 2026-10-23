//! Who may send to / read whom (ARCHITECTURE §5.4), spawn limits (PRD SPN-02), configurable limits and their safe
//! ranges (MCP-TOOLS "Batas bawaan"), the quota guard (D-033), and the loop guard (ARCHITECTURE §5.5). Pure logic: no
//! PTY, UI, or MCP.

use std::collections::VecDeque;
use std::fmt;
use std::ops::RangeInclusive;
use std::time::{Duration, Instant};

/// Fixed limits (MCP-TOOLS "Batas bawaan", not configurable).
pub const QUEUE_PER_TARGET: usize = 5;
pub const READS_PER_MINUTE: u32 = 30;
pub const REPORTS_PER_MINUTE: u32 = 6;
pub const SEND_MAX_BYTES: usize = 8192;
pub const TASK_MAX_BYTES: usize = 16 * 1024;
pub const SPAWN_WINDOW: Duration = Duration::from_secs(600);
/// Quota guard (D-033): `pane_spawn` is refused once the caller's Claude 5-hour usage reaches this percentage.
pub const QUOTA_GUARD_DEFAULT: u8 = 85;
pub const QUOTA_GUARD_RANGE: RangeInclusive<u8> = 50..=100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneKind {
    Agent,
    Shell,
}

#[derive(Clone, Debug)]
pub struct Pane {
    pub id: String,
    pub workspace: String,
    pub kind: PaneKind,
    /// Set when the pane was opened by an agent (`pane_spawn`); `None` when the person opened it.
    pub parent: Option<String>,
    pub exited: bool,
    /// Opened by the person with permissions bypassed (D-034): only its own children may send to it.
    pub bypass: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Relation {
    You,
    Parent,
    Child,
    Peer,
    Shell,
    OtherWorkspace,
}

impl Relation {
    /// Name used in `pane_list`. `OtherWorkspace` panes are never listed.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::You => "you",
            Self::Parent => "parent",
            Self::Child => "child",
            Self::Peer => "peer",
            Self::Shell => "shell",
            Self::OtherWorkspace => "other_workspace",
        }
    }
}

/// Tool errors decided by policy; `code()` is the MCP-TOOLS code, `Display` is the agent-facing text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PolicyError {
    UnknownPane,
    /// Carries the reason, since `pane_read` folds shell/peer/self refusals into this code.
    NotAllowed(&'static str),
    PeerSendOff,
    TargetShell,
    TargetExited,
    /// The target runs with permissions bypassed and takes messages only from its own children (D-034).
    TargetBypass,
    NoParent,
    SelfTarget,
    SpawnOff,
    Nested,
    CapChildren,
    CapGlobal,
    Rate {
        wait_secs: u64,
    },
    /// Caller's Claude 5-hour usage `used` % is at or above the guard `guard` % (D-033).
    QuotaLow {
        used: u8,
        guard: u8,
    },
}

impl PolicyError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnknownPane => "UNKNOWN_PANE",
            Self::NotAllowed(_) => "NOT_ALLOWED",
            Self::PeerSendOff => "PEER_SEND_OFF",
            Self::TargetShell => "TARGET_SHELL",
            Self::TargetExited => "TARGET_EXITED",
            Self::TargetBypass => "TARGET_BYPASS",
            Self::NoParent => "NO_PARENT",
            Self::SelfTarget => "SELF",
            Self::SpawnOff => "SPAWN_OFF",
            Self::Nested => "NESTED",
            Self::CapChildren => "CAP_CHILDREN",
            Self::CapGlobal => "CAP_GLOBAL",
            Self::Rate { .. } => "RATE",
            Self::QuotaLow { .. } => "QUOTA_LOW",
        }
    }
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: ", self.code())?;
        match self {
            Self::UnknownPane => f.write_str("no such pane. Check pane_list."),
            Self::NotAllowed(why) => write!(f, "{why}. Check pane_list, columns can_send and can_read."),
            Self::PeerSendOff => f.write_str(
                "the target is a peer and peer messaging is off in this workspace. Tell the person; do not look for another way.",
            ),
            Self::TargetShell => f.write_str("the target is a shell pane. Not supported."),
            Self::TargetExited => f.write_str(
                "the target agent has exited. Read its screen if allowed, and tell the person.",
            ),
            Self::TargetBypass => f.write_str(
                "the target runs with permissions bypassed and accepts messages only from its own children. Tell \
                 the person; do not look for another way.",
            ),
            Self::NoParent => f.write_str(
                "this pane has no parent (the person opened it). Print your result in your own pane.",
            ),
            Self::SelfTarget => f.write_str("that is your own pane. Not needed."),
            Self::SpawnOff => f.write_str(
                "agents may not open panes (switch off). Do the work yourself; the switch is in Settings.",
            ),
            Self::Nested => f.write_str(
                "this pane was itself opened by an agent. Do the work yourself or ask your parent.",
            ),
            Self::CapChildren => f.write_str(
                "this pane already has the maximum of live children. Wait for a child to finish and be closed by the person.",
            ),
            Self::CapGlobal => f.write_str(
                "the app already has the maximum of agent-opened panes. Ask the person to close unused panes.",
            ),
            Self::Rate { wait_secs } => write!(
                f,
                "spawn rate limit reached. Wait {wait_secs} seconds, or do the work yourself."
            ),
            Self::QuotaLow { used, guard } => write!(
                f,
                "your Claude 5-hour usage is at {used}% and agentdeck stops spawning at {guard}%, so a child could \
                 leave the work half done. Do the work yourself, or tell the person; spawning works again once the \
                 5-hour window resets."
            ),
        }
    }
}

impl std::error::Error for PolicyError {}

pub fn relation(from: &Pane, to: &Pane) -> Relation {
    if from.id == to.id {
        Relation::You
    } else if from.workspace != to.workspace {
        Relation::OtherWorkspace
    } else if to.kind == PaneKind::Shell {
        Relation::Shell
    } else if from.parent.as_deref() == Some(to.id.as_str()) {
        Relation::Parent
    } else if to.parent.as_deref() == Some(from.id.as_str()) {
        Relation::Child
    } else {
        Relation::Peer
    }
}

/// Turns the `to`/`pane` argument (`"p2"` or `"parent"`) into a pane. Does not check permission.
pub fn resolve_target<'a>(
    panes: &'a [Pane],
    from: &Pane,
    to: &str,
) -> Result<&'a Pane, PolicyError> {
    if to == "parent" {
        // A parent the person already closed counts as no parent.
        let parent = from.parent.as_deref().ok_or(PolicyError::NoParent)?;
        return panes
            .iter()
            .find(|p| p.id == parent)
            .ok_or(PolicyError::NoParent);
    }
    panes
        .iter()
        .find(|p| p.id == to)
        .ok_or(PolicyError::UnknownPane)
}

/// `pane_send` rule (§5.4). `peer_messaging` is the "Agen boleh saling kirim" switch of `from`'s workspace.
pub fn can_send(from: &Pane, to: &Pane, peer_messaging: bool) -> Result<(), PolicyError> {
    if from.kind == PaneKind::Shell {
        return Err(PolicyError::NotAllowed(
            "shell panes cannot use agentdeck tools",
        ));
    }
    match relation(from, to) {
        Relation::You => return Err(PolicyError::SelfTarget),
        Relation::OtherWorkspace => {
            return Err(PolicyError::NotAllowed(
                "the target is in another workspace",
            ));
        }
        Relation::Shell => return Err(PolicyError::TargetShell),
        Relation::Peer if !peer_messaging => return Err(PolicyError::PeerSendOff),
        Relation::Parent | Relation::Child | Relation::Peer => {}
    }
    // A bypassed pane runs whatever it is told without asking anyone, so another agent's text must not reach it; its
    // own children (approved by the person, never bypassed) may still report back (D-034).
    if to.bypass && relation(from, to) != Relation::Parent {
        return Err(PolicyError::TargetBypass);
    }
    if to.exited {
        return Err(PolicyError::TargetExited);
    }
    Ok(())
}

/// `pane_read` rule (§5.4): same relations as send, every refusal is `NOT_ALLOWED`; exited panes stay readable.
pub fn can_read(from: &Pane, to: &Pane, peer_messaging: bool) -> Result<(), PolicyError> {
    // Reading never makes the target act, so bypass does not limit it.
    let live = Pane {
        exited: false,
        bypass: false,
        ..to.clone()
    };
    can_send(from, &live, peer_messaging).map_err(|e| match e {
        PolicyError::SelfTarget => PolicyError::NotAllowed("that is your own pane"),
        PolicyError::TargetShell => PolicyError::NotAllowed("shell panes cannot be read"),
        PolicyError::PeerSendOff => {
            PolicyError::NotAllowed("the pane is a peer and peer messaging is off")
        }
        other => other,
    })
}

/// Configurable limits (MCP-TOOLS "Batas bawaan").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Limit {
    ChildrenPerPane,
    AgentOpenedGlobal,
    SpawnsPerWindow,
    ApprovalTimeoutSecs,
    LoopGuard,
}

impl Limit {
    pub const ALL: [Limit; 5] = [
        Self::ChildrenPerPane,
        Self::AgentOpenedGlobal,
        Self::SpawnsPerWindow,
        Self::ApprovalTimeoutSecs,
        Self::LoopGuard,
    ];

    pub fn default_value(self) -> u32 {
        match self {
            Self::ChildrenPerPane => 3,
            Self::AgentOpenedGlobal => 8,
            Self::SpawnsPerWindow => 5,
            Self::ApprovalTimeoutSecs => 120,
            Self::LoopGuard => 12,
        }
    }

    pub fn range(self) -> RangeInclusive<u32> {
        match self {
            Self::ChildrenPerPane => 1..=5,
            Self::AgentOpenedGlobal => 1..=12,
            Self::SpawnsPerWindow => 1..=10,
            Self::ApprovalTimeoutSecs => 30..=300,
            Self::LoopGuard => 2..=50,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutOfRange {
    pub limit: Limit,
    pub value: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Limits {
    values: [u32; 5],
    /// Percent, [`QUOTA_GUARD_RANGE`]. Not a [`Limit`]: Settings has no field for it yet (D-033).
    quota_guard: u8,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            values: Limit::ALL.map(Limit::default_value),
            quota_guard: QUOTA_GUARD_DEFAULT,
        }
    }
}

impl Limits {
    pub fn get(&self, limit: Limit) -> u32 {
        self.values[limit as usize]
    }

    /// Values outside the safe range are refused and the old value is kept.
    pub fn set(&mut self, limit: Limit, value: u32) -> Result<(), OutOfRange> {
        if !limit.range().contains(&value) {
            return Err(OutOfRange { limit, value });
        }
        self.values[limit as usize] = value;
        Ok(())
    }

    pub fn quota_guard(&self) -> u8 {
        self.quota_guard
    }

    /// Outside [`QUOTA_GUARD_RANGE`] the value is refused (returned) and the old one kept.
    pub fn set_quota_guard(&mut self, percent: u8) -> Result<(), u8> {
        if !QUOTA_GUARD_RANGE.contains(&percent) {
            return Err(percent);
        }
        self.quota_guard = percent;
        Ok(())
    }
}

/// Sliding 10-minute window of spawns. Time is passed in, so tests use a fake clock.
#[derive(Debug, Default)]
pub struct SpawnRate {
    times: VecDeque<Instant>,
}

impl SpawnRate {
    fn prune(&mut self, now: Instant) {
        while self
            .times
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) >= SPAWN_WINDOW)
        {
            self.times.pop_front();
        }
    }

    /// `Err(seconds to wait)` when `max` spawns already happened in the window.
    pub fn check(&mut self, now: Instant, max: u32) -> Result<(), u64> {
        self.prune(now);
        if self.times.len() < max as usize {
            return Ok(());
        }
        // Slot frees when the `len - max + 1`-th oldest entry leaves the window (handles a lowered `max`).
        let idx = self.times.len() - max as usize;
        let left = self.times.get(idx).map_or(Duration::ZERO, |t| {
            (*t + SPAWN_WINDOW).saturating_duration_since(now)
        });
        Err(left.as_secs() + u64::from(left.subsec_nanos() > 0))
    }

    pub fn record(&mut self, now: Instant) {
        self.prune(now);
        self.times.push_back(now);
    }
}

/// `pane_spawn` checks in the order of MCP-TOOLS. On `Ok`, call `rate.record(now)` once the request is accepted.
pub fn check_spawn(
    caller: &Pane,
    panes: &[Pane],
    limits: &Limits,
    rate: &mut SpawnRate,
    now: Instant,
    spawn_allowed: bool,
) -> Result<(), PolicyError> {
    if caller.kind == PaneKind::Shell {
        return Err(PolicyError::NotAllowed(
            "shell panes cannot use agentdeck tools",
        ));
    }
    if !spawn_allowed {
        return Err(PolicyError::SpawnOff);
    }
    if caller.parent.is_some() {
        return Err(PolicyError::Nested);
    }
    let live = |p: &&Pane| !p.exited && p.kind == PaneKind::Agent;
    let children = panes
        .iter()
        .filter(live)
        .filter(|p| p.parent.as_deref() == Some(caller.id.as_str()))
        .count();
    if children >= limits.get(Limit::ChildrenPerPane) as usize {
        return Err(PolicyError::CapChildren);
    }
    let agent_opened = panes
        .iter()
        .filter(live)
        .filter(|p| p.parent.is_some())
        .count();
    if agent_opened >= limits.get(Limit::AgentOpenedGlobal) as usize {
        return Err(PolicyError::CapGlobal);
    }
    rate.check(now, limits.get(Limit::SpawnsPerWindow))
        .map_err(|wait_secs| PolicyError::Rate { wait_secs })
}

/// Quota guard of `pane_spawn` (D-033). `five_hour_used`: the caller's Claude 5-hour usage in percent; `None` when
/// there is no reading (Codex, or Claude before its first turn), which is never refused.
pub fn check_quota(five_hour_used: Option<u8>, guard: u8) -> Result<(), PolicyError> {
    match five_hour_used {
        Some(used) if used >= guard => Err(PolicyError::QuotaLow { used, guard }),
        _ => Ok(()),
    }
}

/// Loop guard of one workspace (§5.5).
#[derive(Debug, Default)]
pub struct LoopGuard {
    count: u32,
    paused: bool,
}

impl LoopGuard {
    /// Call after each agent message is delivered. Returns `true` when the workspace is now paused.
    pub fn record_delivered(&mut self, limit: u32) -> bool {
        self.count = self.count.saturating_add(1);
        if self.count >= limit {
            self.paused = true;
        }
        self.paused
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    /// The person pressed Enter in any pane of the workspace, or chose "Continue" / "Stop" on the card.
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(id: &str, ws: &str, parent: Option<&str>) -> Pane {
        Pane {
            id: id.into(),
            workspace: ws.into(),
            kind: PaneKind::Agent,
            parent: parent.map(Into::into),
            exited: false,
            bypass: false,
        }
    }

    fn shell(id: &str, ws: &str) -> Pane {
        Pane {
            kind: PaneKind::Shell,
            ..agent(id, ws, None)
        }
    }

    // ---- §5.4, one test per row ----

    #[test]
    fn parent_to_own_child() {
        let (p, c) = (agent("p1", "w", None), agent("p2", "w", Some("p1")));
        assert_eq!(relation(&p, &c), Relation::Child);
        assert_eq!(can_send(&p, &c, false), Ok(()));
        assert_eq!(can_read(&p, &c, false), Ok(()));
    }

    #[test]
    fn child_to_parent() {
        let (p, c) = (agent("p1", "w", None), agent("p2", "w", Some("p1")));
        let panes = [p.clone(), c.clone()];
        assert_eq!(relation(&c, &p), Relation::Parent);
        assert_eq!(
            resolve_target(&panes, &c, "parent").map(|p| p.id.as_str()),
            Ok("p1")
        );
        assert_eq!(can_send(&c, &p, false), Ok(()));
        assert_eq!(can_read(&c, &p, false), Ok(()));
    }

    #[test]
    fn peer_needs_switch() {
        let (a, b) = (agent("p1", "w", None), agent("p2", "w", None));
        assert_eq!(relation(&a, &b), Relation::Peer);
        assert_eq!(can_send(&a, &b, false), Err(PolicyError::PeerSendOff));
        assert_eq!(
            can_read(&a, &b, false).map_err(|e| e.code()),
            Err("NOT_ALLOWED")
        );
        assert_eq!(can_send(&a, &b, true), Ok(()));
        assert_eq!(can_read(&a, &b, true), Ok(()));
        // Siblings (two children of one parent) are peers too.
        let (s1, s2) = (agent("p3", "w", Some("p1")), agent("p4", "w", Some("p1")));
        assert_eq!(relation(&s1, &s2), Relation::Peer);
        assert_eq!(can_send(&s1, &s2, false), Err(PolicyError::PeerSendOff));
    }

    #[test]
    fn shell_target_refused() {
        let (a, s) = (agent("p1", "w", None), shell("p2", "w"));
        assert_eq!(relation(&a, &s), Relation::Shell);
        for peer in [false, true] {
            assert_eq!(can_send(&a, &s, peer), Err(PolicyError::TargetShell));
            assert_eq!(
                can_read(&a, &s, peer).map_err(|e| e.code()),
                Err("NOT_ALLOWED")
            );
        }
        assert_eq!(
            can_send(&s, &a, true).map_err(|e| e.code()),
            Err("NOT_ALLOWED")
        );
    }

    #[test]
    fn other_workspace_refused() {
        let (a, b) = (agent("p1", "w1", None), agent("p2", "w2", Some("p1")));
        assert_eq!(relation(&a, &b), Relation::OtherWorkspace);
        for peer in [false, true] {
            assert_eq!(
                can_send(&a, &b, peer).map_err(|e| e.code()),
                Err("NOT_ALLOWED")
            );
            assert_eq!(
                can_read(&a, &b, peer).map_err(|e| e.code()),
                Err("NOT_ALLOWED")
            );
        }
    }

    #[test]
    fn self_refused() {
        let a = agent("p1", "w", None);
        assert_eq!(relation(&a, &a), Relation::You);
        assert_eq!(can_send(&a, &a, true), Err(PolicyError::SelfTarget));
        assert_eq!(
            can_read(&a, &a, true).map_err(|e| e.code()),
            Err("NOT_ALLOWED")
        );
    }

    /// D-034: peers (even with peer messaging on) and the pane's own parent are refused; its children still report;
    /// reading is unaffected.
    #[test]
    fn bypassed_target_takes_only_its_childrens_messages() {
        let b = Pane {
            bypass: true,
            ..agent("p1", "w", None)
        };
        let peer = agent("p2", "w", None);
        let child = agent("p3", "w", Some("p1"));
        assert_eq!(can_send(&peer, &b, true), Err(PolicyError::TargetBypass));
        assert_eq!(can_send(&peer, &b, false), Err(PolicyError::PeerSendOff));
        assert_eq!(can_send(&child, &b, false), Ok(()));
        assert_eq!(can_read(&peer, &b, true), Ok(()));
        assert_eq!(PolicyError::TargetBypass.code(), "TARGET_BYPASS");
        // A bypassed pane opened by an agent cannot exist, but if it did its parent would be refused too.
        let bypassed_child = Pane {
            bypass: true,
            ..child.clone()
        };
        let parent = agent("p1", "w", None);
        assert_eq!(
            can_send(&parent, &bypassed_child, false),
            Err(PolicyError::TargetBypass)
        );
    }

    #[test]
    fn exited_target_send_refused_read_allowed() {
        let p = agent("p1", "w", None);
        let c = Pane {
            exited: true,
            ..agent("p2", "w", Some("p1"))
        };
        assert_eq!(can_send(&p, &c, false), Err(PolicyError::TargetExited));
        assert_eq!(can_read(&p, &c, false), Ok(()));
    }

    #[test]
    fn resolve_target_errors() {
        let a = agent("p1", "w", None);
        let orphan = agent("p2", "w", Some("gone"));
        let panes = [a.clone(), orphan.clone()];
        assert_eq!(
            resolve_target(&panes, &a, "parent").err(),
            Some(PolicyError::NoParent)
        );
        assert_eq!(
            resolve_target(&panes, &orphan, "parent").err(),
            Some(PolicyError::NoParent)
        );
        assert_eq!(
            resolve_target(&panes, &a, "p9").err(),
            Some(PolicyError::UnknownPane)
        );
    }

    // ---- spawn ----

    fn spawn(caller: &Pane, panes: &[Pane]) -> Result<(), PolicyError> {
        check_spawn(
            caller,
            panes,
            &Limits::default(),
            &mut SpawnRate::default(),
            Instant::now(),
            true,
        )
    }

    #[test]
    fn spawn_off_and_shell() {
        let a = agent("p1", "w", None);
        let r = check_spawn(
            &a,
            &[],
            &Limits::default(),
            &mut SpawnRate::default(),
            Instant::now(),
            false,
        );
        assert_eq!(r, Err(PolicyError::SpawnOff));
        assert_eq!(
            spawn(&shell("p2", "w"), &[]).map_err(|e| e.code()),
            Err("NOT_ALLOWED")
        );
    }

    #[test]
    fn spawn_nested() {
        assert_eq!(
            spawn(&agent("p2", "w", Some("p1")), &[]),
            Err(PolicyError::Nested)
        );
    }

    #[test]
    fn spawn_cap_children_counts_live_only() {
        let a = agent("p1", "w", None);
        let mut panes: Vec<Pane> = (0..3)
            .map(|i| agent(&format!("c{i}"), "w", Some("p1")))
            .collect();
        assert_eq!(spawn(&a, &panes), Err(PolicyError::CapChildren));
        panes[0].exited = true;
        assert_eq!(spawn(&a, &panes), Ok(()));
    }

    #[test]
    fn spawn_cap_global_across_workspaces() {
        let a = agent("p1", "w", None);
        // 8 live agent-opened panes, each parent has ≤ 2 children so CAP_CHILDREN does not fire first.
        let mut panes: Vec<Pane> = (0..8)
            .map(|i| {
                agent(
                    &format!("c{i}"),
                    &format!("w{}", i % 4),
                    Some(&format!("x{}", i % 4)),
                )
            })
            .collect();
        panes.push(shell("s", "w"));
        assert_eq!(spawn(&a, &panes), Err(PolicyError::CapGlobal));
        panes[0].exited = true;
        assert_eq!(spawn(&a, &panes), Ok(()));
    }

    #[test]
    fn spawn_rate_with_fake_clock() {
        let a = agent("p1", "w", None);
        let limits = Limits::default();
        let mut rate = SpawnRate::default();
        let t0 = Instant::now();
        for i in 0..5 {
            let now = t0 + Duration::from_secs(i * 60);
            assert_eq!(check_spawn(&a, &[], &limits, &mut rate, now, true), Ok(()));
            rate.record(now);
        }
        // Oldest spawn was at t0, so the slot frees at t0 + 600.
        let now = t0 + Duration::from_secs(300);
        assert_eq!(
            check_spawn(&a, &[], &limits, &mut rate, now, true),
            Err(PolicyError::Rate { wait_secs: 300 })
        );
        let now = t0 + Duration::from_millis(599_500);
        assert_eq!(rate.check(now, 5), Err(1));
        assert_eq!(rate.check(t0 + SPAWN_WINDOW, 5), Ok(()));
    }

    #[test]
    fn spawn_rate_after_lowering_limit() {
        let mut rate = SpawnRate::default();
        let t0 = Instant::now();
        for i in 0..5 {
            rate.record(t0 + Duration::from_secs(i * 10));
        }
        // max 3: three of five must leave; the third-oldest (t0+20) frees the slot.
        assert_eq!(rate.check(t0 + Duration::from_secs(100), 3), Err(520));
    }

    #[test]
    fn rate_error_mentions_wait() {
        let e = PolicyError::Rate { wait_secs: 42 };
        assert!(e.to_string().starts_with("RATE: "));
        assert!(e.to_string().contains("42 seconds"));
    }

    // ---- limits ----

    #[test]
    fn limit_defaults_and_ranges() {
        let expected = [
            (3, 1, 5),
            (8, 1, 12),
            (5, 1, 10),
            (120, 30, 300),
            (12, 2, 50),
        ];
        let mut limits = Limits::default();
        for (limit, (def, lo, hi)) in Limit::ALL.into_iter().zip(expected) {
            assert_eq!(limits.get(limit), def, "{limit:?}");
            assert_eq!(limit.range(), lo..=hi, "{limit:?}");
            assert_eq!(
                limits.set(limit, lo - 1),
                Err(OutOfRange {
                    limit,
                    value: lo - 1
                })
            );
            assert_eq!(
                limits.set(limit, hi + 1),
                Err(OutOfRange {
                    limit,
                    value: hi + 1
                })
            );
            assert_eq!(limits.get(limit), def, "refused value must not stick");
            assert_eq!(limits.set(limit, lo), Ok(()));
            assert_eq!(limits.set(limit, hi), Ok(()));
            assert_eq!(limits.get(limit), hi);
        }
    }

    #[test]
    fn fixed_limits() {
        assert_eq!(QUEUE_PER_TARGET, 5);
        assert_eq!(READS_PER_MINUTE, 30);
        assert_eq!(REPORTS_PER_MINUTE, 6);
        assert_eq!(SEND_MAX_BYTES, 8192);
        assert_eq!(TASK_MAX_BYTES, 16384);
        assert_eq!(SPAWN_WINDOW, Duration::from_secs(600));
    }

    #[test]
    fn configured_limit_applies_to_spawn() {
        let a = agent("p1", "w", None);
        let mut limits = Limits::default();
        assert_eq!(limits.set(Limit::ChildrenPerPane, 1), Ok(()));
        let panes = [agent("c", "w", Some("p1"))];
        let r = check_spawn(
            &a,
            &panes,
            &limits,
            &mut SpawnRate::default(),
            Instant::now(),
            true,
        );
        assert_eq!(r, Err(PolicyError::CapChildren));
    }

    #[test]
    fn quota_guard_refuses_at_threshold_only_with_a_reading() {
        let limits = Limits::default();
        let guard = limits.quota_guard();
        assert_eq!(guard, 85);
        assert_eq!(
            check_quota(None, guard),
            Ok(()),
            "no reading: never refused"
        );
        assert_eq!(check_quota(Some(84), guard), Ok(()));
        assert_eq!(
            check_quota(Some(85), guard),
            Err(PolicyError::QuotaLow {
                used: 85,
                guard: 85
            })
        );
        assert_eq!(
            check_quota(Some(100), guard).map_err(|e| e.code()),
            Err("QUOTA_LOW")
        );
        let e = PolicyError::QuotaLow {
            used: 91,
            guard: 85,
        }
        .to_string();
        assert!(e.contains("91%") && e.contains("85%"), "{e}");
    }

    #[test]
    fn quota_guard_range() {
        let mut limits = Limits::default();
        assert_eq!(QUOTA_GUARD_RANGE, 50..=100);
        assert_eq!(limits.set_quota_guard(49), Err(49));
        assert_eq!(limits.set_quota_guard(101), Err(101));
        assert_eq!(limits.quota_guard(), 85, "refused value must not stick");
        assert_eq!(limits.set_quota_guard(50), Ok(()));
        assert_eq!(
            check_quota(Some(50), limits.quota_guard()).map_err(|e| e.code()),
            Err("QUOTA_LOW")
        );
        assert_eq!(limits.set_quota_guard(100), Ok(()));
        assert_eq!(check_quota(Some(99), limits.quota_guard()), Ok(()));
        // The other limits are untouched by the guard.
        assert_eq!(limits.get(Limit::ChildrenPerPane), 3);
    }

    // ---- loop guard ----

    #[test]
    fn loop_guard_pauses_at_limit_and_resets() {
        let mut g = LoopGuard::default();
        for _ in 0..11 {
            assert!(!g.record_delivered(12));
        }
        assert!(g.record_delivered(12));
        assert!(g.is_paused());
        assert_eq!(g.count(), 12);
        g.reset();
        assert!(!g.is_paused());
        assert_eq!(g.count(), 0);
        assert!(!g.record_delivered(12));
    }

    #[test]
    fn loop_guard_reset_midway() {
        let mut g = LoopGuard::default();
        for _ in 0..5 {
            g.record_delivered(2);
        }
        assert!(g.is_paused());
        g.reset();
        assert!(!g.record_delivered(2));
        assert!(g.record_delivered(2));
    }

    #[test]
    fn error_codes() {
        let all = [
            (PolicyError::UnknownPane, "UNKNOWN_PANE"),
            (PolicyError::NotAllowed("x"), "NOT_ALLOWED"),
            (PolicyError::PeerSendOff, "PEER_SEND_OFF"),
            (PolicyError::TargetShell, "TARGET_SHELL"),
            (PolicyError::TargetExited, "TARGET_EXITED"),
            (PolicyError::NoParent, "NO_PARENT"),
            (PolicyError::SelfTarget, "SELF"),
            (PolicyError::SpawnOff, "SPAWN_OFF"),
            (PolicyError::Nested, "NESTED"),
            (PolicyError::CapChildren, "CAP_CHILDREN"),
            (PolicyError::CapGlobal, "CAP_GLOBAL"),
            (PolicyError::Rate { wait_secs: 1 }, "RATE"),
            (
                PolicyError::QuotaLow {
                    used: 90,
                    guard: 85,
                },
                "QUOTA_LOW",
            ),
        ];
        for (e, code) in all {
            assert_eq!(e.code(), code);
            assert!(e.to_string().starts_with(&format!("{code}: ")));
        }
    }
}
