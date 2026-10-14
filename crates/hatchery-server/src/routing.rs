//! hq-core router: roles, presets, seats and a limit-aware fallback.
//!
//! Config (`<data_dir>/routing.json`, owner-edited, hot-reloaded on each call):
//! ```json
//! {
//!   "seats": [{ "id": "box-claude", "node": "hatchery-box", "provider": "claude" }],
//!   "roles": {
//!     "dev":     { "preset": ["claude/opus:high", "codex/gpt-5:high"], "auto": true },
//!     "advisor": { "preset": ["codex/gpt-5:medium"], "auto": false }
//!   },
//!   "limit_cooldown_secs": 3600,
//!   "advisor": { "min_interval_secs": 300, "dedup_secs": 3600 }
//! }
//! ```
//! A preset entry is `provider/model[:effort]`. A seat is one place a provider can run
//! (a node + provider, optionally a named account). The router walks the role's preset in
//! order and, for each entry, the seats of that provider; a seat with a recent `limit`
//! signal is skipped until a `ready` signal, a parsed reset time, or the cooldown passes.
//! Signals are hints (screen heuristics): they only reorder and never delete a seat, and
//! when every seat is limited the router still answers with the soonest-free one.
//! The router recommends; it picks on its own only in roles with `"auto": true`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Seat {
    pub id: String,
    pub node: String,
    pub provider: String,
    #[serde(default)]
    pub account: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Role {
    pub preset: Vec<String>,
    #[serde(default)]
    pub auto: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AdvisorPolicy {
    #[serde(default = "d_interval")]
    pub min_interval_secs: u64,
    #[serde(default = "d_dedup")]
    pub dedup_secs: u64,
}
fn d_interval() -> u64 { 300 }
fn d_dedup() -> u64 { 3600 }
impl Default for AdvisorPolicy {
    fn default() -> Self { Self { min_interval_secs: d_interval(), dedup_secs: d_dedup() } }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RoutingConfig {
    #[serde(default)]
    pub seats: Vec<Seat>,
    #[serde(default)]
    pub roles: BTreeMap<String, Role>,
    #[serde(default)]
    pub limit_cooldown_secs: Option<u64>,
    #[serde(default)]
    pub advisor: AdvisorPolicy,
}

/// One model a node's provider CLI exposes (as reported by the node).
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct InventoryModel {
    pub id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub efforts: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub default: bool,
}

/// What a node reports for one provider.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ProviderInventory {
    pub provider: String,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub models: Vec<InventoryModel>,
}

/// Stored inventory row: one provider on one node.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct NodeInventory {
    pub node: String,
    pub provider: String,
    pub source: String,
    pub models: Vec<InventoryModel>,
    pub updated_at_ms: u64,
}

fn ok_name(x: &str) -> bool {
    !x.is_empty() && x.len() <= 128 && x.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.[]".contains(&b))
}

/// Validate a node's report (bounded, plain identifiers only).
pub fn clean_report(providers: Vec<ProviderInventory>) -> Result<Vec<ProviderInventory>, &'static str> {
    if providers.len() > 16 {
        return Err("too many providers");
    }
    for p in &providers {
        if !ok_name(&p.provider) || p.source.len() > 32 || p.models.len() > 256 {
            return Err("bad provider entry");
        }
        if p.models.iter().any(|m| !ok_name(&m.id) || m.efforts.len() > 16 || m.efforts.iter().any(|e| !ok_name(e))) {
            return Err("bad model entry");
        }
    }
    Ok(providers)
}

impl RoutingConfig {
    /// Seats are the node-reported providers; roles are the user's stored choices.
    pub fn from_store(inventory: &[NodeInventory], roles: BTreeMap<String, Role>) -> Self {
        let seats = inventory
            .iter()
            .map(|i| Seat { id: format!("{}/{}", i.node, i.provider), node: i.node.clone(), provider: i.provider.clone(), account: None })
            .collect();
        Self { seats, roles, limit_cooldown_secs: None, advisor: AdvisorPolicy::default() }
    }
    fn cooldown_ms(&self) -> u64 {
        self.limit_cooldown_secs.unwrap_or(3600) * 1000
    }
}

