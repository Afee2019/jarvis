use crate::config::Config;
use crate::memory::{self, Memory, MemoryCategory};
use crate::observability::{self, Observer, ObserverEvent};
use crate::providers::traits::{
    tool_spec_to_definition, ChatMessage, ChatResponse, ToolDefinition,
};
use crate::providers::{self, Provider};
use crate::runtime;
use crate::security::SecurityPolicy;
use crate::tools::{self, Tool};
use crate::util::truncate_with_ellipsis;
use anyhow::Result;
use std::fmt::Write;
use std::sync::Arc;
use std::time::Instant;

/// Build context preamble by searching memory for relevant entries
async fn build_context(mem: &dyn Memory, user_msg: &str) -> String {
    let mut context = String::new();

    // Pull relevant memories for this message
    if let Ok(entries) = mem.recall(user_msg, 5).await {
        if !entries.is_empty() {
            context.push_str("[Memory context]\n");
            for entry in &entries {
                let _ = writeln!(context, "- {}: {}", entry.key, entry.content);
            }
            context.push('\n');
        }
    }

    context
}

/// Execute a list of tool calls against the tool registry.
///
/// Returns a `ChatMessage::Tool` for each call (success or error).
pub async fn execute_tool_calls(
    tool_calls: &[crate::providers::ToolCall],
    tools: &[Box<dyn Tool>],
    security: &SecurityPolicy,
    observer: &dyn Observer,
    quiet: bool,
) -> Vec<ChatMessage> {
    let mut results = Vec::with_capacity(tool_calls.len());

    for tc in tool_calls {
        let tool_name = &tc.function.name;
        let tool_start = Instant::now();

        // Find the tool in the registry
        let tool = tools.iter().find(|t| t.name() == tool_name);
        let Some(tool) = tool else {
            tracing::warn!(tool = tool_name, "模型请求了未知工具");
            results.push(ChatMessage::Tool {
                tool_call_id: tc.id.clone(),
                content: format!("Error: 未知工具「{tool_name}」"),
            });
            continue;
        };

        // Rate limit check
        if !security.record_action() {
            tracing::warn!(tool = tool_name, "工具调用超出速率限制");
            results.push(ChatMessage::Tool {
                tool_call_id: tc.id.clone(),
                content: "错误: 超出速率限制，请稍后再进行工具调用。".to_string(),
            });
            continue;
        }

        // Parse arguments
        let args: serde_json::Value = match serde_json::from_str(&tc.function.arguments) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    tool = tool_name,
                    error = %e,
                    "工具参数解析失败"
                );
                results.push(ChatMessage::Tool {
                    tool_call_id: tc.id.clone(),
                    content: format!("错误: 参数解析失败: {e}"),
                });
                continue;
            }
        };

        // Execute the tool
        if !quiet {
            tracing::info!(tool = tool_name, "正在执行工具");
        }
        let tool_result = match tool.execute(args).await {
            Ok(result) => {
                if result.success {
                    result.output
                } else {
                    format!("Error: {}", result.error.unwrap_or(result.output))
                }
            }
            Err(e) => {
                tracing::error!(tool = tool_name, error = %e, "工具执行失败");
                format!("Error: {e}")
            }
        };

        let duration = tool_start.elapsed();
        let success = !tool_result.starts_with("Error:");

        observer.record_event(&ObserverEvent::ToolCall {
            tool: tool_name.clone(),
            duration,
            success,
        });

        if !quiet {
            tracing::info!(
                tool = tool_name,
                success,
                duration_ms = duration.as_millis(),
                "工具执行完成"
            );
        }

        results.push(ChatMessage::Tool {
            tool_call_id: tc.id.clone(),
            content: tool_result,
        });
    }

    results
}

