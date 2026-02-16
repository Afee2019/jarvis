//! Generic OpenAI-compatible provider.
//! Most LLM APIs follow the same `/v1/chat/completions` format.
//! This module provides a single implementation that works for all of them.

use crate::providers::traits::{
    ChatMessage, ChatResponse as ProviderChatResponse, FunctionCall, Provider, ToolCall,
    ToolDefinition,
};
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// A provider that speaks the OpenAI-compatible chat completions API.
/// Used by: Venice, Vercel AI Gateway, Cloudflare AI Gateway, Moonshot,
/// Synthetic, `OpenCode` Zen, `Z.AI`, `GLM`, `MiniMax`, Bedrock, Qianfan, Groq, Mistral, `xAI`, etc.
pub struct OpenAiCompatibleProvider {
    pub(crate) name: String,
    pub(crate) base_url: String,
    pub(crate) api_key: Option<String>,
    pub(crate) auth_header: AuthStyle,
    client: Client,
}

/// How the provider expects the API key to be sent.
#[derive(Debug, Clone)]
pub enum AuthStyle {
    /// `Authorization: Bearer <key>`
    Bearer,
    /// `x-api-key: <key>` (used by some Chinese providers)
    XApiKey,
    /// Custom header name
    Custom(String),
}

impl OpenAiCompatibleProvider {
    pub fn new(name: &str, base_url: &str, api_key: Option<&str>, auth_style: AuthStyle) -> Self {
        Self {
            name: name.to_string(),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.map(ToString::to_string),
            auth_header: auth_style,
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    /// Build the full URL for chat completions, detecting if `base_url` already includes the path.
    /// This allows custom providers with non-standard endpoints (e.g., `VolcEngine` ARK uses
    /// `/api/coding/v3/chat/completions` instead of `/v1/chat/completions`).
    fn chat_completions_url(&self) -> String {
        // If base_url already contains "chat/completions", use it as-is
        if self.base_url.contains("chat/completions") {
            self.base_url.clone()
        } else {
            format!("{}/chat/completions", self.base_url)
        }
    }

    /// Build the full URL for responses API, detecting if `base_url` already includes the path.
    fn responses_url(&self) -> String {
        // If base_url already contains "responses", use it as-is
        if self.base_url.contains("responses") {
            self.base_url.clone()
        } else {
            format!("{}/v1/responses", self.base_url)
        }
    }
}

// ── Wire format types for the simple chat_with_system path ──────────

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f64,
}

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct WireChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<WireToolCall>>,
}

// ── Wire format types for tool-calling path ─────────────────────────

/// Request body for the tool-calling API.
#[derive(Debug, Serialize)]
struct ToolChatRequest {
    model: String,
    messages: Vec<WireMessage>,
    temperature: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolDefinition>>,
}

/// A message in the `OpenAI` wire format (untagged role variants).
#[derive(Debug, Serialize)]
struct WireMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<WireToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

/// Tool call in `OpenAI` wire format.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct WireToolCall {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    function: WireFunctionCall,
}

/// Function call in `OpenAI` wire format.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct WireFunctionCall {
    name: String,
    #[serde(deserialize_with = "deserialize_arguments")]
    arguments: String,
}

/// `DeepSeek` compatibility: arguments can be a string or an object.
fn deserialize_arguments<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value: serde_json::Value = Deserialize::deserialize(deserializer)?;
    match value {
        serde_json::Value::String(s) => Ok(s),
        other => Ok(other.to_string()),
    }
}

impl From<&ChatMessage> for WireMessage {
    fn from(msg: &ChatMessage) -> Self {
        match msg {
            ChatMessage::System { content } => WireMessage {
                role: "system".into(),
                content: Some(content.clone()),
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage::User { content } => WireMessage {
                role: "user".into(),
                content: Some(content.clone()),
                tool_calls: None,
                tool_call_id: None,
            },
            ChatMessage::Assistant {
                content,
                tool_calls,
            } => WireMessage {
                role: "assistant".into(),
                content: content.clone(),
                tool_calls: tool_calls.as_ref().map(|tcs| {
                    tcs.iter()
                        .map(|tc| WireToolCall {
                            id: tc.id.clone(),
                            kind: "function".into(),
                            function: WireFunctionCall {
                                name: tc.function.name.clone(),
                                arguments: tc.function.arguments.clone(),
                            },
                        })
                        .collect()
                }),
                tool_call_id: None,
            },
            ChatMessage::Tool {
                tool_call_id,
                content,
            } => WireMessage {
                role: "tool".into(),
                content: Some(content.clone()),
                tool_calls: None,
                tool_call_id: Some(tool_call_id.clone()),
            },
        }
    }
}

// ── Responses API types (unchanged) ─────────────────────────────────

#[derive(Debug, Serialize)]
struct ResponsesRequest {
    model: String,
    input: Vec<ResponsesInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Debug, Serialize)]
