//! Spawn approval cards (SPN-01, UI-UX §4.2): a request waits until the person answers its card, its time runs out,
//! or the requesting pane goes away. Requests stack; each has its own id and the UI shows them oldest first.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;

/// How often a waiting request checks that its requester is still there.
const ALIVE_POLL: Duration = Duration::from_millis(250);

/// What the requesting agent asks for. Agent names are display names ("Claude Code"), shown as they are.
#[derive(Clone, Debug)]
pub struct Request {
    pub pane: String,
    pub requester_agent: String,
    pub agent: String,
    pub title: String,
    pub cwd: String,
    pub task: String,
}

/// A pending card as the UI gets it (`agentdeck://approval`, `approvals_pending`). Times are ms since the epoch.
#[derive(Clone, Debug, serde::Serialize, PartialEq)]
pub struct Approval {
    pub id: String,
    pub pane: String,
    pub requester_agent: String,
    pub agent: String,
    pub title: String,
    pub cwd: String,
    pub task: String,
    pub created_at: u64,
    pub expires_at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Approved,
    Denied,
    Timeout,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Notice {
    /// A new card.
    Opened(Approval),
    /// The card is gone, once per request: answered, timed out, requester gone, or the request was dropped.
    Closed { id: String, outcome: Outcome },
}

pub type Notify = Arc<dyn Fn(Notice) + Send + Sync>;

struct Pending {
    approval: Approval,
    answer: oneshot::Sender<bool>,
}

#[derive(Default)]
struct Inner {
    next: u64,
    /// Oldest first.
    pending: Vec<Pending>,
}

#[derive(Clone)]
pub struct Approvals {
    inner: Arc<Mutex<Inner>>,
    notify: Notify,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// Removes the card when the request ends, however it ends (also when the caller drops the future), and tells the UI.
struct Ending<'a> {
    approvals: &'a Approvals,
    id: String,
    outcome: Outcome,
}

impl Drop for Ending<'_> {
    fn drop(&mut self) {
        self.approvals
            .lock()
            .pending
            .retain(|p| p.approval.id != self.id);
        (self.approvals.notify)(Notice::Closed {
            id: std::mem::take(&mut self.id),
            outcome: self.outcome,
        });
    }
}

impl Approvals {
    pub fn new(notify: Notify) -> Self {
        Self {
            inner: Arc::default(),
            notify,
        }
    }

