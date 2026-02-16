//! LLM realtime scheduling: priority-based request routing for LLM inference.

use super::Scheduling;
use crate::core::types::{AgentId, TaskId};
use anyhow::Result;
use parking_lot::Mutex;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Configuration for LLM realtime scheduling.
#[derive(Debug, Clone)]
pub struct LlmRealtimeConfig {
    /// Maximum depth of priority queue
    pub priority_queue_depth: usize,
    /// Maximum concurrent requests
    pub max_concurrent: usize,
    /// Whether to enable preemption
    pub preemption_enabled: bool,
}

impl Default for LlmRealtimeConfig {
    fn default() -> Self {
        LlmRealtimeConfig {
            priority_queue_depth: 1000,
            max_concurrent: 4,
            preemption_enabled: true,
        }
    }
}

/// Inference request with priority and token estimation.
#[derive(Debug, Clone)]
pub struct InferenceRequest {
    /// Task identifier
    pub task_id: TaskId,
    /// Priority (0-255, higher = more urgent)
    pub priority: u8,
    /// Estimated tokens for this request
    pub tokens_estimated: usize,
    /// When the request was created
    pub created_at: Instant,
}

impl InferenceRequest {
    /// Create a new inference request.
    pub fn new(task_id: TaskId, priority: u8, tokens_estimated: usize) -> Self {
        InferenceRequest {
            task_id,
            priority,
            tokens_estimated,
            created_at: Instant::now(),
        }
    }

    /// Get latency since creation.
    pub fn latency(&self) -> Duration {
        self.created_at.elapsed()
    }
}

impl PartialEq for InferenceRequest {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.task_id.0 == other.task_id.0
    }
}

impl Eq for InferenceRequest {}

impl Ord for InferenceRequest {
    fn cmp(&self, other: &Self) -> Ordering {
        // Higher priority first, then earlier timestamp
        match self.priority.cmp(&other.priority) {
            Ordering::Equal => other.created_at.cmp(&self.created_at),
            other => other,
        }
    }
}

impl PartialOrd for InferenceRequest {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Preemption event record.
#[derive(Debug, Clone)]
pub struct PreemptionEvent {
    /// Task that was preempted
    pub preempted_task: TaskId,
    /// Task that caused the preemption
    pub preempted_by: TaskId,
    /// When the preemption occurred
    pub timestamp: Instant,
}

/// LLM realtime scheduler with preemption support.
pub struct LlmRealtimeScheduling {
    /// Priority queue of pending requests
    request_queue: Arc<Mutex<BinaryHeap<InferenceRequest>>>,
    /// Active requests: agent_key -> (task_id, request)
    active_requests: Arc<Mutex<HashMap<String, (TaskId, InferenceRequest)>>>,
    /// Completed requests with latency
    completed_requests: Arc<Mutex<Vec<(TaskId, Duration)>>>,
    /// Preemption events
    preemption_events: Arc<Mutex<Vec<PreemptionEvent>>>,
    /// Configuration
    config: LlmRealtimeConfig,
}

impl LlmRealtimeScheduling {
    /// Create a new LLM realtime scheduler.
    pub fn new(config: LlmRealtimeConfig) -> Self {
        LlmRealtimeScheduling {
            request_queue: Arc::new(Mutex::new(BinaryHeap::new())),
            active_requests: Arc::new(Mutex::new(HashMap::new())),
            completed_requests: Arc::new(Mutex::new(Vec::new())),
            preemption_events: Arc::new(Mutex::new(Vec::new())),
            config,
        }
    }

    /// Add an inference request to the queue.
    pub fn add_request(&self, request: InferenceRequest) {
        let mut queue = self.request_queue.lock();

        // Enforce queue depth limit
        if queue.len() >= self.config.priority_queue_depth {
            // Drop lowest priority request
            let mut temp: Vec<InferenceRequest> = queue.drain().collect();
            temp.sort_by(|a, b| b.cmp(a)); // Sort descending
            temp.truncate(self.config.priority_queue_depth - 1);
            *queue = temp.into_iter().collect();
        }

        queue.push(request);
    }

    /// Get the number of requests in queue.
    pub fn queue_length(&self) -> usize {
        self.request_queue.lock().len()
    }

    /// Get the number of active requests.
    pub fn active_count(&self) -> usize {
        self.active_requests.lock().len()
    }

