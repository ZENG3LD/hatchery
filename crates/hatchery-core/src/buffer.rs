//! Bounded in-memory replay buffer for the C2 restreamer (A2).
//!
//! Events are numbered with a monotonic `seq` per stream key (node + channel).
//! Core ACKs the last processed seq; on reconnect it RESUMEs after N.
//! Overflow applies backpressure to the node; gaps older than the buffer are
//! recovered by C2 from node journals (never by core).

use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

/// Identity of one sequenced stream inside C2.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StreamKey {
    pub node_id: String,
    pub channel_id: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BufferedEvent {
    pub seq: u64,
    pub node_id: String,
    pub channel_id: u32,
    /// Opaque payload (inner ciphertext). C2 does not inspect it.
    pub payload: Vec<u8>,
    pub enqueued_at: Instant,
}

#[derive(Clone, Debug)]
pub struct BufferLimits {
    /// Max events retained across all streams.
    pub max_events: usize,
    /// Max total payload bytes retained.
    pub max_bytes: usize,
    /// Max age of an unacked event.
    pub max_age: Duration,
}

impl Default for BufferLimits {
    fn default() -> Self {
        Self {
            max_events: 4096,
            max_bytes: 16 * 1024 * 1024,
            max_age: Duration::from_secs(300),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnqueueOutcome {
    /// Event stored; `window_credit` remaining for the stream (0 = backpressure).
    Accepted { window_credit: u32 },
    /// Buffer full or age-capped; node must wait / retry from its journal.
    Backpressure,
}

/// Per-stream sequencing + global bounded retention.
pub struct ReplayBuffer {
    limits: BufferLimits,
    /// Next seq to assign per stream.
    next_seq: BTreeMap<StreamKey, u64>,
    /// Last ACK from core per stream (events with seq <= ack are releasable).
    last_ack: BTreeMap<StreamKey, u64>,
    /// Events awaiting ACK, oldest first.
    events: VecDeque<BufferedEvent>,
    total_bytes: usize,
    /// Per-stream credit for WINDOW backpressure (starts at default_credit).
    credits: BTreeMap<StreamKey, u32>,
    default_credit: u32,
}

impl ReplayBuffer {
    pub fn new(limits: BufferLimits) -> Self {
        Self {
            limits,
            next_seq: BTreeMap::new(),
            last_ack: BTreeMap::new(),
            events: VecDeque::new(),
            total_bytes: 0,
            credits: BTreeMap::new(),
            default_credit: 64,
        }
    }

    pub fn with_default_credit(mut self, credit: u32) -> Self {
        self.default_credit = credit.max(1);
        self
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    pub fn credit(&self, key: &StreamKey) -> u32 {
        self.credits.get(key).copied().unwrap_or(self.default_credit)
    }

    /// Expire by age, then refuse if still over limits.
    fn trim_expired(&mut self, now: Instant) {
        while let Some(front) = self.events.front() {
            if now.duration_since(front.enqueued_at) <= self.limits.max_age {
                break;
            }
            if let Some(ev) = self.events.pop_front() {
                self.total_bytes = self.total_bytes.saturating_sub(ev.payload.len());
            }
        }
    }

    pub fn enqueue(
        &mut self,
        node_id: impl Into<String>,
        channel_id: u32,
        payload: Vec<u8>,
        now: Instant,
    ) -> EnqueueOutcome {
        self.trim_expired(now);
        let node_id = node_id.into();
        let key = StreamKey {
            node_id: node_id.clone(),
            channel_id,
        };
        let credit = self.credit(&key);
        if credit == 0 {
            return EnqueueOutcome::Backpressure;
        }
        let would_events = self.events.len() + 1;
        let would_bytes = self.total_bytes + payload.len();
        if would_events > self.limits.max_events || would_bytes > self.limits.max_bytes {
            // Zero credit on this stream so the node sees WINDOW=0.
            self.credits.insert(key, 0);
            return EnqueueOutcome::Backpressure;
        }
        let seq = {
            let entry = self.next_seq.entry(key.clone()).or_insert(1);
            let seq = *entry;
            *entry = seq + 1;
            seq
        };
        self.total_bytes += payload.len();
        self.events.push_back(BufferedEvent {
            seq,
            node_id,
            channel_id,
            payload,
            enqueued_at: now,
        });
        let new_credit = credit - 1;
        self.credits.insert(key, new_credit);
        EnqueueOutcome::Accepted {
            window_credit: new_credit,
        }
    }

    /// Core ACK: drop events with seq <= ack for that stream; restore credit.
    pub fn ack(&mut self, node_id: &str, channel_id: u32, seq: u64) {
        let key = StreamKey {
            node_id: node_id.to_owned(),
            channel_id,
        };
        let prev = self.last_ack.get(&key).copied().unwrap_or(0);
        if seq <= prev {
            return;
        }
        self.last_ack.insert(key.clone(), seq);
        let mut released = 0u32;
        let mut kept = VecDeque::new();
        while let Some(ev) = self.events.pop_front() {
            let same = ev.node_id == key.node_id && ev.channel_id == key.channel_id;
            if same && ev.seq <= seq {
                self.total_bytes = self.total_bytes.saturating_sub(ev.payload.len());
                released = released.saturating_add(1);
            } else {
                kept.push_back(ev);
            }
        }
        self.events = kept;
        let credit = self.credit(&key).saturating_add(released).min(self.default_credit);
        self.credits.insert(key, credit);
    }

    /// Global ACK used when core drains a single combined stream and acks by global order.
    /// Here we treat `seq` as the highest seq observed in the drain for a given stream key
    /// carried in the envelope; callers should use [`ack`].
    pub fn events_after(&self, node_id: &str, channel_id: u32, after: u64) -> Vec<BufferedEvent> {
        self.events
            .iter()
            .filter(|ev| {
                ev.node_id == node_id && ev.channel_id == channel_id && ev.seq > after
            })
            .cloned()
            .collect()
    }

    /// Lowest seq still retained for the stream, if any.
    pub fn oldest_seq(&self, node_id: &str, channel_id: u32) -> Option<u64> {
        self.events
            .iter()
            .filter(|ev| ev.node_id == node_id && ev.channel_id == channel_id)
            .map(|ev| ev.seq)
            .min()
    }

    /// Highest seq assigned for the stream (whether or not still buffered).
    pub fn next_seq(&self, node_id: &str, channel_id: u32) -> u64 {
        let key = StreamKey {
            node_id: node_id.to_owned(),
            channel_id,
        };
        self.next_seq.get(&key).copied().unwrap_or(1)
    }

    /// True when RESUME(after=N) asks for a seq that has already left the buffer.
    pub fn gap_before_buffer(&self, node_id: &str, channel_id: u32, after: u64) -> bool {
        match self.oldest_seq(node_id, channel_id) {
            None => {
                // Nothing buffered: gap if core expects something already produced.
                let next = self.next_seq(node_id, channel_id);
                after + 1 < next
            }
            Some(oldest) => after + 1 < oldest,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits_tiny() -> BufferLimits {
        BufferLimits {
            max_events: 3,
            max_bytes: 1024,
            max_age: Duration::from_secs(60),
        }
    }

    #[test]
    fn assigns_monotonic_seq_and_acks() {
        let mut buf = ReplayBuffer::new(limits_tiny()).with_default_credit(8);
        let now = Instant::now();
        assert!(matches!(
            buf.enqueue("n1", 1, b"a".to_vec(), now),
            EnqueueOutcome::Accepted { window_credit: 7 }
        ));
        assert!(matches!(
            buf.enqueue("n1", 1, b"b".to_vec(), now),
            EnqueueOutcome::Accepted { .. }
        ));
        let after0 = buf.events_after("n1", 1, 0);
        assert_eq!(after0.len(), 2);
        assert_eq!(after0[0].seq, 1);
        assert_eq!(after0[1].seq, 2);
        buf.ack("n1", 1, 1);
        let after1 = buf.events_after("n1", 1, 0);
        assert_eq!(after1.len(), 1);
        assert_eq!(after1[0].seq, 2);
    }

    #[test]
    fn overflow_signals_backpressure() {
        let mut buf = ReplayBuffer::new(limits_tiny()).with_default_credit(8);
        let now = Instant::now();
        for i in 0..3 {
            assert!(
                matches!(
                    buf.enqueue("n1", 1, vec![i], now),
                    EnqueueOutcome::Accepted { .. }
                ),
                "event {i}"
            );
        }
        assert_eq!(
            buf.enqueue("n1", 1, vec![9], now),
            EnqueueOutcome::Backpressure
        );
        assert_eq!(buf.credit(&StreamKey { node_id: "n1".into(), channel_id: 1 }), 0);
    }

    #[test]
    fn resume_gap_detected_when_acked_out() {
        let mut buf = ReplayBuffer::new(BufferLimits {
            max_events: 10,
            max_bytes: 4096,
            max_age: Duration::from_secs(60),
        })
        .with_default_credit(8);
        let now = Instant::now();
        for _ in 0..5 {
            buf.enqueue("n1", 1, b"x".to_vec(), now);
        }
        buf.ack("n1", 1, 3);
        // oldest retained is 4; RESUME(after=1) needs seq 2 which is gone.
        assert!(buf.gap_before_buffer("n1", 1, 1));
        assert!(!buf.gap_before_buffer("n1", 1, 3));
        let replay = buf.events_after("n1", 1, 3);
        assert_eq!(replay.iter().map(|e| e.seq).collect::<Vec<_>>(), vec![4, 5]);
    }

    #[test]
    fn age_expiry_drops_old_events() {
        let mut buf = ReplayBuffer::new(BufferLimits {
            max_events: 10,
            max_bytes: 4096,
            max_age: Duration::from_millis(5),
        });
        let past = Instant::now() - Duration::from_millis(50);
        buf.enqueue("n1", 1, b"old".to_vec(), past);
        assert_eq!(buf.len(), 1);
        // enqueue triggers trim
        let outcome = buf.enqueue("n1", 1, b"new".to_vec(), Instant::now());
        assert!(matches!(outcome, EnqueueOutcome::Accepted { .. }));
        assert_eq!(buf.len(), 1);
        assert_eq!(buf.events_after("n1", 1, 0)[0].payload, b"new");
    }
}
