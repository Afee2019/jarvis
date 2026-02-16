use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::tools::ToolSpec;

// ── Multi-turn chat message types (OpenAI function calling format) ───

/// A message in a multi-turn conversation with tool support.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum ChatMessage {
    System {
        content: String,
    },
    User {
        content: String,
    },
    Assistant {
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_calls: Option<Vec<ToolCall>>,
    },
    Tool {
        tool_call_id: String,
        content: String,
    },
}

/// A tool call requested by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub function: FunctionCall,
}

/// The function name and arguments for a tool call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    /// JSON-encoded arguments string.
    pub arguments: String,
}

/// A tool definition sent to the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    #[serde(rename = "type")]
    pub kind: String,
    pub function: FunctionDef,
}

/// Function metadata for a tool definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Response from a provider that supports tool calling.
#[derive(Debug, Clone)]
pub enum ChatResponse {
    /// Pure text response (no tool calls).
    Text(String),
    /// Model wants to call one or more tools.
    ToolUse {
        tool_calls: Vec<ToolCall>,
        text: Option<String>,
    },
}

/// Convert a `ToolSpec` (from the tool registry) into a `ToolDefinition` (for the API).
pub fn tool_spec_to_definition(spec: &ToolSpec) -> ToolDefinition {
    ToolDefinition {
        kind: "function".to_string(),
        function: FunctionDef {
            name: spec.name.clone(),
            description: spec.description.clone(),
            parameters: spec.parameters.clone(),
        },
    }
}

#[async_trait]
pub trait Provider: Send + Sync {
    async fn chat(&self, message: &str, model: &str, temperature: f64) -> anyhow::Result<String> {
        self.chat_with_system(None, message, model, temperature)
            .await
    }

    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String>;

    /// Multi-turn chat with tool definitions. Returns structured `ChatResponse`.
    ///
    /// Default implementation ignores tools and falls back to `chat_with_system`,
    /// extracting user message from the message list.
    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        _tools: &[ToolDefinition],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ChatResponse> {
        // Extract system prompt and last user message for fallback
        let system_prompt = messages.iter().find_map(|m| {
            if let ChatMessage::System { content } = m {
                Some(content.as_str())
            } else {
                None
            }
        });
        let user_message = messages
            .iter()
            .rev()
            .find_map(|m| {
                if let ChatMessage::User { content } = m {
                    Some(content.as_str())
                } else {
                    None
                }
            })
            .unwrap_or("");

        let text = self
            .chat_with_system(system_prompt, user_message, model, temperature)
            .await?;
        Ok(ChatResponse::Text(text))
    }

    /// Warm up the HTTP connection pool (TLS handshake, DNS, HTTP/2 setup).
    /// Default implementation is a no-op; providers with HTTP clients should override.
    async fn warmup(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_spec_to_definition_converts() {
        let spec = ToolSpec {
            name: "shell".into(),
            description: "Execute shell commands".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string" }
                },
                "required": ["command"]
            }),
        };
        let def = tool_spec_to_definition(&spec);
        assert_eq!(def.kind, "function");
        assert_eq!(def.function.name, "shell");
        assert_eq!(def.function.description, "Execute shell commands");
        assert!(def.function.parameters["properties"]["command"].is_object());
    }

    #[test]
    fn chat_message_system_serde() {
        let msg = ChatMessage::System {
            content: "You are helpful.".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"role\":\"system\""));
        assert!(json.contains("You are helpful."));
    }

    #[test]
    fn chat_message_tool_serde() {
        let msg = ChatMessage::Tool {
            tool_call_id: "call_123".into(),
            content: "result".into(),
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("\"role\":\"tool\""));
        assert!(json.contains("call_123"));
    }

    #[test]
    fn tool_call_serde_roundtrip() {
        let tc = ToolCall {
            id: "call_abc".into(),
            function: FunctionCall {
                name: "shell".into(),
                arguments: r#"{"command":"date"}"#.into(),
            },
        };
        let json = serde_json::to_string(&tc).unwrap();
        let parsed: ToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, "call_abc");
        assert_eq!(parsed.function.name, "shell");
    }

    #[test]
    fn tool_definition_serde() {
        let def = ToolDefinition {
            kind: "function".into(),
            function: FunctionDef {
                name: "test".into(),
                description: "A test".into(),
                parameters: serde_json::json!({"type": "object"}),
            },
        };
        let json = serde_json::to_string(&def).unwrap();
        assert!(json.contains("\"type\":\"function\""));
        assert!(json.contains("\"name\":\"test\""));
    }

    #[test]
    fn assistant_message_skips_none_fields() {
        let msg = ChatMessage::Assistant {
            content: Some("hello".into()),
            tool_calls: None,
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(!json.contains("tool_calls"));
    }
}
