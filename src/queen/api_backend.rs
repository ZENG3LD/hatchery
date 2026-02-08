use std::time::Duration;
use serde::{Serialize, Deserialize};
use anyhow::{Result, anyhow};
use chrono::{DateTime, Utc};

// ============================================================================
// API Format Types
// ============================================================================

/// Supported API format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ApiFormat {
    /// OpenAI-compatible (ChatGPT, Groq, Together, etc.)
    OpenAI,
    /// Anthropic Messages API
    Anthropic,
}

/// A message in the conversation history.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: Role,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Role {
    System,
    User,
    Assistant,
}

/// Configuration for an API backend.
#[derive(Debug, Clone)]
pub struct ApiBackendConfig {
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,  // loaded from env var at runtime
    pub api_key_env: String,      // env var name (e.g., "ANTHROPIC_API_KEY")
    pub format: ApiFormat,
    pub max_tokens: usize,
    pub temperature: f32,
    pub timeout: Duration,
    pub system_prompt: Option<String>,
}

impl Default for ApiBackendConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.anthropic.com".to_string(),
            model: "claude-sonnet-4-5-20250929".to_string(),
            api_key: None,
            api_key_env: "ANTHROPIC_API_KEY".to_string(),
            format: ApiFormat::Anthropic,
            max_tokens: 4096,
            temperature: 0.0,
            timeout: Duration::from_secs(120),
            system_prompt: None,
        }
    }
}

// ============================================================================
// Request/Response types
// ============================================================================

/// A chat completion request (format-agnostic).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<RequestMessage>,
    pub max_tokens: usize,
    pub temperature: f32,
    pub system: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestMessage {
    pub role: String,
    pub content: String,
}

/// A chat completion response (format-agnostic).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub content: String,
    pub model: String,
    pub input_tokens: usize,
    pub output_tokens: usize,
    pub stop_reason: Option<String>,
}

// ============================================================================
// Format Adapters
// ============================================================================

/// Convert a ChatRequest to OpenAI-compatible JSON.
pub fn to_openai_request(req: &ChatRequest) -> serde_json::Value {
    let mut messages = Vec::new();
    if let Some(system) = &req.system {
        messages.push(serde_json::json!({
            "role": "system",
            "content": system,
        }));
    }
    for msg in &req.messages {
        messages.push(serde_json::json!({
            "role": msg.role,
            "content": msg.content,
        }));
    }
    serde_json::json!({
        "model": req.model,
        "messages": messages,
        "max_tokens": req.max_tokens,
        "temperature": req.temperature,
    })
}

/// Convert a ChatRequest to Anthropic Messages API JSON.
pub fn to_anthropic_request(req: &ChatRequest) -> serde_json::Value {
    let messages: Vec<_> = req.messages.iter().map(|m| {
        serde_json::json!({
            "role": m.role,
            "content": m.content,
        })
    }).collect();

    let mut body = serde_json::json!({
        "model": req.model,
        "messages": messages,
        "max_tokens": req.max_tokens,
        "temperature": req.temperature,
    });

    if let Some(system) = &req.system {
        body["system"] = serde_json::Value::String(system.clone());
    }

    body
}

/// Parse OpenAI-compatible response JSON into ChatResponse.
pub fn parse_openai_response(json: &serde_json::Value) -> Result<ChatResponse> {
    let content = json.get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| anyhow!("Invalid OpenAI response: missing choices[0].message.content"))?;

    let empty_usage = serde_json::json!({});
    let usage = json.get("usage").unwrap_or(&empty_usage);

    Ok(ChatResponse {
        content: content.to_string(),
        model: json.get("model").and_then(|m| m.as_str()).unwrap_or("unknown").to_string(),
        input_tokens: usage.get("prompt_tokens").and_then(|t| t.as_u64()).unwrap_or(0) as usize,
        output_tokens: usage.get("completion_tokens").and_then(|t| t.as_u64()).unwrap_or(0) as usize,
        stop_reason: json.get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("finish_reason"))
            .and_then(|r| r.as_str())
            .map(String::from),
    })
}