    /// A poisoned lock only means a panic elsewhere mid-update; every update leaves the list consistent.
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Shows a card and waits for the person. `alive` says whether the requesting pane is still running (for
    /// `pane_spawn`: its token still resolves); once it says no, the request is `Denied`.
    pub async fn request(
        &self,
        req: Request,
        timeout: Duration,
        alive: impl Fn() -> bool,
    ) -> Outcome {
        let (tx, rx) = oneshot::channel();
        let created_at = now_ms();
        let approval = {
            let mut inner = self.lock();
            inner.next += 1;
            let approval = Approval {
                id: format!("a{}", inner.next),
                pane: req.pane,
                requester_agent: req.requester_agent,
                agent: req.agent,
                title: req.title,
                cwd: req.cwd,
                task: req.task,
                created_at,
                expires_at: created_at.saturating_add(timeout.as_millis() as u64),
            };
            inner.pending.push(Pending {
                approval: approval.clone(),
                answer: tx,
            });
            approval
        };
        let mut ending = Ending {
            approvals: self,
            id: approval.id.clone(),
            // Until set below: the caller dropped the request (e.g. the MCP call was cancelled).
            outcome: Outcome::Denied,
        };
        (self.notify)(Notice::Opened(approval));
        let gone = async move {
            while alive() {
                tokio::time::sleep(ALIVE_POLL).await;
            }
        };
        ending.outcome = tokio::select! {
            answer = rx => if answer == Ok(true) { Outcome::Approved } else { Outcome::Denied },
            () = tokio::time::sleep(timeout) => Outcome::Timeout,
            () = gone => Outcome::Denied,
        };
        ending.outcome
    }

    /// The person's answer. False when the card is no longer pending (already answered, timed out, or gone).
    pub fn answer(&self, id: &str, allow: bool) -> bool {
        let mut inner = self.lock();
        let Some(i) = inner.pending.iter().position(|p| p.approval.id == id) else {
            return false;
        };
        inner.pending.remove(i).answer.send(allow).is_ok()
    }

    /// Pending cards, oldest first, for a WebView that (re)loads.
    pub fn pending(&self) -> Vec<Approval> {
        self.lock()
            .pending
            .iter()
            .map(|p| p.approval.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn setup() -> (Approvals, Arc<Mutex<Vec<Notice>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let approvals = Approvals::new(Arc::new(move |n| log.lock().unwrap().push(n)));
        (approvals, seen)
    }

    fn req(pane: &str) -> Request {
        Request {
            pane: pane.into(),
            requester_agent: "Claude Code".into(),
            agent: "Codex".into(),
            title: "review: auth".into(),
            cwd: "/tmp/app".into(),
            task: "Review the diff".into(),
        }
    }

    const LONG: Duration = Duration::from_secs(30);

    /// Starts a request in the background and waits until its card is pending.
    async fn start(
        a: &Approvals,
        pane: &str,
        timeout: Duration,
        alive: impl Fn() -> bool + Send + 'static,
    ) -> (String, tokio::task::JoinHandle<Outcome>) {
        let before = a.pending().len();
        let a2 = a.clone();
        let r = req(pane);
        let task = tokio::spawn(async move { a2.request(r, timeout, alive).await });
        while a.pending().len() == before {
            tokio::task::yield_now().await;
        }
        let id = a.pending().last().unwrap().id.clone();
        (id, task)
    }

    fn closed(seen: &Mutex<Vec<Notice>>) -> Vec<(String, Outcome)> {
        seen.lock()
            .unwrap()
            .iter()
            .filter_map(|n| match n {
                Notice::Closed { id, outcome } => Some((id.clone(), *outcome)),
                Notice::Opened(_) => None,
            })
            .collect()
    }

    #[tokio::test]
    async fn approve_and_deny() {
        let (a, seen) = setup();
        let (id, task) = start(&a, "p1", LONG, || true).await;
        let card = a.pending()[0].clone();
        assert_eq!(card.expires_at - card.created_at, 30_000);
        assert_eq!(seen.lock().unwrap()[0], Notice::Opened(card));
        assert!(a.answer(&id, true));
        assert_eq!(task.await.unwrap(), Outcome::Approved);
        assert!(!a.answer(&id, false), "already answered");

        let (id2, task) = start(&a, "p1", LONG, || true).await;
        assert!(a.answer(&id2, false));
        assert_eq!(task.await.unwrap(), Outcome::Denied);
        assert!(a.pending().is_empty());
        assert_eq!(
            closed(&seen),
            [(id, Outcome::Approved), (id2, Outcome::Denied)]
        );
    }

    #[tokio::test]
    async fn times_out() {
        let (a, seen) = setup();
        let (id, task) = start(&a, "p1", Duration::from_millis(50), || true).await;
        assert_eq!(task.await.unwrap(), Outcome::Timeout);
        assert!(a.pending().is_empty());
        assert!(!a.answer(&id, true), "a late answer changes nothing");
        assert_eq!(closed(&seen), [(id, Outcome::Timeout)]);
    }

    #[tokio::test]
    async fn requester_gone_denies() {
        let (a, seen) = setup();
        let alive = Arc::new(AtomicBool::new(true));
        let flag = alive.clone();
        let (id, task) = start(&a, "p1", LONG, move || flag.load(Ordering::SeqCst)).await;
        alive.store(false, Ordering::SeqCst);
        assert_eq!(task.await.unwrap(), Outcome::Denied);
        assert!(a.pending().is_empty());
        assert_eq!(closed(&seen), [(id, Outcome::Denied)]);
    }

    #[tokio::test]
    async fn dropped_request_closes_its_card() {
        let (a, seen) = setup();
        let (id, task) = start(&a, "p1", LONG, || true).await;
        task.abort();
        let _ = task.await;
        assert!(a.pending().is_empty());
        assert_eq!(closed(&seen), [(id, Outcome::Denied)]);
    }

    #[tokio::test]
    async fn stacked_oldest_first_answered_independently() {
        let (a, _) = setup();
        let (id1, t1) = start(&a, "p1", LONG, || true).await;
        let (id2, t2) = start(&a, "p2", LONG, || true).await;
        let (id3, t3) = start(&a, "p1", LONG, || true).await;
        let ids: Vec<_> = a.pending().into_iter().map(|c| c.id).collect();
        assert_eq!(ids, [id1.clone(), id2.clone(), id3.clone()]);

        assert!(a.answer(&id2, true));
        assert_eq!(t2.await.unwrap(), Outcome::Approved);
        let ids: Vec<_> = a.pending().into_iter().map(|c| c.id).collect();
        assert_eq!(ids, [id1.clone(), id3.clone()]);
        assert!(a.answer(&id3, false));
        assert!(a.answer(&id1, true));
        assert_eq!(t1.await.unwrap(), Outcome::Approved);
        assert_eq!(t3.await.unwrap(), Outcome::Denied);
    }
}