/// Trim conversation history to keep at most `max_turns` User turns.
///
/// System message (index 0) is always preserved. `max_turns == 0` means no limit.
pub fn trim_history(history: &mut Vec<ChatMessage>, max_turns: usize) {
    if max_turns == 0 {
        return;
    }

    // Count User messages
    let user_count = history
        .iter()
        .filter(|m| matches!(m, ChatMessage::User { .. }))
        .count();

    if user_count <= max_turns {
        return;
    }

    // Find the cut point: skip the first (user_count - max_turns) User messages,
    // then drain everything between index 1 and the start of the kept portion.
    let skip = user_count - max_turns;
    let mut user_seen = 0;
    let mut cut_index = 1; // start after System message
    for (i, msg) in history.iter().enumerate().skip(1) {
        if matches!(msg, ChatMessage::User { .. }) {
            user_seen += 1;
            if user_seen > skip {
                cut_index = i;
                break;
            }
        }
    }

    history.drain(1..cut_index);
}

/// Run the tool-calling loop: send messages → parse `tool_calls` → execute → feedback → repeat.
///
/// Operates on a shared `history` buffer. The caller is responsible for:
/// - Ensuring `history[0]` is a `System` message
/// - Appending a `User` message before calling this function
/// - Calling `trim_history()` before appending new User messages (if desired)
///
/// Returns the final text response from the model.
///
/// When `quiet` is true, suppresses all stdout/stderr output (for TUI mode).
#[allow(clippy::too_many_arguments)]
pub async fn run_tool_loop(
    provider: &dyn Provider,
    history: &mut Vec<ChatMessage>,
    tools: &[Box<dyn Tool>],
    tool_definitions: &[ToolDefinition],
    model: &str,
    temperature: f64,
    max_iterations: usize,
    security: &SecurityPolicy,
    observer: &dyn Observer,
    quiet: bool,
) -> Result<String> {
    for iteration in 0..max_iterations {
        let response = provider
            .chat_with_tools(history, tool_definitions, model, temperature)
            .await?;

        match response {
            ChatResponse::Text(text) => {
                // Append the assistant's final text to history so subsequent calls see it
                history.push(ChatMessage::Assistant {
                    content: Some(text.clone()),
                    tool_calls: None,
                });
                return Ok(text);
            }
            ChatResponse::ToolUse {
                tool_calls,
                text: assistant_text,
            } => {
                if !quiet {
                    tracing::info!(iteration, num_calls = tool_calls.len(), "模型请求工具调用");

                    if let Some(ref text) = assistant_text {
                        if !text.trim().is_empty() {
                            println!("{text}");
                        }
                    }
                }

                // Append assistant message with tool_calls
                history.push(ChatMessage::Assistant {
                    content: assistant_text,
                    tool_calls: Some(tool_calls.clone()),
                });

                // Execute all tool calls
                let tool_results =
                    execute_tool_calls(&tool_calls, tools, security, observer, quiet).await;

                // Print tool results for user visibility (skip in TUI mode)
                if !quiet {
                    for result in &tool_results {
                        if let ChatMessage::Tool {
                            content,
                            tool_call_id,
                        } = result
                        {
                            let tool_name = tool_calls
                                .iter()
                                .find(|tc| tc.id == *tool_call_id)
                                .map_or("unknown", |tc| tc.function.name.as_str());
                            let preview = truncate_with_ellipsis(content, 200);
                            println!("  [{tool_name}] {preview}");
                        }
                    }
                }

                // Append all tool results to history
                history.extend(tool_results);
            }
        }
    }

    // Max iterations reached — ask for a final text response without tools
    tracing::warn!(max_iterations, "工具循环已达最大迭代次数，正在请求最终响应");
    history.push(ChatMessage::User {
        content: "你已达到工具调用的最大迭代次数。请根据目前收集的信息，立即给出最终回答。"
            .to_string(),
    });

    let final_response = provider
        .chat_with_tools(history, &[], model, temperature)
        .await?;

    match final_response {
        ChatResponse::Text(text) => {
            history.push(ChatMessage::Assistant {
                content: Some(text.clone()),
                tool_calls: None,
            });
            Ok(text)
        }
        ChatResponse::ToolUse { text, .. } => {
            let final_text =
                text.unwrap_or_else(|| "在迭代次数限制内未能给出最终回答。".to_string());
            history.push(ChatMessage::Assistant {
                content: Some(final_text.clone()),
                tool_calls: None,
            });
            Ok(final_text)
        }
    }
}

