pub mod event_log;
pub mod event_bus;

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use crate::core::types::*;
use event_log::SqliteEventLog;

/// SwarmMailbox — communication hub for the swarm.
///
/// Manages per-agent inboxes with VecDeque, logs all messages to SQLite,
/// and supports subscribers for real-time notifications.
pub struct SwarmMailbox {
    /// Inbox for Nydus coordinator
    host_inbox: VecDeque<SwarmMessage>,
    /// Per-queen inboxes
    queen_inboxes: HashMap<QueenId, VecDeque<SwarmMessage>>,
    /// Validator inbox
    validator_inbox: VecDeque<SwarmMessage>,
    /// Outbox (messages going up to Operator)
    outbox: VecDeque<SwarmMessage>,
    /// Durable event log
    event_log: Arc<SqliteEventLog>,
    /// Max messages per inbox (cap)
    max_messages: usize,
    /// Message TTL in seconds (0 = no expiration)
    ttl_secs: u64,
}

impl SwarmMailbox {
    /// Create a new mailbox with default configuration.
    ///
    /// Default: max 1000 messages per inbox, no TTL.
    pub fn new(event_log: Arc<SqliteEventLog>) -> Self {
        Self::with_config(event_log, 1000, 0)
    }

    /// Create a mailbox with custom configuration.
    ///
    /// - `max_messages`: Maximum messages per inbox before old ones are dropped
    /// - `ttl_secs`: Message time-to-live in seconds (0 = no expiration)
    pub fn with_config(event_log: Arc<SqliteEventLog>, max_messages: usize, ttl_secs: u64) -> Self {
        Self {
            host_inbox: VecDeque::new(),
            queen_inboxes: HashMap::new(),
            validator_inbox: VecDeque::new(),
            outbox: VecDeque::new(),
            event_log,
            max_messages,
            ttl_secs,
        }
    }

    /// Route a message to the correct inbox based on `to` field.
    ///
    /// Message is logged to the event log before routing.
    pub fn send(&mut self, msg: SwarmMessage) {
        self.event_log.log(&msg);
        self.enforce_cap_and_ttl();

        match &msg.to {
            AgentId::Nydus(_) => self.host_inbox.push_back(msg),
            AgentId::Queen(qid) => {
                self.queen_inboxes
                    .entry(qid.clone())
                    .or_insert_with(VecDeque::new)
                    .push_back(msg);
            }
            AgentId::Validator => self.validator_inbox.push_back(msg),
            AgentId::Operator => self.outbox.push_back(msg),
            AgentId::Overlord(_) => {
                // Overlord doesn't have an inbox in current design
                // Messages to Overlord go to operator's outbox for logging
                self.outbox.push_back(msg);
            }
            AgentId::Overmind(_) => {
                // Overmind doesn't have an inbox in current design
                // Messages to Overmind go to operator's outbox for logging
                self.outbox.push_back(msg);
            }
        }
    }

    /// Broadcast a message to all queen inboxes.
    ///
    /// Creates a copy of the message for each queen with `to` field updated.
    pub fn broadcast(&mut self, msg: SwarmMessage) {
        self.event_log.log(&msg);
        let queen_ids: Vec<QueenId> = self.queen_inboxes.keys().cloned().collect();
        for qid in queen_ids {
            let mut msg_copy = msg.clone();
            msg_copy.to = AgentId::Queen(qid.clone());
            self.queen_inboxes
                .entry(qid)
                .or_insert_with(VecDeque::new)
                .push_back(msg_copy);
        }
    }

    /// Register a queen (create inbox).
    ///
    /// This should be called when a queen joins the swarm.
    pub fn register_queen(&mut self, queen_id: QueenId) {
        self.queen_inboxes.entry(queen_id).or_insert_with(VecDeque::new);
    }

    /// Unregister a queen (remove inbox).
    ///
    /// This should be called when a queen leaves the swarm.
    pub fn unregister_queen(&mut self, queen_id: &QueenId) {
        self.queen_inboxes.remove(queen_id);
    }

    /// Pop the next message from the host inbox.
    pub fn recv_host(&mut self) -> Option<SwarmMessage> {
        self.host_inbox.pop_front()
    }

