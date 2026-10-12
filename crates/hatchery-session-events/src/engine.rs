use crate::event::*;
use crate::providers::{parser_for, Item, Parsed, ProviderParser};
use std::collections::{hash_map::DefaultHasher, HashMap};
use std::hash::{Hash, Hasher};

/// An item must be unchanged this long before it is emitted (suppresses character-by-character flapping).
const STABLE_MS: u64 = 350;
/// No screen change for this long = idle (unless the provider shows a working marker).
const QUIET_MS: u64 = 1200;

struct Slot {
    id: u64,
    emitted_hash: Option<u64>,
    rev: u32,
    cand_hash: u64,
    cand_since: u64,
}

/// Feeds screens (or raw bytes) of one session through its provider parser and emits typed events.
pub struct EventStream {
    provider: String,
    parser: Box<dyn ProviderParser>,
    slots: HashMap<String, Slot>,
    next_id: u64,
    last_items: Vec<Item>,
    last_parsed: Parsed,
    screen_hash: u64,
    last_change: u64,
    state: Option<(SessionState, String)>,
    state_id: u64,
    state_rev: u32,
    emu: Option<vt100::Parser>,
    size: (u16, u16),
}

fn h<T: Hash + ?Sized>(t: &T) -> u64 {
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    s.finish()
}

impl EventStream {
    pub fn new(provider: &str) -> Self {
        Self::with_parser(provider, parser_for(provider))
    }

    pub fn with_parser(provider: &str, parser: Box<dyn ProviderParser>) -> Self {
        Self {
            provider: provider.to_owned(), parser, slots: HashMap::new(), next_id: 1, last_items: Vec::new(), last_parsed: Parsed::default(),
            screen_hash: 0, last_change: 0, state: None, state_id: 0, state_rev: 0, emu: None, size: (30, 100),
        }
    }

    pub fn set_size(&mut self, rows: u16, cols: u16) {
        self.size = (rows, cols);
        if let Some(e) = self.emu.as_mut() { e.screen_mut().set_size(rows, cols); }
    }

    /// Raw PTY bytes: emulate them, then observe the resulting screen.
    pub fn feed_bytes(&mut self, bytes: &[u8], now_ms: u64) -> Vec<SessionEvent> {
        let (r, c) = self.size;
        let emu = self.emu.get_or_insert_with(|| vt100::Parser::new(r, c, 0));
        emu.process(bytes);
        let text = emu.screen().contents();
        self.observe_screen(&text, now_ms)
    }

    /// A rendered screen (what C2 relays as a terminal frame).
    pub fn observe_screen(&mut self, text: &str, now_ms: u64) -> Vec<SessionEvent> {
        let rows: Vec<String> = text.lines().map(|l| l.trim_end().to_owned()).collect();
        let hash = h(&rows);
        if hash != self.screen_hash {
            self.screen_hash = hash;
            self.last_change = now_ms;
        }
        let parsed = self.parser.parse(&rows);
        self.last_items = parsed.items.clone();
        self.last_parsed = parsed;
        self.settle(now_ms)
    }

    /// Call periodically so quiet screens settle (items become stable, working turns idle).
    pub fn tick(&mut self, now_ms: u64) -> Vec<SessionEvent> {
        self.settle(now_ms)
    }

    fn settle(&mut self, now_ms: u64) -> Vec<SessionEvent> {
        let mut out = Vec::new();
        let items = std::mem::take(&mut self.last_items);
        for it in &items {
            let hash = h(&it.kind);
            let next_id = &mut self.next_id;
            let slot = self.slots.entry(it.key.clone()).or_insert_with(|| {
                let id = *next_id;
                *next_id += 1;
                Slot { id, emitted_hash: None, rev: 0, cand_hash: hash, cand_since: now_ms }
            });
            if slot.cand_hash != hash {
                slot.cand_hash = hash;
                slot.cand_since = now_ms;
            }
            if now_ms.saturating_sub(slot.cand_since) >= STABLE_MS && slot.emitted_hash != Some(hash) {
                slot.emitted_hash = Some(hash);
                slot.rev += 1;
                out.push(SessionEvent { id: slot.id, rev: slot.rev, provider: self.provider.clone(), source: format!("pty:{}", self.provider), confidence: Confidence::Heuristic, ts_ms: now_ms, event: it.kind.clone() });
            }
        }
        self.last_items = items;
        // session state
        let p = &self.last_parsed;
        let quiet = now_ms.saturating_sub(self.last_change) >= QUIET_MS;
        let (state, detail) = if p.needs_input {
            (SessionState::NeedsInput, p.prompt_hint.clone())
        } else if let Some(d) = &p.working {
            (SessionState::Working, d.clone())
        } else if self.state.is_none() && quiet && self.last_items.is_empty() {
            (SessionState::Starting, String::new())
        } else if !quiet {
            (SessionState::Working, "output changing".to_owned())
        } else {
            (SessionState::Idle, String::new())
        };
        if self.state.as_ref().map(|s| (s.0, &s.1)) != Some((state, &detail)) {
            if self.state.is_none() { self.state_id = self.next_id; self.next_id += 1; }
            self.state_rev += 1;
            self.state = Some((state, detail.clone()));
            out.push(SessionEvent { id: self.state_id, rev: self.state_rev, provider: self.provider.clone(), source: format!("pty:{}", self.provider), confidence: Confidence::Heuristic, ts_ms: now_ms, event: EventKind::State { state, detail } });
        }
        out
    }

    pub fn state(&self) -> Option<SessionState> { self.state.as_ref().map(|s| s.0) }
}
