//! Model Context Protocol (MCP) adapter.
//!
//! Simplified local implementation of MCP for tool-based agent communication.

use crate::communication::{agent_key, Communication, Message};
use crate::core::types::AgentId;
use anyhow::{anyhow, Result};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::communication::direct::{DirectCommunication, DirectConfig};

/// Configuration for McpCommunication.
#[derive(Debug, Clone)]
pub struct McpConfig {
    /// MCP endpoint identifier.
    pub endpoint: String,
    /// Direct communication config.
    pub direct_config: DirectConfig,
}

impl Default for McpConfig {
    fn default() -> Self {
        McpConfig {
            endpoint: "local://mcp".to_string(),
            direct_config: DirectConfig::default(),
        }
    }
}

/// MCP tool definition.
#[derive(Debug, Clone)]
pub struct McpTool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

impl McpTool {
    /// Create a new MCP tool.
    pub fn new(name: String, description: String, parameters: serde_json::Value) -> Self {
        McpTool {
            name,
            description,
            parameters,
        }
    }
}

/// MCP communication adapter.
///
/// Provides tool-based message passing between agents using MCP semantics.
pub struct McpCommunication {
    config: McpConfig,
    /// Underlying direct communication
    direct: DirectCommunication,
    /// Registered tools: tool_name -> (agent_id, tool)
    tools: Arc<RwLock<HashMap<String, (AgentId, McpTool)>>>,
}

impl McpCommunication {
    /// Create a new McpCommunication instance.
    pub fn new(config: McpConfig) -> Self {
        let direct = DirectCommunication::new(config.direct_config.clone());

        McpCommunication {
            config,
            direct,
            tools: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a tool provided by an agent.
    pub fn register_tool(&self, agent_id: AgentId, tool: McpTool) -> Result<()> {
        let mut tools = self.tools.write();

        if tools.contains_key(&tool.name) {
            return Err(anyhow!("Tool already registered: {}", tool.name));
        }

        tools.insert(tool.name.clone(), (agent_id, tool));
        Ok(())
    }

    /// Unregister a tool.
    pub fn unregister_tool(&self, tool_name: &str) -> Result<()> {
        let mut tools = self.tools.write();

        if tools.remove(tool_name).is_none() {
            return Err(anyhow!("Tool not found: {}", tool_name));
        }

        Ok(())
    }

    /// Get all registered tools.
    pub fn list_tools(&self) -> Vec<McpTool> {
        self.tools
            .read()
            .values()
            .map(|(_, tool)| tool.clone())
            .collect()
    }

    /// Call a tool by name.
    pub fn call_tool(
        &self,
        caller: AgentId,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<()> {
        let (tool_owner, tool_description) = {
            let tools = self.tools.read();
            let (owner, tool) = tools
                .get(tool_name)
                .ok_or_else(|| anyhow!("Tool not found: {}", tool_name))?;
            (owner.clone(), tool.description.clone())
        };

        // Create tool invocation message
        let msg = Message::new(
            caller.clone(),
            Some(tool_owner.clone()),
            serde_json::json!({
                "type": "mcp_tool_call",
                "tool": tool_name,
                "arguments": arguments,
                "description": tool_description,
            }),
        );

        // Send to tool owner
        self.direct.send(caller, tool_owner, msg)?;

        Ok(())
    }

    /// Get tool by name.
    pub fn get_tool(&self, tool_name: &str) -> Option<McpTool> {
        self.tools
            .read()
            .get(tool_name)
            .map(|(_, tool)| tool.clone())
    }

    /// Get tools provided by an agent.
    pub fn get_agent_tools(&self, agent_id: &AgentId) -> Vec<McpTool> {
        let target_key = agent_key(agent_id);
        self.tools
            .read()
            .values()
            .filter_map(|(owner, tool)| {
                if agent_key(owner) == target_key {
                    Some(tool.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get the endpoint identifier.
    pub fn endpoint(&self) -> &str {
        &self.config.endpoint
    }
}

impl Communication for McpCommunication {
    fn send(&self, from: AgentId, to: AgentId, message: Message) -> Result<()> {
        self.direct.send(from, to, message)
    }

    fn broadcast(&self, from: AgentId, message: Message) -> Result<()> {
        self.direct.broadcast(from, message)
    }

    fn subscribe(&self, agent_id: AgentId, topic: &str) -> Result<mpsc::Receiver<Message>> {
        self.direct.subscribe(agent_id, topic)
    }

    fn publish(&self, topic: &str, message: Message) -> Result<()> {
        self.direct.publish(topic, message)
    }

    fn receiver(&self, agent_id: AgentId) -> Result<mpsc::Receiver<Message>> {
        self.direct.receiver(agent_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::QueenId;

    #[test]
    fn test_tool_registration() {
        let comm = McpCommunication::new(McpConfig::default());

        let queen = AgentId::Queen(QueenId("Q1".to_string()));
        let tool = McpTool::new(
            "execute_code".to_string(),
            "Execute code snippet".to_string(),
            serde_json::json!({"code": "string"}),
        );

        comm.register_tool(queen.clone(), tool.clone()).unwrap();

        let tools = comm.list_tools();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "execute_code");

        let agent_tools = comm.get_agent_tools(&queen);
        assert_eq!(agent_tools.len(), 1);
    }

    #[tokio::test]
    async fn test_tool_invocation() {
        let comm = McpCommunication::new(McpConfig::default());

        let queen1 = AgentId::Queen(QueenId("Q1".to_string()));
        let queen2 = AgentId::Queen(QueenId("Q2".to_string()));

        // Register tool
        let tool = McpTool::new(
            "analyze".to_string(),
            "Analyze data".to_string(),
            serde_json::json!({"data": "object"}),
        );
        comm.register_tool(queen2.clone(), tool).unwrap();

        // Set up receiver
        let mut rx = comm.receiver(queen2.clone()).unwrap();

        // Call tool
        comm.call_tool(queen1, "analyze", serde_json::json!({"data": {"key": "value"}}))
            .unwrap();

        // Verify message received
        let received = rx.recv().await.unwrap();
        assert_eq!(
            received.payload["type"],
            serde_json::Value::String("mcp_tool_call".to_string())
        );
        assert_eq!(
            received.payload["tool"],
            serde_json::Value::String("analyze".to_string())
        );
    }
}