/// Parse Anthropic Messages API response JSON into ChatResponse.
pub fn parse_anthropic_response(json: &serde_json::Value) -> Result<ChatResponse> {
    let content = json.get("content")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("text"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow!("Invalid Anthropic response: missing content[0].text"))?;

    let empty_usage = serde_json::json!({});
    let usage = json.get("usage").unwrap_or(&empty_usage);

    Ok(ChatResponse {
        content: content.to_string(),
        model: json.get("model").and_then(|m| m.as_str()).unwrap_or("unknown").to_string(),
        input_tokens: usage.get("input_tokens").and_then(|t| t.as_u64()).unwrap_or(0) as usize,
        output_tokens: usage.get("output_tokens").and_then(|t| t.as_u64()).unwrap_or(0) as usize,
        stop_reason: json.get("stop_reason").and_then(|r| r.as_str()).map(String::from),
    })
}

// ============================================================================
// Conversation Manager
// ============================================================================

/// Manages multi-turn conversation history for an API-backed worker.
pub struct ConversationManager {
    history: Vec<ConversationMessage>,
    max_history: usize,
    total_input_tokens: usize,
    total_output_tokens: usize,
}

impl ConversationManager {
    pub fn new(max_history: usize) -> Self {
        Self { history: Vec::new(), max_history, total_input_tokens: 0, total_output_tokens: 0 }
    }

    /// Add a user message.
    pub fn add_user_message(&mut self, content: &str) {
        self.history.push(ConversationMessage {
            role: Role::User,
            content: content.to_string(),
            timestamp: Utc::now(),
        });
        self.trim();
    }

    /// Add an assistant response.
    pub fn add_assistant_message(&mut self, content: &str) {
        self.history.push(ConversationMessage {
            role: Role::Assistant,
            content: content.to_string(),
            timestamp: Utc::now(),
        });
        self.trim();
    }

    /// Update token counts from a response.
    pub fn update_tokens(&mut self, input: usize, output: usize) {
        self.total_input_tokens += input;
        self.total_output_tokens += output;
    }

    /// Build request messages from history.
    pub fn to_request_messages(&self) -> Vec<RequestMessage> {
        self.history.iter().map(|m| RequestMessage {
            role: match m.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
            }.to_string(),
            content: m.content.clone(),
        }).collect()
    }

    /// Build a full ChatRequest.
    pub fn build_request(&self, config: &ApiBackendConfig) -> ChatRequest {
        ChatRequest {
            model: config.model.clone(),
            messages: self.to_request_messages(),
            max_tokens: config.max_tokens,
            temperature: config.temperature,
            system: config.system_prompt.clone(),
        }
    }

    /// Get total tokens used.
    pub fn total_tokens(&self) -> usize {
        self.total_input_tokens + self.total_output_tokens
    }

    /// Number of messages in history.
    pub fn len(&self) -> usize { self.history.len() }

    /// Whether history is empty.
    pub fn is_empty(&self) -> bool { self.history.is_empty() }

    /// Clear conversation history.
    pub fn clear(&mut self) {
        self.history.clear();
    }

    /// Trim to max_history (keep recent).
    fn trim(&mut self) {
        if self.history.len() > self.max_history {
            let excess = self.history.len() - self.max_history;
            self.history.drain(0..excess);
        }
    }
}

// ============================================================================
// ApiWorkerBackend (stub - real HTTP later)
// ============================================================================

/// API-backed worker that sends prompts to an LLM API.
///
/// Note: HTTP calls are stubbed out — the real implementation requires
/// reqwest dependency which will be added in a future phase.
pub struct ApiWorkerBackend {
    config: ApiBackendConfig,
    conversation: ConversationManager,
    created_at: DateTime<Utc>,
}