#[allow(clippy::too_many_lines)]
pub async fn run(
    config: Config,
    message: Option<String>,
    provider_override: Option<String>,
    model_override: Option<String>,
    temperature: f64,
) -> Result<()> {
    // ── Wire up agnostic subsystems ──────────────────────────────
    let observer: Arc<dyn Observer> =
        Arc::from(observability::create_observer(&config.observability));
    let _runtime = runtime::create_runtime(&config.runtime)?;
    let security = Arc::new(SecurityPolicy::from_config(
        &config.autonomy,
        &config.workspace_dir,
    ));

    // ── Memory (the brain) ────────────────────────────────────────
    let mem: Arc<dyn Memory> = Arc::from(memory::create_memory(
        &config.memory,
        &config.workspace_dir,
        config.api_key.as_deref(),
    )?);
    tracing::info!(backend = mem.name(), "记忆系统已初始化");

    // ── Tools (including memory tools) ────────────────────────────
    let composio_key = if config.composio.enabled {
        config.composio.api_key.as_deref()
    } else {
        None
    };
    let tools = tools::all_tools(
        &security,
        mem.clone(),
        composio_key,
        &config.browser,
        &config.brave_search,
    );

    // Build tool definitions for the API
    let tool_definitions: Vec<ToolDefinition> = tools
        .iter()
        .map(|t| tool_spec_to_definition(&t.spec()))
        .collect();

    // ── Resolve provider ─────────────────────────────────────────
    let provider_name = provider_override
        .as_deref()
        .or(config.default_provider.as_deref())
        .unwrap_or("openrouter");

    let model_name = model_override
        .as_deref()
        .or(config.default_model.as_deref())
        .unwrap_or("anthropic/claude-sonnet-4-20250514");

    let provider: Box<dyn Provider> = providers::create_resilient_provider(
        provider_name,
        config.api_key.as_deref(),
        &config.reliability,
    )?;

    observer.record_event(&ObserverEvent::AgentStart {
        provider: provider_name.to_string(),
        model: model_name.to_string(),
    });

    // ── Build system prompt from workspace MD files (OpenClaw framework) ──
    let skills = crate::skills::load_skills(&config.workspace_dir);
    let mut tool_descs: Vec<(&str, &str)> = vec![
        (
            "shell",
            "Execute terminal commands. Use when: running local checks, build/test commands, diagnostics. Don't use when: a safer dedicated tool exists, or command is destructive without approval.",
        ),
        (
            "file_read",
            "Read file contents. Use when: inspecting project files, configs, logs. Don't use when: a targeted search is enough.",
        ),
        (
            "file_write",
            "Write file contents. Use when: applying focused edits, scaffolding files, updating docs/code. Don't use when: side effects are unclear or file ownership is uncertain.",
        ),
        (
            "memory_store",
            "Save to memory. Use when: preserving durable preferences, decisions, key context. Don't use when: information is transient/noisy/sensitive without need.",
        ),
        (
            "memory_recall",
            "Search memory. Use when: retrieving prior decisions, user preferences, historical context. Don't use when: answer is already in current context.",
        ),
        (
            "memory_forget",
            "Delete a memory entry. Use when: memory is incorrect/stale or explicitly requested for removal. Don't use when: impact is uncertain.",
        ),
    ];
    if config.browser.enabled {
        tool_descs.push((
            "browser_open",
            "Open approved HTTPS URLs in Brave Browser (allowlist-only, no scraping)",
        ));
    }
    if config.brave_search.enabled {
        tool_descs.push((
            "web_search",
            "Search the web using Brave Search. Use when: you need current information, facts, documentation, or any knowledge beyond your training data.",
        ));
    }
    let system_prompt = crate::channels::build_system_prompt(
        &config.workspace_dir,
        model_name,
        &tool_descs,
        &skills,
    );

    let max_iterations = config.autonomy.max_tool_iterations;
    let max_history_turns = config.autonomy.max_history_turns;

    // ── Execute ──────────────────────────────────────────────────
    let start = Instant::now();

    if let Some(msg) = message {
        // Auto-save user message to memory
        if config.memory.auto_save {
            let _ = mem
                .store("user_msg", &msg, MemoryCategory::Conversation)
                .await;
        }

        // Inject memory context into user message
        let context = build_context(mem.as_ref(), &msg).await;
        let enriched = if context.is_empty() {
            msg.clone()
        } else {
            format!("{context}{msg}")
        };

        // Single-message mode: fresh history for one-shot
        let mut history = vec![
            ChatMessage::System {
                content: system_prompt.clone(),
            },
            ChatMessage::User { content: enriched },
        ];

        let response = run_tool_loop(
            provider.as_ref(),
            &mut history,
            &tools,
            &tool_definitions,
            model_name,
            temperature,
            max_iterations,
            &security,
            observer.as_ref(),
            false,
        )
        .await?;
        println!("{response}");

        // Auto-save assistant response to daily log
        if config.memory.auto_save {
            let summary = truncate_with_ellipsis(&response, 100);
            let _ = mem
                .store("assistant_resp", &summary, MemoryCategory::Daily)
                .await;
        }
    } else {
        println!("🤖 Jarvis 交互模式");
        println!("输入 /quit 退出。\n");

        let (tx, mut rx) = tokio::sync::mpsc::channel(32);
        let cli = crate::channels::CliChannel::new();

        // Spawn listener
        let listen_handle = tokio::spawn(async move {
            let _ = crate::channels::Channel::listen(&cli, tx).await;
        });

        // Persistent conversation history across turns
        let mut history = vec![ChatMessage::System {
            content: system_prompt.clone(),
        }];

        while let Some(msg) = rx.recv().await {
            // Auto-save conversation turns
            if config.memory.auto_save {
                let _ = mem
                    .store("user_msg", &msg.content, MemoryCategory::Conversation)
                    .await;
            }

            // Inject memory context into user message
            let context = build_context(mem.as_ref(), &msg.content).await;
            let enriched = if context.is_empty() {
                msg.content.clone()
            } else {
                format!("{context}{}", msg.content)
            };

            // Trim history before adding new turn
            trim_history(&mut history, max_history_turns);
            history.push(ChatMessage::User { content: enriched });

            let response = run_tool_loop(
                provider.as_ref(),
                &mut history,
                &tools,
                &tool_definitions,
                model_name,
                temperature,
                max_iterations,
                &security,
                observer.as_ref(),
                false,
            )
            .await?;
            println!("\n{response}\n");

            if config.memory.auto_save {
                let summary = truncate_with_ellipsis(&response, 100);
                let _ = mem
                    .store("assistant_resp", &summary, MemoryCategory::Daily)
                    .await;
            }
        }

        listen_handle.abort();
    }

    let duration = start.elapsed();
    observer.record_event(&ObserverEvent::AgentEnd {
        duration,
        tokens_used: None,
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::traits::{ChatMessage, ChatResponse, FunctionCall, ToolCall};

    struct MockToolProvider {
        /// Responses to return in order: first call returns responses[0], etc.
        responses: Vec<ChatResponse>,
        call_count: std::sync::atomic::AtomicUsize,
    }

    #[async_trait::async_trait]
    impl Provider for MockToolProvider {
        async fn chat_with_system(
            &self,
            _system_prompt: Option<&str>,
            _message: &str,
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<String> {
            Ok("fallback".into())
        }

        async fn chat_with_tools(
            &self,
            _messages: &[ChatMessage],
            _tools: &[ToolDefinition],
            _model: &str,
            _temperature: f64,
        ) -> anyhow::Result<ChatResponse> {
            let idx = self
                .call_count
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if idx < self.responses.len() {
                Ok(self.responses[idx].clone())
            } else {
                Ok(ChatResponse::Text("done".into()))
            }
        }
    }

    fn make_echo_tool() -> Box<dyn Tool> {
        struct EchoTool;

        #[async_trait::async_trait]
        impl Tool for EchoTool {
            fn name(&self) -> &str {
                "echo"
            }
            fn description(&self) -> &str {
                "Echo back the input"
            }
            fn parameters_schema(&self) -> serde_json::Value {
                serde_json::json!({
                    "type": "object",
                    "properties": {
                        "text": { "type": "string" }
                    },
                    "required": ["text"]
                })
            }
            async fn execute(
                &self,
                args: serde_json::Value,
            ) -> anyhow::Result<crate::tools::ToolResult> {
                let text = args["text"].as_str().unwrap_or("(no text)");
                Ok(crate::tools::ToolResult {
                    success: true,
                    output: text.to_string(),
                    error: None,
                })
            }
        }

        Box::new(EchoTool)
    }

    // Helper to build a fresh history with system + user messages.
    fn make_history(system: &str, user: &str) -> Vec<ChatMessage> {
        vec![
            ChatMessage::System {
                content: system.into(),
            },
            ChatMessage::User {
                content: user.into(),
            },
        ]
    }

    #[tokio::test]
    async fn tool_loop_text_response_returns_immediately() {
        let provider = MockToolProvider {
            responses: vec![ChatResponse::Text("Hello!".into())],
            call_count: std::sync::atomic::AtomicUsize::new(0),
        };
        let security = SecurityPolicy::default();
        let observer = crate::observability::NoopObserver;

        let mut history = make_history("system", "hello");
        let result = run_tool_loop(
            &provider,
            &mut history,
            &[],
            &[],
            "model",
            0.7,
            10,
            &security,
            &observer,
            true,
        )
        .await
        .unwrap();

        assert_eq!(result, "Hello!");
        // History should now contain: System, User, Assistant
        assert_eq!(history.len(), 3);
        assert!(
            matches!(&history[2], ChatMessage::Assistant { content: Some(t), .. } if t == "Hello!")
        );
    }

    #[tokio::test]
    async fn tool_loop_executes_tool_and_returns_text() {
        let tool = make_echo_tool();
        let tool_defs = vec![tool_spec_to_definition(&tool.spec())];

        let provider = MockToolProvider {
            responses: vec![
                // First call: model wants to call echo tool
                ChatResponse::ToolUse {
                    tool_calls: vec![ToolCall {
                        id: "call_1".into(),
                        function: FunctionCall {
                            name: "echo".into(),
                            arguments: r#"{"text":"hello world"}"#.into(),
                        },
                    }],
                    text: None,
                },
                // Second call: model returns final text
                ChatResponse::Text("The echo returned: hello world".into()),
            ],
            call_count: std::sync::atomic::AtomicUsize::new(0),
        };

        let security = SecurityPolicy {
            max_actions_per_hour: 100,
            ..SecurityPolicy::default()
        };
        let observer = crate::observability::NoopObserver;

        let mut history = make_history("system", "echo something");
        let result = run_tool_loop(
            &provider,
            &mut history,
            &[tool],
            &tool_defs,
            "model",
            0.7,
            10,
            &security,
            &observer,
            true,
        )
        .await
        .unwrap();

        assert_eq!(result, "The echo returned: hello world");
    }

    #[tokio::test]
    async fn tool_loop_handles_unknown_tool() {
        let provider = MockToolProvider {
            responses: vec![
                ChatResponse::ToolUse {
                    tool_calls: vec![ToolCall {
                        id: "call_1".into(),
                        function: FunctionCall {
                            name: "nonexistent_tool".into(),
                            arguments: "{}".into(),
                        },
                    }],
                    text: None,
                },
                ChatResponse::Text("Sorry, that tool doesn't exist.".into()),
            ],
            call_count: std::sync::atomic::AtomicUsize::new(0),
        };

        let security = SecurityPolicy {
            max_actions_per_hour: 100,
            ..SecurityPolicy::default()
        };
        let observer = crate::observability::NoopObserver;

        let mut history = make_history("system", "use nonexistent");
        let result = run_tool_loop(
            &provider,
            &mut history,
            &[],
            &[],
            "model",
            0.7,
            10,
            &security,
            &observer,
            true,
        )
        .await
        .unwrap();

        assert_eq!(result, "Sorry, that tool doesn't exist.");
    }

    #[tokio::test]
    async fn tool_loop_respects_max_iterations() {
        // Model returns tool calls for 3 iterations, then text on the final forced call
        let mut responses: Vec<ChatResponse> = Vec::new();
        for i in 0..3 {
            responses.push(ChatResponse::ToolUse {
                tool_calls: vec![ToolCall {
                    id: format!("call_{i}"),
                    function: FunctionCall {
                        name: "echo".into(),
                        arguments: r#"{"text":"loop"}"#.into(),
                    },
                }],
                text: None,
            });
        }
        // After 3 iterations, the loop hits max and forces a no-tools call — index 3
        responses.push(ChatResponse::Text("Stopped after max iterations.".into()));

        let tool = make_echo_tool();
        let tool_defs = vec![tool_spec_to_definition(&tool.spec())];

        let provider = MockToolProvider {
            responses,
            call_count: std::sync::atomic::AtomicUsize::new(0),
        };

        let security = SecurityPolicy {
            max_actions_per_hour: 100,
            ..SecurityPolicy::default()
        };
        let observer = crate::observability::NoopObserver;

        let mut history = make_history("system", "keep looping");
        let result = run_tool_loop(
            &provider,
            &mut history,
            &[tool],
            &tool_defs,
            "model",
            0.7,
            3, // only 3 iterations
            &security,
            &observer,
            true,
        )
        .await
        .unwrap();

        assert_eq!(result, "Stopped after max iterations.");
        // Should have been called: 3 tool iterations + 1 final = 4 times
        assert_eq!(
            provider
                .call_count
                .load(std::sync::atomic::Ordering::SeqCst),
            4
        );
    }

    // ── trim_history tests ──────────────────────────────────────

    #[test]
    fn trim_history_keeps_system_message() {
        let mut history = vec![
            ChatMessage::System {
                content: "sys".into(),
            },
            ChatMessage::User {
                content: "msg1".into(),
            },
            ChatMessage::Assistant {
                content: Some("resp1".into()),
                tool_calls: None,
            },
            ChatMessage::User {
                content: "msg2".into(),
            },
            ChatMessage::Assistant {
                content: Some("resp2".into()),
                tool_calls: None,
            },
            ChatMessage::User {
                content: "msg3".into(),
            },
            ChatMessage::Assistant {
                content: Some("resp3".into()),
                tool_calls: None,
            },
        ];

        trim_history(&mut history, 1);

        // Should keep: System + last User turn (msg3 + resp3)
        assert!(matches!(&history[0], ChatMessage::System { content } if content == "sys"));
        assert!(matches!(&history[1], ChatMessage::User { content } if content == "msg3"));
        assert_eq!(history.len(), 3);
    }

    #[test]
    fn trim_history_removes_oldest_turns() {
        let mut history = vec![
            ChatMessage::System {
                content: "sys".into(),
            },
            ChatMessage::User {
                content: "msg1".into(),
            },
            ChatMessage::Assistant {
                content: Some("resp1".into()),
                tool_calls: None,
            },
            ChatMessage::User {
                content: "msg2".into(),
            },
            ChatMessage::Assistant {
                content: Some("resp2".into()),
                tool_calls: None,
            },
            ChatMessage::User {
                content: "msg3".into(),
            },
            ChatMessage::Assistant {
                content: Some("resp3".into()),
                tool_calls: None,
            },
        ];

        trim_history(&mut history, 2);

        // Should keep: System + last 2 User turns (msg2+resp2, msg3+resp3)
        assert!(matches!(&history[0], ChatMessage::System { .. }));
        assert!(matches!(&history[1], ChatMessage::User { content } if content == "msg2"));
        assert!(
            matches!(&history[2], ChatMessage::Assistant { content: Some(t), .. } if t == "resp2")
        );
        assert!(matches!(&history[3], ChatMessage::User { content } if content == "msg3"));
        assert_eq!(history.len(), 5);
    }

    #[test]
    fn trim_history_zero_means_unlimited() {
        let mut history = vec![
            ChatMessage::System {
                content: "sys".into(),
            },
            ChatMessage::User {
                content: "msg1".into(),
            },
            ChatMessage::User {
                content: "msg2".into(),
            },
            ChatMessage::User {
                content: "msg3".into(),
            },
        ];
        let original_len = history.len();

        trim_history(&mut history, 0);

        assert_eq!(history.len(), original_len);
    }

    #[test]
    fn trim_history_no_op_when_within_limit() {
        let mut history = vec![
            ChatMessage::System {
                content: "sys".into(),
            },
            ChatMessage::User {
                content: "msg1".into(),
            },
            ChatMessage::Assistant {
                content: Some("resp1".into()),
                tool_calls: None,
            },
        ];
        let original_len = history.len();

        trim_history(&mut history, 5);

        assert_eq!(history.len(), original_len);
    }

    #[tokio::test]
    async fn conversation_history_accumulates_across_calls() {
        let provider = MockToolProvider {
            responses: vec![
                ChatResponse::Text("I'm assistant turn 1".into()),
                ChatResponse::Text("I'm assistant turn 2".into()),
            ],
            call_count: std::sync::atomic::AtomicUsize::new(0),
        };
        let security = SecurityPolicy::default();
        let observer = crate::observability::NoopObserver;

        // Start with system message only
        let mut history = vec![ChatMessage::System {
            content: "system".into(),
        }];

        // First turn
        history.push(ChatMessage::User {
            content: "hello".into(),
        });
        let r1 = run_tool_loop(
            &provider,
            &mut history,
            &[],
            &[],
            "model",
            0.7,
            10,
            &security,
            &observer,
            true,
        )
        .await
        .unwrap();
        assert_eq!(r1, "I'm assistant turn 1");
        // History: System, User("hello"), Assistant("turn 1")
        assert_eq!(history.len(), 3);

        // Second turn — history carries over
        history.push(ChatMessage::User {
            content: "what did I say?".into(),
        });
        let r2 = run_tool_loop(
            &provider,
            &mut history,
            &[],
            &[],
            "model",
            0.7,
            10,
            &security,
            &observer,
            true,
        )
        .await
        .unwrap();
        assert_eq!(r2, "I'm assistant turn 2");
        // History: System, User, Assistant, User, Assistant = 5
        assert_eq!(history.len(), 5);
        assert!(
            matches!(&history[3], ChatMessage::User { content } if content == "what did I say?")
        );
    }

    #[tokio::test]
    async fn execute_tool_calls_rate_limit() {
        let tool = make_echo_tool();
        let security = SecurityPolicy {
            max_actions_per_hour: 1,
            ..SecurityPolicy::default()
        };
        let observer = crate::observability::NoopObserver;

        let calls = vec![
            crate::providers::ToolCall {
                id: "call_1".into(),
                function: FunctionCall {
                    name: "echo".into(),
                    arguments: r#"{"text":"first"}"#.into(),
                },
            },
            crate::providers::ToolCall {
                id: "call_2".into(),
                function: FunctionCall {
                    name: "echo".into(),
                    arguments: r#"{"text":"second"}"#.into(),
                },
            },
        ];

        let results = execute_tool_calls(&calls, &[tool], &security, &observer, true).await;

        assert_eq!(results.len(), 2);
        // First should succeed
        if let ChatMessage::Tool { content, .. } = &results[0] {
            assert_eq!(content, "first");
        } else {
            panic!("Expected Tool message");
        }
        // Second should be rate limited
        if let ChatMessage::Tool { content, .. } = &results[1] {
            assert!(
                content.contains("速率限制"),
                "Expected rate limit error, got: {content}"
            );
        } else {
            panic!("Expected Tool message");
        }
    }

    #[tokio::test]
    async fn execute_tool_calls_bad_arguments() {
        let tool = make_echo_tool();
        let security = SecurityPolicy {
            max_actions_per_hour: 100,
            ..SecurityPolicy::default()
        };
        let observer = crate::observability::NoopObserver;

        let calls = vec![crate::providers::ToolCall {
            id: "call_1".into(),
            function: FunctionCall {
                name: "echo".into(),
                arguments: "not valid json".into(),
            },
        }];

        let results = execute_tool_calls(&calls, &[tool], &security, &observer, true).await;

        assert_eq!(results.len(), 1);
        if let ChatMessage::Tool { content, .. } = &results[0] {
            assert!(
                content.contains("参数解析失败"),
                "Expected parse error, got: {content}"
            );
        } else {
            panic!("Expected Tool message");
        }
    }
}