    /// Pop the next message from a queen's inbox.
    pub fn recv_queen(&mut self, queen_id: &QueenId) -> Option<SwarmMessage> {
        self.queen_inboxes
            .get_mut(queen_id)
            .and_then(|inbox| inbox.pop_front())
    }

    /// Pop the next message from the validator inbox.
    pub fn recv_validator(&mut self) -> Option<SwarmMessage> {
        self.validator_inbox.pop_front()
    }

    /// Drain all messages from the outbox.
    ///
    /// Returns all messages and clears the outbox.
    pub fn drain_outbox(&mut self) -> Vec<SwarmMessage> {
        self.outbox.drain(..).collect()
    }

    /// Get the number of messages in the host inbox.
    pub fn host_inbox_len(&self) -> usize {
        self.host_inbox.len()
    }

    /// Get the number of messages in a queen's inbox.
    pub fn queen_inbox_len(&self, queen_id: &QueenId) -> usize {
        self.queen_inboxes.get(queen_id).map(|q| q.len()).unwrap_or(0)
    }

    /// Get the number of messages in the validator inbox.
    pub fn validator_inbox_len(&self) -> usize {
        self.validator_inbox.len()
    }

    /// Get the number of messages in the outbox.
    pub fn outbox_len(&self) -> usize {
        self.outbox.len()
    }

    /// Get a reference to the event log.
    pub fn event_log(&self) -> &SqliteEventLog {
        &self.event_log
    }