impl ApiWorkerBackend {
    pub fn new(config: ApiBackendConfig) -> Self {
        Self {
            conversation: ConversationManager::new(50),
            config,
            created_at: Utc::now(),
        }
    }

    /// Send a prompt and get a response (STUB — returns placeholder).
    pub fn send(&mut self, prompt: &str) -> Result<ChatResponse> {
        self.conversation.add_user_message(prompt);

        // Build the request (this is real — useful for testing the format)
        let _request = self.conversation.build_request(&self.config);

        // STUB: return placeholder response
        let response = ChatResponse {
            content: format!("[STUB] API response to: {}", &prompt[..prompt.len().min(50)]),
            model: self.config.model.clone(),
            input_tokens: prompt.len() / 4,
            output_tokens: 50,
            stop_reason: Some("end_turn".to_string()),
        };

        self.conversation.add_assistant_message(&response.content);
        self.conversation.update_tokens(response.input_tokens, response.output_tokens);

        Ok(response)
    }

    /// Get conversation manager (for inspection).
    pub fn conversation(&self) -> &ConversationManager { &self.conversation }

    /// Get config.
    pub fn config(&self) -> &ApiBackendConfig { &self.config }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_openai_request_format() {
        let req = ChatRequest {
            model: "gpt-4".to_string(),
            messages: vec![
                RequestMessage { role: "user".to_string(), content: "Hello".to_string() },
            ],
            max_tokens: 1000,
            temperature: 0.7,
            system: Some("You are helpful".to_string()),
        };

        let json = to_openai_request(&req);
        assert_eq!(json["model"], "gpt-4");
        assert_eq!(json["max_tokens"], 1000);
        assert!((json["temperature"].as_f64().unwrap() - 0.7).abs() < 0.01);
        assert_eq!(json["messages"][0]["role"], "system");
        assert_eq!(json["messages"][0]["content"], "You are helpful");
        assert_eq!(json["messages"][1]["role"], "user");
        assert_eq!(json["messages"][1]["content"], "Hello");
    }

    #[test]
    fn test_to_anthropic_request_format() {
        let req = ChatRequest {
            model: "claude-3-5-sonnet-20241022".to_string(),
            messages: vec![
                RequestMessage { role: "user".to_string(), content: "Hello".to_string() },
            ],
            max_tokens: 2048,
            temperature: 0.5,
            system: Some("Be concise".to_string()),
        };

        let json = to_anthropic_request(&req);
        assert_eq!(json["model"], "claude-3-5-sonnet-20241022");
        assert_eq!(json["max_tokens"], 2048);
        assert_eq!(json["temperature"], 0.5);
        assert_eq!(json["system"], "Be concise");
        assert_eq!(json["messages"][0]["role"], "user");
        assert_eq!(json["messages"][0]["content"], "Hello");
    }

    #[test]
    fn test_parse_openai_response_valid() {
        let json = serde_json::json!({
            "model": "gpt-4",
            "choices": [{
                "message": {
                    "content": "Hi there!"
                },
                "finish_reason": "stop"
            }],
            "usage": {
                "prompt_tokens": 10,
                "completion_tokens": 5
            }
        });

        let resp = parse_openai_response(&json).unwrap();
        assert_eq!(resp.content, "Hi there!");
        assert_eq!(resp.model, "gpt-4");
        assert_eq!(resp.input_tokens, 10);
        assert_eq!(resp.output_tokens, 5);
        assert_eq!(resp.stop_reason, Some("stop".to_string()));
    }

    #[test]
    fn test_parse_anthropic_response_valid() {
        let json = serde_json::json!({
            "model": "claude-3-5-sonnet-20241022",
            "content": [{
                "text": "Hello!"
            }],
            "usage": {
                "input_tokens": 15,
                "output_tokens": 3
            },
            "stop_reason": "end_turn"
        });

        let resp = parse_anthropic_response(&json).unwrap();
        assert_eq!(resp.content, "Hello!");
        assert_eq!(resp.model, "claude-3-5-sonnet-20241022");
        assert_eq!(resp.input_tokens, 15);
        assert_eq!(resp.output_tokens, 3);
        assert_eq!(resp.stop_reason, Some("end_turn".to_string()));
    }