    /// Get the number of completed requests.
    pub fn completed_count(&self) -> usize {
        self.completed_requests.lock().len()
    }

    /// Get preemption event count.
    pub fn preemption_count(&self) -> usize {
        self.preemption_events.lock().len()
    }

    /// Check SLA metrics (latency percentiles).
    pub fn sla_check(&self) -> SlaMetrics {
        let completed = self.completed_requests.lock();

        if completed.is_empty() {
            return SlaMetrics::default();
        }

        let mut latencies: Vec<f64> = completed.iter().map(|(_, d)| d.as_secs_f64()).collect();
        latencies.sort_by(|a, b| a.partial_cmp(b).unwrap_or(Ordering::Equal));

        let p50_idx = latencies.len() / 2;
        let p95_idx = (latencies.len() as f64 * 0.95) as usize;
        let p99_idx = (latencies.len() as f64 * 0.99) as usize;

        SlaMetrics {
            p50: Duration::from_secs_f64(latencies.get(p50_idx).copied().unwrap_or(0.0)),
            p95: Duration::from_secs_f64(latencies.get(p95_idx).copied().unwrap_or(0.0)),
            p99: Duration::from_secs_f64(latencies.get(p99_idx).copied().unwrap_or(0.0)),
            total_requests: completed.len(),
        }
    }

    /// Get request statistics.
    pub fn request_stats(&self) -> RequestStats {
        let completed = self.completed_requests.lock();
        let active = self.active_requests.lock();
        let preemptions = self.preemption_events.lock();

        let total_latency: f64 = completed.iter().map(|(_, d)| d.as_secs_f64()).sum();
        let avg_latency = if !completed.is_empty() {
            Duration::from_secs_f64(total_latency / completed.len() as f64)
        } else {
            Duration::from_secs(0)
        };

        RequestStats {
            total_requests: completed.len(),
            active_requests: active.len(),
            avg_latency,
            preemption_count: preemptions.len(),
        }
    }

    /// Try to preempt lowest priority active request.
    fn try_preempt(&self, new_request: &InferenceRequest) -> Option<String> {
        if !self.config.preemption_enabled {
            return None;
        }

        let active = self.active_requests.lock();

        // Find lowest priority active request
        let mut min_priority = 255u8;
        let mut preempt_agent: Option<String> = None;

        for (agent_key, (_, request)) in active.iter() {
            if request.priority < min_priority && new_request.priority > request.priority {
                min_priority = request.priority;
                preempt_agent = Some(agent_key.clone());
            }
        }

        preempt_agent
    }

    /// Convert AgentId to string key.
    fn agent_key(agent_id: &AgentId) -> String {
        match agent_id {
            AgentId::Nydus(id) => format!("nydus:{}", id.0),
            AgentId::Queen(id) => format!("queen:{}", id.0),
            AgentId::Overlord(id) => format!("overlord:{}", id),
            AgentId::Overmind(id) => format!("overmind:{}", id),
            AgentId::Validator => "validator".to_string(),
            AgentId::Operator => "operator".to_string(),
        }
    }
}

impl Scheduling for LlmRealtimeScheduling {
    fn next_task(&mut self, agent_id: AgentId) -> Result<Option<TaskId>> {
        let mut queue = self.request_queue.lock();
        let mut active = self.active_requests.lock();

        // Check if we're at max concurrent limit
        if active.len() >= self.config.max_concurrent {
            // Try to preempt if there's a high-priority request
            if let Some(next_request) = queue.peek() {
                if let Some(preempt_agent_key) = self.try_preempt(next_request) {
                    // Record preemption event
                    if let Some((preempted_task, _)) = active.remove(&preempt_agent_key) {
                        let event = PreemptionEvent {
                            preempted_task: preempted_task.clone(),
                            preempted_by: next_request.task_id.clone(),
                            timestamp: Instant::now(),
                        };

                        let mut events = self.preemption_events.lock();
                        events.push(event);

                        // Re-queue the preempted task (at lower priority)
                        let preempted_request = InferenceRequest::new(
                            preempted_task,
                            0, // Lowest priority
                            0,
                        );
                        queue.push(preempted_request);
                    }
                } else {
                    return Ok(None); // At max concurrent, can't preempt
                }
            } else {
                return Ok(None); // No requests in queue
            }
        }

        // Dequeue highest priority request
        if let Some(request) = queue.pop() {
            let task_id = request.task_id.clone();
            let agent_key = Self::agent_key(&agent_id);

            active.insert(agent_key, (task_id.clone(), request));

            Ok(Some(task_id))
        } else {
            Ok(None)
        }
    }

