//! Circuit breaker pattern for preventing cascading failures.

use super::{Resilience, ResilienceAction, HealthStatus, RecoveryPlan, RecoveryAction, agent_key};
use crate::core::types::{TaskId, AgentId};
use anyhow::Result;
use std::collections::HashMap;
use std::time::{Duration, Instant};

// ============================================================================
// Configuration
// ============================================================================

/// Circuit breaker state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CircuitState {
    /// Circuit is closed, requests flow normally
    Closed,
    /// Circuit is open, requests are rejected
    Open,
    /// Circuit is half-open, testing if service recovered
    HalfOpen,
}

/// Configuration for circuit breaker behavior.
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening the circuit
    pub failure_threshold: usize,
    /// Number of successes in half-open state before closing
    pub success_threshold: usize,
    /// Timeout before transitioning from open to half-open
    pub timeout: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        CircuitBreakerConfig {
            failure_threshold: 5,
            success_threshold: 2,
            timeout: Duration::from_secs(60),
        }
    }
}

// ============================================================================
// Circuit Breaker
// ============================================================================

/// Circuit breaker for a single agent.
#[derive(Debug, Clone)]
struct CircuitBreaker {
    state: CircuitState,
    failure_count: usize,
    success_count: usize,
    last_failure: Option<Instant>,
    last_state_change: Instant,
}

impl CircuitBreaker {
    fn new() -> Self {
        CircuitBreaker {
            state: CircuitState::Closed,
            failure_count: 0,
            success_count: 0,
            last_failure: None,
            last_state_change: Instant::now(),
        }
    }

    fn record_failure(&mut self, config: &CircuitBreakerConfig) {
        self.failure_count += 1;
        self.last_failure = Some(Instant::now());
        self.success_count = 0;

        if self.state == CircuitState::Closed && self.failure_count >= config.failure_threshold {
            self.state = CircuitState::Open;
            self.last_state_change = Instant::now();
        } else if self.state == CircuitState::HalfOpen {
            // Failure in half-open state reopens the circuit
            self.state = CircuitState::Open;
            self.last_state_change = Instant::now();
        }
    }

    fn record_success(&mut self, config: &CircuitBreakerConfig) {
        self.success_count += 1;
        self.failure_count = 0;

        if self.state == CircuitState::HalfOpen && self.success_count >= config.success_threshold {
            self.state = CircuitState::Closed;
            self.last_state_change = Instant::now();
        }
    }

    fn should_allow_request(&mut self, config: &CircuitBreakerConfig) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if timeout has elapsed to transition to half-open
                if let Some(last_failure) = self.last_failure {
                    let elapsed = Instant::now().duration_since(last_failure);
                    if elapsed >= config.timeout {
                        self.state = CircuitState::HalfOpen;
                        self.last_state_change = Instant::now();
                        self.success_count = 0;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true,
        }
    }
}

// ============================================================================
// Implementation
// ============================================================================

/// Circuit breaker resilience handler.
pub struct CircuitBreakerResilience {
    config: CircuitBreakerConfig,
    breakers: HashMap<String, CircuitBreaker>,
}

impl CircuitBreakerResilience {
    /// Create a new circuit breaker resilience handler with the given configuration.
    pub fn new(config: CircuitBreakerConfig) -> Self {
        CircuitBreakerResilience {
            config,
            breakers: HashMap::new(),
        }
    }

    /// Create with default configuration.
    pub fn default() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }

    /// Get or create a circuit breaker for an agent.
    fn get_or_create_breaker(&mut self, agent_key: String) -> &mut CircuitBreaker {
        self.breakers
            .entry(agent_key)
            .or_insert_with(CircuitBreaker::new)
    }

    /// Check if a request is allowed through the circuit breaker.
    pub fn is_allowed(&mut self, agent_id: &AgentId) -> bool {
        let key = agent_key(agent_id);
        let config = self.config.clone();
        let breaker = self.get_or_create_breaker(key);
        breaker.should_allow_request(&config)
    }

    /// Record a successful operation.
    pub fn record_success(&mut self, agent_id: &AgentId) {
        let key = agent_key(agent_id);
        let config = self.config.clone();
        let breaker = self.get_or_create_breaker(key);
        breaker.record_success(&config);
    }

    /// Get the current state of a circuit breaker.
    pub fn get_state(&self, agent_id: &AgentId) -> CircuitState {
        let key = agent_key(agent_id);
        self.breakers
            .get(&key)
            .map(|b| b.state.clone())
            .unwrap_or(CircuitState::Closed)
    }

    /// Get failure count for an agent.
    pub fn get_failure_count(&self, agent_id: &AgentId) -> usize {
        let key = agent_key(agent_id);
        self.breakers
            .get(&key)
            .map(|b| b.failure_count)
            .unwrap_or(0)
    }

    /// Manually reset a circuit breaker.
    pub fn reset(&mut self, agent_id: &AgentId) {
        let key = agent_key(agent_id);
        self.breakers.insert(key, CircuitBreaker::new());
    }

    /// Get all agents with open circuits.
    pub fn get_open_circuits(&self) -> Vec<String> {
        self.breakers
            .iter()
            .filter(|(_, b)| b.state == CircuitState::Open)
            .map(|(key, _)| key.clone())
            .collect()
    }
}