/// `provider/model[:effort]`
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ModelRef {
    pub provider: String,
    pub model: String,
    pub effort: Option<String>,
}

pub fn parse_model_ref(s: &str) -> Option<ModelRef> {
    let (provider, rest) = s.split_once('/')?;
    let (model, effort) = match rest.split_once(':') {
        Some((m, e)) => (m, Some(e.to_owned()).filter(|e| !e.is_empty())),
        None => (rest, None),
    };
    let ok = |x: &str| !x.is_empty() && x.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b));
    (ok(provider) && ok(model) && effort.as_deref().is_none_or(ok)).then(|| ModelRef { provider: provider.into(), model: model.into(), effort })
}

/// Latest limit knowledge per (node, provider), built from session signals.
#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct SeatState {
    /// When the seat is expected to be free again (ms); `None` = free.
    pub limited_until_ms: Option<u64>,
    pub hint: Option<String>,
}

/// Fold signals (oldest first) into per-(node, provider) state.
pub fn fold_signals<'a>(signals: impl IntoIterator<Item = &'a crate::tasks::SessionSignal>, cfg: &RoutingConfig) -> BTreeMap<(String, String), SeatState> {
    let mut out: BTreeMap<(String, String), SeatState> = BTreeMap::new();
    for s in signals {
        let key = (s.node_id.clone(), s.provider.clone());
        match s.kind.as_str() {
            "limit" => {
                let until = s.hint.as_deref().and_then(|h| parse_reset(h, s.at_ms)).unwrap_or(s.at_ms + cfg.cooldown_ms());
                out.insert(key, SeatState { limited_until_ms: Some(until), hint: s.hint.clone() });
            }
            "ready" => {
                out.insert(key, SeatState::default());
            }
            _ => {}
        }
    }
    out
}