    fn notify_completion(&mut self, task_id: TaskId, agent_id: AgentId) -> Result<()> {
        let agent_key = Self::agent_key(&agent_id);

        let mut active = self.active_requests.lock();

        if let Some((_, request)) = active.remove(&agent_key) {
            let latency = request.latency();

            let mut completed = self.completed_requests.lock();
            completed.push((task_id, latency));
        }

        Ok(())
    }

    fn next_interval(&self) -> Duration {
        // Very low latency for LLM inference
        Duration::from_millis(10)
    }

    fn should_terminate(&self) -> bool {
        let queue = self.request_queue.lock();
        let active = self.active_requests.lock();

        queue.is_empty() && active.is_empty()
    }
}

/// SLA metrics.
#[derive(Debug, Clone, Default)]
pub struct SlaMetrics {
    pub p50: Duration,
    pub p95: Duration,
    pub p99: Duration,
    pub total_requests: usize,
}

/// Request statistics.
#[derive(Debug, Clone)]
pub struct RequestStats {
    pub total_requests: usize,
    pub active_requests: usize,
    pub avg_latency: Duration,
    pub preemption_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::QueenId;

    #[test]
    fn test_llm_realtime_basic() {
        let config = LlmRealtimeConfig::default();
        let mut scheduler = LlmRealtimeScheduling::new(config);

        let request1 = InferenceRequest::new(TaskId("task1".to_string()), 100, 500);
        let request2 = InferenceRequest::new(TaskId("task2".to_string()), 200, 300);

        scheduler.add_request(request1);
        scheduler.add_request(request2);

        assert_eq!(scheduler.queue_length(), 2);

        let agent_id = AgentId::Queen(QueenId("Q0".to_string()));

        // Should get higher priority first
        let task1 = scheduler.next_task(agent_id.clone()).unwrap();
        assert_eq!(task1, Some(TaskId("task2".to_string())));
    }

    #[test]
    fn test_llm_realtime_max_concurrent() {
        let config = LlmRealtimeConfig {
            max_concurrent: 2,
            preemption_enabled: false,
            ..Default::default()
        };
        let mut scheduler = LlmRealtimeScheduling::new(config);

        scheduler.add_request(InferenceRequest::new(TaskId("task1".to_string()), 100, 500));
        scheduler.add_request(InferenceRequest::new(TaskId("task2".to_string()), 100, 500));
        scheduler.add_request(InferenceRequest::new(TaskId("task3".to_string()), 100, 500));

        let agent1 = AgentId::Queen(QueenId("Q0".to_string()));
        let agent2 = AgentId::Queen(QueenId("Q1".to_string()));

        // First two should succeed
        assert!(scheduler.next_task(agent1.clone()).unwrap().is_some());
        assert!(scheduler.next_task(agent2.clone()).unwrap().is_some());

        // Third should fail (max concurrent reached, preemption disabled)
        let agent3 = AgentId::Queen(QueenId("Q2".to_string()));
        assert!(scheduler.next_task(agent3).unwrap().is_none());
    }

    #[test]
    fn test_llm_realtime_sla_check() {
        let config = LlmRealtimeConfig::default();
        let mut scheduler = LlmRealtimeScheduling::new(config);

        let request = InferenceRequest::new(TaskId("task1".to_string()), 100, 500);
        scheduler.add_request(request);

        let agent_id = AgentId::Queen(QueenId("Q0".to_string()));
        let task = scheduler.next_task(agent_id.clone()).unwrap().unwrap();

        std::thread::sleep(Duration::from_millis(10));

        scheduler.notify_completion(task, agent_id).unwrap();

        let sla = scheduler.sla_check();
        assert_eq!(sla.total_requests, 1);
        assert!(sla.p50 > Duration::from_millis(0));
    }

    #[test]
    fn test_llm_realtime_request_stats() {
        let config = LlmRealtimeConfig::default();
        let scheduler = LlmRealtimeScheduling::new(config);

        scheduler.add_request(InferenceRequest::new(TaskId("task1".to_string()), 100, 500));

        let stats = scheduler.request_stats();
        assert_eq!(stats.total_requests, 0); // None completed yet
        assert_eq!(stats.active_requests, 0);
    }
}