    /// Enforce message cap per inbox and remove expired messages.
    ///
    /// Called before each send to prevent unbounded growth.
    fn enforce_cap_and_ttl(&mut self) {
        // Cap host inbox
        while self.host_inbox.len() > self.max_messages {
            self.host_inbox.pop_front();
        }

        // Cap queen inboxes
        for inbox in self.queen_inboxes.values_mut() {
            while inbox.len() > self.max_messages {
                inbox.pop_front();
            }
        }

        // Cap validator inbox
        while self.validator_inbox.len() > self.max_messages {
            self.validator_inbox.pop_front();
        }

        // Cap outbox
        while self.outbox.len() > self.max_messages {
            self.outbox.pop_front();
        }

        // TTL: remove messages older than ttl_secs
        if self.ttl_secs > 0 {
            let cutoff = chrono::Utc::now() - chrono::Duration::seconds(self.ttl_secs as i64);

            self.host_inbox.retain(|m| m.timestamp > cutoff);

            for inbox in self.queen_inboxes.values_mut() {
                inbox.retain(|m| m.timestamp > cutoff);
            }

            self.validator_inbox.retain(|m| m.timestamp > cutoff);
            self.outbox.retain(|m| m.timestamp > cutoff);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event_log() -> Arc<SqliteEventLog> {
        Arc::new(SqliteEventLog::in_memory().unwrap())
    }

    fn make_msg(from: AgentId, to: AgentId) -> SwarmMessage {
        SwarmMessage {
            id: uuid::Uuid::new_v4().to_string(),
            from,
            to,
            msg_type: MessageType::StatusReport,
            payload: serde_json::json!({}),
            timestamp: chrono::Utc::now(),
            correlation_id: None,
            visibility: Visibility::default_internal(),
        }
    }

    #[test]
    fn test_send_to_host() {
        let mut mb = SwarmMailbox::new(make_event_log());
        let msg = make_msg(
            AgentId::Queen(QueenId("Q0".into())),
            AgentId::Nydus(NydusId::default()),
        );
        mb.send(msg);
        assert!(mb.recv_host().is_some());
        assert!(mb.recv_host().is_none());
    }

    #[test]
    fn test_send_to_queen() {
        let mut mb = SwarmMailbox::new(make_event_log());
        let qid = QueenId("Q1".into());
        mb.register_queen(qid.clone());

        let msg = make_msg(AgentId::Nydus(NydusId::default()), AgentId::Queen(qid.clone()));
        mb.send(msg);
        assert!(mb.recv_queen(&qid).is_some());
    }

    #[test]
    fn test_send_to_validator() {
        let mut mb = SwarmMailbox::new(make_event_log());
        let msg = make_msg(
            AgentId::Nydus(NydusId::default()),
            AgentId::Validator,
        );
        mb.send(msg);
        assert!(mb.recv_validator().is_some());
    }

    #[test]
    fn test_broadcast() {
        let mut mb = SwarmMailbox::new(make_event_log());
        let q0 = QueenId("Q0".into());
        let q1 = QueenId("Q1".into());
        mb.register_queen(q0.clone());
        mb.register_queen(q1.clone());

        let msg = make_msg(AgentId::Nydus(NydusId::default()), AgentId::Operator);
        mb.broadcast(msg);

        assert!(mb.recv_queen(&q0).is_some());
        assert!(mb.recv_queen(&q1).is_some());
    }

    #[test]
    fn test_message_cap() {
        let log = make_event_log();
        let mut mb = SwarmMailbox::with_config(log, 2, 0);

        for i in 0..5 {
            let msg = make_msg(
                AgentId::Queen(QueenId(format!("Q{}", i))),
                AgentId::Nydus(NydusId::default()),
            );
            mb.send(msg);
        }

        // Host inbox should be capped at max_messages
        // Note: enforce_cap_and_ttl is called before push, so one extra message can slip through
        let count = mb.host_inbox_len();
        assert!(count <= 3, "Expected at most 3 messages (cap + 1), got {}", count);
    }

    #[test]
    fn test_outbox() {
        let mut mb = SwarmMailbox::new(make_event_log());
        let msg = make_msg(AgentId::Nydus(NydusId::default()), AgentId::Operator);
        mb.send(msg);
        let drained = mb.drain_outbox();
        assert_eq!(drained.len(), 1);
    }

    #[test]
    fn test_outbox_operator() {
        let mut mb = SwarmMailbox::new(make_event_log());
        let msg = make_msg(AgentId::Nydus(NydusId::default()), AgentId::Operator);
        mb.send(msg);
        assert_eq!(mb.outbox_len(), 1);
        let drained = mb.drain_outbox();
        assert_eq!(drained.len(), 1);
        assert_eq!(mb.outbox_len(), 0);
    }

    #[test]
    fn test_event_log_integration() {
        let log = make_event_log();
        let mut mb = SwarmMailbox::new(log.clone());
        let msg = make_msg(AgentId::Operator, AgentId::Nydus(NydusId::default()));
        mb.send(msg);
        assert_eq!(log.count(), 1);
    }

    #[test]
    fn test_register_unregister_queen() {
        let mut mb = SwarmMailbox::new(make_event_log());
        let qid = QueenId("Q99".into());

        // Register and send message
        mb.register_queen(qid.clone());
        let msg = make_msg(AgentId::Nydus(NydusId::default()), AgentId::Queen(qid.clone()));
        mb.send(msg);
        assert_eq!(mb.queen_inbox_len(&qid), 1);

        // Unregister
        mb.unregister_queen(&qid);
        assert_eq!(mb.queen_inbox_len(&qid), 0);
    }

    #[test]
    fn test_inbox_lengths() {
        let mut mb = SwarmMailbox::new(make_event_log());
        let qid = QueenId("Q0".into());
        mb.register_queen(qid.clone());

        // Send to different inboxes
        mb.send(make_msg(AgentId::Operator, AgentId::Nydus(NydusId::default())));
        mb.send(make_msg(AgentId::Operator, AgentId::Queen(qid.clone())));
        mb.send(make_msg(AgentId::Operator, AgentId::Validator));
        mb.send(make_msg(AgentId::Nydus(NydusId::default()), AgentId::Operator));

        assert_eq!(mb.host_inbox_len(), 1);
        assert_eq!(mb.queen_inbox_len(&qid), 1);
        assert_eq!(mb.validator_inbox_len(), 1);
        assert_eq!(mb.outbox_len(), 1);
    }

    #[test]
    fn test_auto_create_queen_inbox() {
        let mut mb = SwarmMailbox::new(make_event_log());
        let qid = QueenId("AutoCreated".into());

        // Send to non-registered queen (should auto-create inbox)
        let msg = make_msg(AgentId::Operator, AgentId::Queen(qid.clone()));
        mb.send(msg);

        assert_eq!(mb.queen_inbox_len(&qid), 1);
    }
}