/// "try again in 2 hours", "in 45 minutes": relative reset, capped at 8 days.
pub fn parse_reset(hint: &str, at_ms: u64) -> Option<u64> {
    let lower = hint.to_lowercase();
    let idx = lower.find(" in ")?;
    let mut words = lower[idx + 4..].split_whitespace();
    let n: f64 = words.next()?.parse().ok()?;
    let unit = words.next()?;
    let secs = if unit.starts_with("min") { 60.0 } else if unit.starts_with("hour") || unit == "h" || unit.starts_with("hr") { 3600.0 } else if unit.starts_with("day") { 86400.0 } else { return None };
    let ms = (n * secs * 1000.0) as u64;
    (ms > 0 && ms <= 8 * 86_400_000).then_some(at_ms + ms)
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Candidate {
    pub seat: Seat,
    pub model: ModelRef,
    pub available: bool,
    pub limited_until_ms: Option<u64>,
    pub hint: Option<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Recommendation {
    pub role: String,
    /// First available candidate (or the soonest-free one when all are limited).
    pub pick: Option<Candidate>,
    /// Whether the router may act on `pick` without the operator (role allows auto).
    pub auto: bool,
    /// The preferred candidate when `pick` is a fallback from it.
    pub fallback_from: Option<Candidate>,
    pub candidates: Vec<Candidate>,
}

/// A role preset entry is valid only if some node reports that provider/model (and effort).
pub fn offered(inventory: &[NodeInventory], m: &ModelRef) -> bool {
    inventory.iter().any(|i| {
        i.provider == m.provider
            && i.models.iter().any(|x| x.id == m.model && m.effort.as_ref().is_none_or(|e| x.efforts.iter().any(|y| y == e)))
    })
}

pub fn recommend(cfg: &RoutingConfig, state: &BTreeMap<(String, String), SeatState>, role: &str, now_ms: u64) -> Result<Recommendation, String> {
    recommend_in(cfg, None, state, role, now_ms)
}

/// Like [`recommend`], but only seats whose node reports the model count (when `inventory` is given).
pub fn recommend_in(cfg: &RoutingConfig, inventory: Option<&[NodeInventory]>, state: &BTreeMap<(String, String), SeatState>, role: &str, now_ms: u64) -> Result<Recommendation, String> {
    let r = cfg.roles.get(role).ok_or("unknown role")?;
    let mut candidates = Vec::new();
    for entry in &r.preset {
        let model = parse_model_ref(entry).ok_or_else(|| format!("bad preset entry: {entry}"))?;
        let has = |seat: &Seat| {
            inventory.is_none_or(|inv| {
                inv.iter().any(|i| i.node == seat.node && i.provider == seat.provider && i.models.iter().any(|x| x.id == model.model))
            })
        };
        for seat in cfg.seats.iter().filter(|s| s.provider == model.provider && has(s)) {
            let st = state.get(&(seat.node.clone(), seat.provider.clone())).cloned().unwrap_or_default();
            let limited = st.limited_until_ms.filter(|u| *u > now_ms);
            candidates.push(Candidate { seat: seat.clone(), model: model.clone(), available: limited.is_none(), limited_until_ms: limited, hint: limited.and(st.hint) });
        }
    }
    let pick = candidates
        .iter()
        .find(|c| c.available)
        .or_else(|| candidates.iter().min_by_key(|c| c.limited_until_ms.unwrap_or(u64::MAX)))
        .cloned();
    let fallback_from = match (&pick, candidates.first()) {
        (Some(p), Some(first)) if p != first => Some(first.clone()),
        _ => None,
    };
    Ok(Recommendation { role: role.into(), pick, auto: r.auto, fallback_from, candidates })
}

/// Advisor anti-noise gate: one advice per `min_interval` per advisor, and the same topic
/// not again within `dedup`. Memory only (a restart forgets; that is acceptable noise).
#[derive(Default)]
pub struct AdviceGate {
    last: BTreeMap<String, u64>,
    topics: BTreeMap<(String, String), u64>,
}

impl AdviceGate {
    pub fn check(&mut self, policy: &AdvisorPolicy, advisor: &str, topic: &str, now_ms: u64) -> Result<(), &'static str> {
        let topic = topic.trim().to_lowercase();
        if topic.is_empty() {
            return Err("topic required");
        }
        if self.last.get(advisor).is_some_and(|t| now_ms < t + policy.min_interval_secs * 1000) {
            return Err("too soon");
        }
        let key = (advisor.to_owned(), topic);
        if self.topics.get(&key).is_some_and(|t| now_ms < t + policy.dedup_secs * 1000) {
            return Err("duplicate topic");
        }
        self.last.insert(advisor.to_owned(), now_ms);
        self.topics.insert(key, now_ms);
        let horizon = now_ms.saturating_sub(policy.dedup_secs * 1000);
        self.topics.retain(|_, t| *t >= horizon);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::SessionSignal;

    fn cfg() -> RoutingConfig {
        serde_json::from_value(serde_json::json!({
            "seats": [
                { "id": "a-claude", "node": "a", "provider": "claude" },
                { "id": "b-claude", "node": "b", "provider": "claude" },
                { "id": "a-codex", "node": "a", "provider": "codex" }
            ],
            "roles": {
                "dev": { "preset": ["claude/opus:high", "codex/gpt-5:high"], "auto": true },
                "advisor": { "preset": ["codex/gpt-5:medium"] }
            },
            "limit_cooldown_secs": 600
        }))
        .unwrap()
    }

    fn sig(id: i64, node: &str, provider: &str, kind: &str, hint: Option<&str>, at: u64) -> SessionSignal {
        SessionSignal { id, node_id: node.into(), workspace_id: "w".into(), instance_id: 1, provider: provider.into(), kind: kind.into(), hint: hint.map(Into::into), at_ms: at }
    }

    #[test]
    fn parses_model_refs() {
        assert_eq!(parse_model_ref("claude/opus:high").unwrap().effort.as_deref(), Some("high"));
        assert_eq!(parse_model_ref("codex/gpt-5").unwrap().effort, None);
        assert!(parse_model_ref("bad").is_none());
        assert!(parse_model_ref("a/b c").is_none());
    }

    #[test]
    fn picks_first_free_seat_and_falls_back_on_limit() {
        let c = cfg();
        let r = recommend(&c, &BTreeMap::new(), "dev", 0).unwrap();
        assert_eq!(r.pick.as_ref().unwrap().seat.id, "a-claude");
        assert!(r.auto && r.fallback_from.is_none());
        let sigs = [sig(1, "a", "claude", "limit", Some("usage limit, try again in 2 hours"), 1000)];
        let st = fold_signals(&sigs, &c);
        let r = recommend(&c, &st, "dev", 5000).unwrap();
        assert_eq!(r.pick.as_ref().unwrap().seat.id, "b-claude");
        assert_eq!(r.fallback_from.as_ref().unwrap().seat.id, "a-claude");
        assert_eq!(r.candidates[0].limited_until_ms, Some(1000 + 7_200_000));
        // Return after reset.
        let r = recommend(&c, &st, "dev", 1000 + 7_200_001).unwrap();
        assert_eq!(r.pick.unwrap().seat.id, "a-claude");
    }

    #[test]
    fn ready_clears_and_cooldown_applies_without_hint() {
        let c = cfg();
        let sigs = [sig(1, "a", "claude", "limit", None, 0), sig(2, "b", "claude", "limit", None, 0)];
        let st = fold_signals(&sigs, &c);
        let r = recommend(&c, &st, "dev", 1).unwrap();
        assert_eq!(r.pick.as_ref().unwrap().seat.provider, "codex");
        // All limited: soonest-free is still offered.
        let sigs2 = [sig(1, "a", "claude", "limit", None, 0), sig(2, "b", "claude", "limit", None, 10), sig(3, "a", "codex", "limit", None, 20)];
        let r = recommend(&c, &fold_signals(&sigs2, &c), "dev", 30).unwrap();
        assert_eq!(r.pick.as_ref().unwrap().seat.id, "a-claude");
        assert!(!r.pick.unwrap().available);
        // ready clears.
        let sigs3 = [sig(1, "a", "claude", "limit", None, 0), sig(2, "a", "claude", "ready", None, 5)];
        assert!(recommend(&c, &fold_signals(&sigs3, &c), "dev", 6).unwrap().pick.unwrap().available);
        // After cooldown (600 s).
        assert!(recommend(&c, &st, "dev", 600_001).unwrap().candidates.iter().all(|x| x.available));
    }

    #[test]
    fn non_auto_role_and_unknown_role() {
        let c = cfg();
        assert!(!recommend(&c, &BTreeMap::new(), "advisor", 0).unwrap().auto);
        assert!(recommend(&c, &BTreeMap::new(), "nope", 0).is_err());
    }

    #[test]
    fn advice_gate_limits_noise() {
        let p = AdvisorPolicy { min_interval_secs: 60, dedup_secs: 600 };
        let mut g = AdviceGate::default();
        assert!(g.check(&p, "adv", "tests failing", 0).is_ok());
        assert_eq!(g.check(&p, "adv", "other", 1000), Err("too soon"));
        assert_eq!(g.check(&p, "adv", "Tests failing ", 61_000), Err("duplicate topic"));
        assert!(g.check(&p, "adv", "other", 61_000).is_ok());
        assert!(g.check(&p, "adv2", "tests failing", 61_000).is_ok());
        assert!(g.check(&p, "adv", "tests failing", 700_000).is_ok());
    }
}