struct ResponsesInput {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ResponsesResponse {
    #[serde(default)]
    output: Vec<ResponsesOutput>,
    #[serde(default)]
    output_text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponsesOutput {
    #[serde(default)]
    content: Vec<ResponsesContent>,
}

#[derive(Debug, Deserialize)]
struct ResponsesContent {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
}

fn first_nonempty(text: Option<&str>) -> Option<String> {
    text.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn extract_responses_text(response: &ResponsesResponse) -> Option<String> {
    if let Some(text) = first_nonempty(response.output_text.as_deref()) {
        return Some(text);
    }

    for item in &response.output {
        for content in &item.content {
            if content.kind.as_deref() == Some("output_text") {
                if let Some(text) = first_nonempty(content.text.as_deref()) {
                    return Some(text);
                }
            }
        }
    }

    for item in &response.output {
        for content in &item.content {
            if let Some(text) = first_nonempty(content.text.as_deref()) {
                return Some(text);
            }
        }
    }

    None
}

// ── Provider implementation ─────────────────────────────────────────

impl OpenAiCompatibleProvider {
    fn apply_auth_header(
        &self,
        req: reqwest::RequestBuilder,
        api_key: &str,
    ) -> reqwest::RequestBuilder {
        match &self.auth_header {
            AuthStyle::Bearer => req.header("Authorization", format!("Bearer {api_key}")),
            AuthStyle::XApiKey => req.header("x-api-key", api_key),
            AuthStyle::Custom(header) => req.header(header, api_key),
        }
    }

    fn require_api_key(&self) -> anyhow::Result<&str> {
        self.api_key.as_deref().ok_or_else(|| {
            anyhow::anyhow!(
                "{} API key not set. Run `jarvis onboard` or set the appropriate env var.",
                self.name
            )
        })
    }

    async fn chat_via_responses(
        &self,
        api_key: &str,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
    ) -> anyhow::Result<String> {
        let request = ResponsesRequest {
            model: model.to_string(),
            input: vec![ResponsesInput {
                role: "user".to_string(),
                content: message.to_string(),
            }],
            instructions: system_prompt.map(str::to_string),
            stream: Some(false),
        };

        let url = self.responses_url();

        let response = self
            .apply_auth_header(self.client.post(&url).json(&request), api_key)
            .send()
            .await?;

        if !response.status().is_success() {
            let error = response.text().await?;
            anyhow::bail!("{} Responses API error: {error}", self.name);
        }

        let responses: ResponsesResponse = response.json().await?;

        extract_responses_text(&responses)
            .ok_or_else(|| anyhow::anyhow!("No response from {} Responses API", self.name))
    }
}

#[async_trait]
impl Provider for OpenAiCompatibleProvider {
    async fn chat_with_system(
        &self,
        system_prompt: Option<&str>,
        message: &str,
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<String> {
        let api_key = self.require_api_key()?;

        let mut messages = Vec::new();

        if let Some(sys) = system_prompt {
            messages.push(Message {
                role: "system".to_string(),
                content: sys.to_string(),
            });
        }

        messages.push(Message {
            role: "user".to_string(),
            content: message.to_string(),
        });

        let request = ChatRequest {
            model: model.to_string(),
            messages,
            temperature,
        };

        let url = self.chat_completions_url();

        let response = self
            .apply_auth_header(self.client.post(&url).json(&request), api_key)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error = response.text().await?;

            if status == reqwest::StatusCode::NOT_FOUND {
                return self
                    .chat_via_responses(api_key, system_prompt, message, model)
                    .await
                    .map_err(|responses_err| {
                        anyhow::anyhow!(
                            "{} API error: {error} (chat completions unavailable; responses fallback failed: {responses_err})",
                            self.name
                        )
                    });
            }

            anyhow::bail!("{} API error: {error}", self.name);
        }

        let chat_response: WireChatResponse = response.json().await?;

        chat_response
            .choices
            .into_iter()
            .next()
            .and_then(|c| c.message.content)
            .ok_or_else(|| anyhow::anyhow!("No response from {}", self.name))
    }

    async fn chat_with_tools(
        &self,
        messages: &[ChatMessage],
        tools: &[ToolDefinition],
        model: &str,
        temperature: f64,
    ) -> anyhow::Result<ProviderChatResponse> {
        let api_key = self.require_api_key()?;

        let wire_messages: Vec<WireMessage> = messages.iter().map(WireMessage::from).collect();

        let tools_field = if tools.is_empty() {
            None
        } else {
            Some(tools.to_vec())
        };

        let request = ToolChatRequest {
            model: model.to_string(),
            messages: wire_messages,
            temperature,
            tools: tools_field,
        };

        let url = self.chat_completions_url();

        let response = self
            .apply_auth_header(self.client.post(&url).json(&request), api_key)
            .send()
            .await?;

        if !response.status().is_success() {
            let error = response.text().await?;
            anyhow::bail!("{} API error: {error}", self.name);
        }

        let chat_response: WireChatResponse = response.json().await?;

        let choice = chat_response
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No response from {}", self.name))?;

        let msg = choice.message;

        // Check if the model returned tool calls
        if let Some(wire_tool_calls) = msg.tool_calls {
            if !wire_tool_calls.is_empty() {
                let tool_calls: Vec<ToolCall> = wire_tool_calls
                    .into_iter()
                    .map(|wtc| ToolCall {
                        id: wtc.id,
                        function: FunctionCall {
                            name: wtc.function.name,
                            arguments: wtc.function.arguments,
                        },
                    })
                    .collect();
                return Ok(ProviderChatResponse::ToolUse {
                    tool_calls,
                    text: msg.content,
                });
            }
        }

        // Pure text response
        let text = msg
            .content
            .ok_or_else(|| anyhow::anyhow!("No content in response from {}", self.name))?;
        Ok(ProviderChatResponse::Text(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_provider(name: &str, url: &str, key: Option<&str>) -> OpenAiCompatibleProvider {
        OpenAiCompatibleProvider::new(name, url, key, AuthStyle::Bearer)
    }

    #[test]
    fn creates_with_key() {
        let p = make_provider("venice", "https://api.venice.ai", Some("vn-key"));
        assert_eq!(p.name, "venice");
        assert_eq!(p.base_url, "https://api.venice.ai");
        assert_eq!(p.api_key.as_deref(), Some("vn-key"));
    }

    #[test]
    fn creates_without_key() {
        let p = make_provider("test", "https://example.com", None);
        assert!(p.api_key.is_none());
    }

    #[test]
    fn strips_trailing_slash() {
        let p = make_provider("test", "https://example.com/", None);
        assert_eq!(p.base_url, "https://example.com");
    }

    #[tokio::test]
    async fn chat_fails_without_key() {
        let p = make_provider("Venice", "https://api.venice.ai", None);
        let result = p
            .chat_with_system(None, "hello", "llama-3.3-70b", 0.7)
            .await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Venice API key not set"));
    }

    #[test]
    fn request_serializes_correctly() {
        let req = ChatRequest {
            model: "llama-3.3-70b".to_string(),
            messages: vec![
                Message {
                    role: "system".to_string(),
                    content: "You are Jarvis".to_string(),
                },
                Message {
                    role: "user".to_string(),
                    content: "hello".to_string(),
                },
            ],
            temperature: 0.7,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("llama-3.3-70b"));
        assert!(json.contains("system"));
        assert!(json.contains("user"));
    }

    #[test]
    fn response_deserializes() {
        let json = r#"{"choices":[{"message":{"content":"Hello from Venice!"}}]}"#;
        let resp: WireChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            resp.choices[0].message.content.as_deref(),
            Some("Hello from Venice!")
        );
    }

    #[test]
    fn response_empty_choices() {
        let json = r#"{"choices":[]}"#;
        let resp: WireChatResponse = serde_json::from_str(json).unwrap();
        assert!(resp.choices.is_empty());
    }

    #[test]
    fn response_with_tool_calls_deserializes() {
        let json = r#"{
            "choices": [{
                "message": {
                    "content": null,
                    "tool_calls": [{
                        "id": "call_abc123",
                        "type": "function",
                        "function": {
                            "name": "shell",
                            "arguments": "{\"command\":\"date\"}"
                        }
                    }]
                }
            }]
        }"#;
        let resp: WireChatResponse = serde_json::from_str(json).unwrap();
        let msg = &resp.choices[0].message;
        assert!(msg.content.is_none());
        let tcs = msg.tool_calls.as_ref().unwrap();
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].id, "call_abc123");
        assert_eq!(tcs[0].function.name, "shell");
        assert!(tcs[0].function.arguments.contains("date"));
    }

    #[test]
    fn response_with_tool_calls_and_text_deserializes() {
        let json = r#"{
            "choices": [{
                "message": {
                    "content": "Let me check.",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "shell",
                            "arguments": "{\"command\":\"date\"}"
                        }
                    }]
                }
            }]
        }"#;
        let resp: WireChatResponse = serde_json::from_str(json).unwrap();
        let msg = &resp.choices[0].message;
        assert_eq!(msg.content.as_deref(), Some("Let me check."));
        assert!(msg.tool_calls.is_some());
    }

    #[test]
    fn deepseek_arguments_as_object_deserializes() {
        // DeepSeek may return arguments as an object instead of a string
        let json = r#"{
            "choices": [{
                "message": {
                    "content": null,
                    "tool_calls": [{
                        "id": "call_ds1",
                        "type": "function",
                        "function": {
                            "name": "shell",
                            "arguments": {"command": "date"}
                        }
                    }]
                }
            }]
        }"#;
        let resp: WireChatResponse = serde_json::from_str(json).unwrap();
        let tc = &resp.choices[0].message.tool_calls.as_ref().unwrap()[0];
        assert_eq!(tc.function.name, "shell");
        // When arguments is an object, it gets serialized to a JSON string
        let parsed: serde_json::Value = serde_json::from_str(&tc.function.arguments).unwrap();
        assert_eq!(parsed["command"], "date");
    }

    #[test]
    fn wire_message_from_chat_message_system() {
        let msg = ChatMessage::System {
            content: "You are helpful.".into(),
        };
        let wire = WireMessage::from(&msg);
        assert_eq!(wire.role, "system");
        assert_eq!(wire.content.as_deref(), Some("You are helpful."));
        assert!(wire.tool_calls.is_none());
        assert!(wire.tool_call_id.is_none());
    }

    #[test]
    fn wire_message_from_chat_message_tool() {
        let msg = ChatMessage::Tool {
            tool_call_id: "call_123".into(),
            content: "result data".into(),
        };
        let wire = WireMessage::from(&msg);
        assert_eq!(wire.role, "tool");
        assert_eq!(wire.content.as_deref(), Some("result data"));
        assert_eq!(wire.tool_call_id.as_deref(), Some("call_123"));
    }

    #[test]
    fn wire_message_from_assistant_with_tool_calls() {
        let msg = ChatMessage::Assistant {
            content: None,
            tool_calls: Some(vec![ToolCall {
                id: "call_abc".into(),
                function: FunctionCall {
                    name: "shell".into(),
                    arguments: r#"{"command":"ls"}"#.into(),
                },
            }]),
        };
        let wire = WireMessage::from(&msg);
        assert_eq!(wire.role, "assistant");
        assert!(wire.content.is_none());
        let tcs = wire.tool_calls.unwrap();
        assert_eq!(tcs.len(), 1);
        assert_eq!(tcs[0].id, "call_abc");
        assert_eq!(tcs[0].kind, "function");
        assert_eq!(tcs[0].function.name, "shell");
    }

    #[test]
    fn tool_chat_request_serializes_with_tools() {
        let req = ToolChatRequest {
            model: "deepseek-chat".into(),
            messages: vec![WireMessage {
                role: "user".into(),
                content: Some("hello".into()),
                tool_calls: None,
                tool_call_id: None,
            }],
            temperature: 0.7,
            tools: Some(vec![ToolDefinition {
                kind: "function".into(),
                function: crate::providers::traits::FunctionDef {
                    name: "shell".into(),
                    description: "Run commands".into(),
                    parameters: serde_json::json!({"type": "object"}),
                },
            }]),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"tools\""));
        assert!(json.contains("\"shell\""));
    }

    #[test]
    fn tool_chat_request_omits_empty_tools() {
        let req = ToolChatRequest {
            model: "test".into(),
            messages: vec![],
            temperature: 0.7,
            tools: None,
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(!json.contains("tools"));
    }

    #[tokio::test]
    async fn chat_with_tools_fails_without_key() {
        let p = make_provider("Test", "https://example.com", None);
        let result = p
            .chat_with_tools(
                &[ChatMessage::User {
                    content: "hello".into(),
                }],
                &[],
                "model",
                0.7,
            )
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("API key not set"));
    }

    #[test]
    fn x_api_key_auth_style() {
        let p = OpenAiCompatibleProvider::new(
            "moonshot",
            "https://api.moonshot.cn",
            Some("ms-key"),
            AuthStyle::XApiKey,
        );
        assert!(matches!(p.auth_header, AuthStyle::XApiKey));
    }

    #[test]
    fn custom_auth_style() {
        let p = OpenAiCompatibleProvider::new(
            "custom",
            "https://api.example.com",
            Some("key"),
            AuthStyle::Custom("X-Custom-Key".into()),
        );
        assert!(matches!(p.auth_header, AuthStyle::Custom(_)));
    }

    #[tokio::test]
    async fn all_compatible_providers_fail_without_key() {
        let providers = vec![
            make_provider("Venice", "https://api.venice.ai", None),
            make_provider("Moonshot", "https://api.moonshot.cn", None),
            make_provider("GLM", "https://open.bigmodel.cn", None),
            make_provider("MiniMax", "https://api.minimax.chat", None),
            make_provider("Groq", "https://api.groq.com/openai", None),
            make_provider("Mistral", "https://api.mistral.ai", None),
            make_provider("xAI", "https://api.x.ai", None),
        ];

        for p in providers {
            let result = p.chat_with_system(None, "test", "model", 0.7).await;
            assert!(result.is_err(), "{} should fail without key", p.name);
            assert!(
                result.unwrap_err().to_string().contains("API key not set"),
                "{} error should mention key",
                p.name
            );
        }
    }

    #[test]
    fn responses_extracts_top_level_output_text() {
        let json = r#"{"output_text":"Hello from top-level","output":[]}"#;
        let response: ResponsesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            extract_responses_text(&response).as_deref(),
            Some("Hello from top-level")
        );
    }

    #[test]
    fn responses_extracts_nested_output_text() {
        let json =
            r#"{"output":[{"content":[{"type":"output_text","text":"Hello from nested"}]}]}"#;
        let response: ResponsesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            extract_responses_text(&response).as_deref(),
            Some("Hello from nested")
        );
    }

    #[test]
    fn responses_extracts_any_text_as_fallback() {
        let json = r#"{"output":[{"content":[{"type":"message","text":"Fallback text"}]}]}"#;
        let response: ResponsesResponse = serde_json::from_str(json).unwrap();
        assert_eq!(
            extract_responses_text(&response).as_deref(),
            Some("Fallback text")
        );
    }

    // ══════════════════════════════════════════════════════════
    // Custom endpoint path tests (Issue #114)
    // ══════════════════════════════════════════════════════════

    #[test]
    fn chat_completions_url_standard_openai() {
        let p = make_provider("openai", "https://api.openai.com/v1", None);
        assert_eq!(
            p.chat_completions_url(),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn chat_completions_url_trailing_slash() {
        let p = make_provider("test", "https://api.example.com/v1/", None);
        assert_eq!(
            p.chat_completions_url(),
            "https://api.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn chat_completions_url_volcengine_ark() {
        let p = make_provider(
            "volcengine",
            "https://ark.cn-beijing.volces.com/api/coding/v3/chat/completions",
            None,
        );
        assert_eq!(
            p.chat_completions_url(),
            "https://ark.cn-beijing.volces.com/api/coding/v3/chat/completions"
        );
    }

    #[test]
    fn chat_completions_url_custom_full_endpoint() {
        let p = make_provider(
            "custom",
            "https://my-api.example.com/v2/llm/chat/completions",
            None,
        );
        assert_eq!(
            p.chat_completions_url(),
            "https://my-api.example.com/v2/llm/chat/completions"
        );
    }

    #[test]
    fn responses_url_standard() {
        let p = make_provider("test", "https://api.example.com", None);
        assert_eq!(p.responses_url(), "https://api.example.com/v1/responses");
    }

    #[test]
    fn responses_url_custom_full_endpoint() {
        let p = make_provider(
            "custom",
            "https://my-api.example.com/api/v2/responses",
            None,
        );
        assert_eq!(
            p.responses_url(),
            "https://my-api.example.com/api/v2/responses"
        );
    }

    #[test]
    fn chat_completions_url_without_v1() {
        let p = make_provider("test", "https://api.example.com", None);
        assert_eq!(
            p.chat_completions_url(),
            "https://api.example.com/chat/completions"
        );
    }

    #[test]
    fn chat_completions_url_base_with_v1() {
        let p = make_provider("test", "https://api.example.com/v1", None);
        assert_eq!(
            p.chat_completions_url(),
            "https://api.example.com/v1/chat/completions"
        );
    }
}
