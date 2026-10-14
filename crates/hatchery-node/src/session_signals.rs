//! Short session signals the node reads off its own PTY screens: a provider CLI hit a
//! usage/rate limit, needs a login, or is ready again. Only state changes are sent to
//! core (`task.session_signal` over the node-call channel); screens never leave the node
//! for this. Detection is heuristic text matching on the rendered screen and is kept
//! per provider-agnostic phrase lists so a CLI wording change degrades to "no signal".

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Signal {
    /// `hint` is the matching screen line (trimmed, short), e.g. "resets 3pm".
    Limit { hint: String },
    LoginRequired,
    Ready,
}

impl Signal {
    pub fn kind(&self) -> &'static str {
        match self {
            Signal::Limit { .. } => "limit",
            Signal::LoginRequired => "login_required",
            Signal::Ready => "ready",
        }
    }
}

const LIMIT_PHRASES: &[&str] = &[
    "usage limit",
    "rate limit",
    "rate-limit",
    "limit reached",
    "reached your limit",
    "you've hit your",
    "you have hit your",
    "quota exceeded",
    "exceeded your current quota",
    "too many requests",
    "out of credits",
    "weekly limit",
    "5-hour limit",
];
const LOGIN_PHRASES: &[&str] = &[
    "select login method",
    "sign in with chatgpt",
    "please log in",
    "please login",
    "run /login",
    "not logged in",
    "approve in your browser",
    "how would you like to authenticate",
    "invalid api key",
    "authentication required",
];

/// Classify one screen. Lines are scanned bottom-up: the newest state wins.
pub fn classify(screen: &str) -> Option<Signal> {
    for line in screen.lines().rev().filter(|l| !l.trim().is_empty()).take(40) {
        let lower = line.to_lowercase();
        if LIMIT_PHRASES.iter().any(|p| lower.contains(p)) {
            let hint: String = line.trim().trim_matches(|c: char| !c.is_alphanumeric()).chars().take(120).collect();
            return Some(Signal::Limit { hint });
        }
        if LOGIN_PHRASES.iter().any(|p| lower.contains(p)) {
            return Some(Signal::LoginRequired);
        }
    }
    None
}

#[derive(Default)]
struct Track {
    last: Option<&'static str>,
    scanned: Option<Instant>,
}

/// Per-session change detector with a scan budget (at most one scan per `every`).
pub struct Detector {
    every: Duration,
    sessions: HashMap<(String, u64), Track>,
}

impl Detector {
    pub fn new(every: Duration) -> Self {
        Self { every, sessions: HashMap::new() }
    }

    /// Feed a screen. Returns a signal only when the state changed. A session leaving a
    /// limit/login state reports `Ready`; a quiet session that never had one reports
    /// nothing.
    pub fn observe(&mut self, workspace: &str, instance: u64, screen: &str, now: Instant) -> Option<Signal> {
        let t = self.sessions.entry((workspace.to_owned(), instance)).or_default();
        if t.scanned.is_some_and(|at| now.duration_since(at) < self.every) {
            return None;
        }
        t.scanned = Some(now);
        let found = classify(screen);
        let kind = found.as_ref().map(Signal::kind);
        match (t.last, kind) {
            (a, b) if a == b => None,
            (Some(_), None) => {
                t.last = None;
                Some(Signal::Ready)
            }
            (_, Some(k)) => {
                t.last = Some(k);
                found
            }
            (None, None) => None,
        }
    }

    pub fn forget(&mut self, workspace: &str, instance: u64) {
        self.sessions.remove(&(workspace.to_owned(), instance));
    }
}

/// One outgoing signal.
#[derive(Clone, Debug)]
pub struct Outgoing {
    pub workspace: String,
    pub instance: u64,
    pub provider: String,
    pub signal: Signal,
}

static SINK: OnceLock<tokio::sync::mpsc::UnboundedSender<Outgoing>> = OnceLock::new();
static DETECTOR: OnceLock<Mutex<Detector>> = OnceLock::new();

/// Install the sink (once, at startup). Without a sink, screens are not scanned.
pub fn set_sink(tx: tokio::sync::mpsc::UnboundedSender<Outgoing>) {
    let _ = SINK.set(tx);
}

/// Called from the frame publisher; cheap when no sink is installed.
pub fn feed(workspace: &str, instance: u64, provider: &str, screen: &str) {
    let Some(tx) = SINK.get() else { return };
    let det = DETECTOR.get_or_init(|| Mutex::new(Detector::new(Duration::from_secs(1))));
    let signal = det.lock().unwrap_or_else(|p| p.into_inner()).observe(workspace, instance, screen, Instant::now());
    if let Some(signal) = signal {
        let _ = tx.send(Outgoing { workspace: workspace.to_owned(), instance, provider: provider.to_owned(), signal });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_limits_and_logins() {
        assert_eq!(
            classify("● done\n\nClaude usage limit reached. Your limit will reset at 3pm.\n> "),
            Some(Signal::Limit { hint: "Claude usage limit reached. Your limit will reset at 3pm".into() })
        );
        assert_eq!(classify("■ You've hit your usage limit. Try again in 2 hours."), Some(Signal::Limit { hint: "You've hit your usage limit. Try again in 2 hours".into() }));
        assert_eq!(classify("Select login method:\n 1. Claude account"), Some(Signal::LoginRequired));
        assert_eq!(classify("Welcome back!\n> fix the tests"), None);
    }

    #[test]
    fn newest_line_wins() {
        // A login prompt below an older limit line: login is the current state.
        assert_eq!(classify("rate limit hit earlier\n...\nPlease log in again"), Some(Signal::LoginRequired));
    }

    #[test]
    fn detector_reports_changes_only() {
        let mut d = Detector::new(Duration::from_millis(0));
        let t0 = Instant::now();
        assert_eq!(d.observe("w", 1, "hello", t0), None);
        assert!(matches!(d.observe("w", 1, "usage limit reached", t0), Some(Signal::Limit { .. })));
        assert_eq!(d.observe("w", 1, "usage limit reached", t0), None);
        assert_eq!(d.observe("w", 1, "Please log in", t0), Some(Signal::LoginRequired));
        assert_eq!(d.observe("w", 1, "> ready", t0), Some(Signal::Ready));
        assert_eq!(d.observe("w", 1, "> ready", t0), None);
        // Separate sessions are independent.
        assert!(d.observe("w", 2, "rate limit", t0).is_some());
    }

    #[test]
    fn scan_budget_skips_frequent_frames() {
        let mut d = Detector::new(Duration::from_secs(1));
        let t0 = Instant::now();
        assert_eq!(d.observe("w", 1, "x", t0), None);
        assert_eq!(d.observe("w", 1, "usage limit", t0 + Duration::from_millis(10)), None);
        assert!(d.observe("w", 1, "usage limit", t0 + Duration::from_millis(1100)).is_some());
    }
}