impl Resilience for CircuitBreakerResilience {
    fn handle_failure(&mut self, _task_id: TaskId, agent_id: AgentId, _error: String) -> Result<ResilienceAction> {
        let agent_key_str = agent_key(&agent_id);
        let config = self.config.clone();
        let breaker = self.get_or_create_breaker(agent_key_str);

        // Record the failure
        breaker.record_failure(&config);

        // Determine action based on new state
        match breaker.state {
            CircuitState::Closed => {
                // Still closed, retry
                Ok(ResilienceAction::Retry {
                    delay: Duration::from_secs(1),
                })
            }
            CircuitState::Open => {
                // Circuit opened, abandon task
                Ok(ResilienceAction::Abandon)
            }
            CircuitState::HalfOpen => {
                // Probe failed, circuit reopened, abandon
                Ok(ResilienceAction::Abandon)
            }
        }
    }

    fn check_health(&self, agent_id: AgentId) -> Result<HealthStatus> {
        let key = agent_key(&agent_id);

        if let Some(breaker) = self.breakers.get(&key) {
            let status = match breaker.state {
                CircuitState::Closed => HealthStatus::Healthy,
                CircuitState::HalfOpen => HealthStatus::Degraded,
                CircuitState::Open => HealthStatus::Critical,
            };
            Ok(status)
        } else {
            Ok(HealthStatus::Healthy)
        }
    }

    fn plan_recovery(&mut self, agent_id: AgentId) -> Result<RecoveryPlan> {
        let key = agent_key(&agent_id);
        let breaker = self.get_or_create_breaker(key);

        let action = match breaker.state {
            CircuitState::Closed => {
                // Healthy, no recovery needed
                RecoveryAction::Resume {
                    session_id: format!("healthy_{}", agent_key(&agent_id)),
                }
            }
            CircuitState::Open => {
                // Open circuit, suggest waiting for timeout
                let elapsed = breaker.last_failure
                    .map(|t| Instant::now().duration_since(t))
                    .unwrap_or(Duration::from_secs(0));

                let remaining = self.config.timeout.saturating_sub(elapsed);

                if remaining.as_secs() > 0 {
                    RecoveryAction::Resume {
                        session_id: format!("waiting_{}s", remaining.as_secs()),
                    }
                } else {
                    RecoveryAction::Restart
                }
            }
            CircuitState::HalfOpen => {
                // Half-open, probe the agent
                RecoveryAction::Resume {
                    session_id: format!("probe_{}", agent_key(&agent_id)),
                }
            }
        };

        Ok(RecoveryPlan {
            agent_id: agent_id.clone(),
            action,
        })
    }

    fn record_failure(&mut self, _task_id: TaskId) -> Result<()> {
        // Circuit breaker doesn't track individual task failures
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_circuit_breaker_state_transitions() {
        let mut cb = CircuitBreakerResilience::new(CircuitBreakerConfig {
            failure_threshold: 3,
            success_threshold: 2,
            timeout: Duration::from_secs(1),
        });

        let agent_id = AgentId::Validator;

        // Should start closed
        assert_eq!(cb.get_state(&agent_id), CircuitState::Closed);

        // Record failures to open circuit
        for i in 0..3 {
            let task_id = TaskId(format!("task_{}", i));
            cb.handle_failure(task_id, agent_id.clone(), "error".to_string())
                .unwrap();
        }

        // Should now be open
        assert_eq!(cb.get_state(&agent_id), CircuitState::Open);

        // Should reject requests while open
        assert!(!cb.is_allowed(&agent_id));

        // Wait for timeout
        std::thread::sleep(Duration::from_millis(1100));

        // Should transition to half-open and allow probe
        assert!(cb.is_allowed(&agent_id));
        assert_eq!(cb.get_state(&agent_id), CircuitState::HalfOpen);

        // Record successes to close circuit
        cb.record_success(&agent_id);
        cb.record_success(&agent_id);

        // Should now be closed
        assert_eq!(cb.get_state(&agent_id), CircuitState::Closed);
    }

    #[test]
    fn test_half_open_failure_reopens() {
        let mut cb = CircuitBreakerResilience::new(CircuitBreakerConfig {
            failure_threshold: 2,
            success_threshold: 2,
            timeout: Duration::from_secs(1),
        });

        let agent_id = AgentId::Validator;

        // Open the circuit
        for i in 0..2 {
            let task_id = TaskId(format!("task_{}", i));
            cb.handle_failure(task_id, agent_id.clone(), "error".to_string())
                .unwrap();
        }

        assert_eq!(cb.get_state(&agent_id), CircuitState::Open);

        // Wait and transition to half-open
        std::thread::sleep(Duration::from_millis(1100));
        cb.is_allowed(&agent_id);
        assert_eq!(cb.get_state(&agent_id), CircuitState::HalfOpen);

        // Fail in half-open state
        cb.handle_failure(TaskId("probe".to_string()), agent_id.clone(), "error".to_string())
            .unwrap();

        // Should reopen
        assert_eq!(cb.get_state(&agent_id), CircuitState::Open);
    }
}