    #[test]
    fn test_parse_openai_response_invalid() {
        let json = serde_json::json!({
            "error": "Bad request"
        });

        let result = parse_openai_response(&json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid OpenAI response"));
    }

    #[test]
    fn test_parse_anthropic_response_invalid() {
        let json = serde_json::json!({
            "error": {
                "type": "invalid_request_error"
            }
        });

        let result = parse_anthropic_response(&json);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid Anthropic response"));
    }

    #[test]
    fn test_conversation_manager_add_and_trim() {
        let mut cm = ConversationManager::new(3);
        cm.add_user_message("Message 1");
        cm.add_assistant_message("Response 1");
        cm.add_user_message("Message 2");

        // At this point we have 3 messages (max_history), no trim yet
        assert_eq!(cm.len(), 3);

        // Adding 4th message triggers trim (keeps last 3)
        cm.add_assistant_message("Response 2");
        assert_eq!(cm.len(), 3);

        // Add one more to trigger another trim
        cm.add_user_message("Message 3");
        assert_eq!(cm.len(), 3);

        // First message should be gone, keeping: Response 1, Message 2, Response 2, Message 3
        // Wait, that's 4. Let me recalculate:
        // After Response 2: we have [Response 1, Message 2, Response 2]
        // After Message 3: we have [Message 2, Response 2, Message 3]
        let messages = cm.to_request_messages();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].content, "Message 2");
        assert_eq!(messages[1].content, "Response 2");
        assert_eq!(messages[2].content, "Message 3");
    }

    #[test]
    fn test_conversation_manager_build_request() {
        let mut cm = ConversationManager::new(10);
        cm.add_user_message("Hello");
        cm.add_assistant_message("Hi");

        let config = ApiBackendConfig {
            model: "test-model".to_string(),
            max_tokens: 500,
            temperature: 0.3,
            system_prompt: Some("Test system".to_string()),
            ..Default::default()
        };

        let req = cm.build_request(&config);
        assert_eq!(req.model, "test-model");
        assert_eq!(req.max_tokens, 500);
        assert_eq!(req.temperature, 0.3);
        assert_eq!(req.system, Some("Test system".to_string()));
        assert_eq!(req.messages.len(), 2);
        assert_eq!(req.messages[0].content, "Hello");
        assert_eq!(req.messages[1].content, "Hi");
    }

    #[test]
    fn test_conversation_manager_token_tracking() {
        let mut cm = ConversationManager::new(10);
        assert_eq!(cm.total_tokens(), 0);

        cm.update_tokens(100, 50);
        assert_eq!(cm.total_tokens(), 150);

        cm.update_tokens(200, 75);
        assert_eq!(cm.total_tokens(), 425);
    }

    #[test]
    fn test_api_worker_backend_send_stub() {
        let config = ApiBackendConfig::default();
        let mut backend = ApiWorkerBackend::new(config);

        let response = backend.send("Test prompt").unwrap();
        assert!(response.content.starts_with("[STUB]"));
        assert_eq!(backend.conversation().len(), 2); // user + assistant
        assert!(backend.conversation().total_tokens() > 0);
    }

    #[test]
    fn test_api_backend_config_default() {
        let config = ApiBackendConfig::default();
        assert_eq!(config.base_url, "https://api.anthropic.com");
        assert_eq!(config.model, "claude-sonnet-4-5-20250929");
        assert_eq!(config.api_key_env, "ANTHROPIC_API_KEY");
        assert!(matches!(config.format, ApiFormat::Anthropic));
        assert_eq!(config.max_tokens, 4096);
        assert_eq!(config.temperature, 0.0);
        assert_eq!(config.timeout, Duration::from_secs(120));
        assert_eq!(config.system_prompt, None);
    }
}
