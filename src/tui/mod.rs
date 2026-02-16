pub mod app;
pub mod event;
pub mod ui;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::stdout;
use std::sync::Arc;
use tokio::sync::mpsc;

use crate::agent::loop_::{run_tool_loop, trim_history};
use crate::config::Config;
use crate::memory::{self, Memory, MemoryCategory};
use crate::observability::{self, Observer, ObserverEvent};
use crate::providers::traits::{tool_spec_to_definition, ChatMessage, ToolDefinition};
use crate::providers::{self, Provider};
use crate::runtime;
use crate::security::SecurityPolicy;
use crate::tools::{self, Tool};
use crate::util::truncate_with_ellipsis;

use app::{App, AppStatus, MessageRole, SlashResult};
use event::{spawn_event_reader, AppEvent};

const HELP_TEXT: &str = "\
Commands:
  /quit, /exit, /q  — Exit TUI
  /clear, /cls      — Clear chat history
  /help, /h, /?     — Show this help

Keys:
  Enter       — Send message
  Ctrl+C, Esc — Quit
  Backspace   — Delete character
  Left/Right  — Move cursor
  Up/Down     — Scroll chat
  PageUp/Down — Scroll chat (page)
  Ctrl+L      — Clear screen";

/// Run the TUI agent loop.
#[allow(clippy::too_many_lines)]
pub async fn run(
    config: Config,
    provider_override: Option<String>,
    model_override: Option<String>,
    temperature: f64,
) -> Result<()> {
    // ── Wire up subsystems (same as agent::run) ──────────────
    let observer: Arc<dyn Observer> =
        Arc::from(observability::create_observer(&config.observability));
    let _runtime = runtime::create_runtime(&config.runtime)?;
    let security = Arc::new(SecurityPolicy::from_config(
        &config.autonomy,
        &config.workspace_dir,
    ));

    let mem: Arc<dyn Memory> = Arc::from(memory::create_memory(
        &config.memory,
        &config.workspace_dir,
        config.api_key.as_deref(),
    )?);

    let composio_key = if config.composio.enabled {
        config.composio.api_key.as_deref()
    } else {
        None
    };
    let tools: Arc<Vec<Box<dyn Tool>>> = Arc::new(tools::all_tools(
        &security,
        mem.clone(),
        composio_key,
        &config.browser,
        &config.brave_search,
    ));

    // Build tool definitions for function calling API
    let tool_definitions: Arc<Vec<ToolDefinition>> = Arc::new(
        tools
            .iter()
            .map(|t| tool_spec_to_definition(&t.spec()))
            .collect(),
    );

    let provider_name = provider_override
        .as_deref()
        .or(config.default_provider.as_deref())
        .unwrap_or("openrouter");

    let model_name = model_override
        .as_deref()
        .or(config.default_model.as_deref())
        .unwrap_or("anthropic/claude-sonnet-4-20250514");

    // Use Arc so we can clone into spawned tasks
    let provider: Arc<dyn Provider> = Arc::from(providers::create_resilient_provider(
        provider_name,
        config.api_key.as_deref(),
        &config.reliability,
    )?);

    observer.record_event(&ObserverEvent::AgentStart {
        provider: provider_name.to_string(),
        model: model_name.to_string(),
    });

    let skills = crate::skills::load_skills(&config.workspace_dir);
    let mut tool_descs: Vec<(&str, &str)> = vec![
        ("shell", "Execute terminal commands."),
        ("file_read", "Read file contents."),
        ("file_write", "Write file contents."),
        ("memory_store", "Save to memory."),
        ("memory_recall", "Search memory."),
        ("memory_forget", "Delete a memory entry."),
    ];
    if config.brave_search.enabled {
        tool_descs.push(("web_search", "Search the web using Brave Search."));
    }
    let system_prompt = Arc::new(crate::channels::build_system_prompt(
        &config.workspace_dir,
        model_name,
        &tool_descs,
        &skills,
    ));
    let model_owned = Arc::new(model_name.to_string());
    let max_history_turns = config.autonomy.max_history_turns;

    // ── Shared conversation history ─────────────────────────
    let history: Arc<tokio::sync::Mutex<Vec<ChatMessage>>> =
        Arc::new(tokio::sync::Mutex::new(vec![ChatMessage::System {
            content: (*system_prompt).clone(),
        }]));

    // ── Initialize terminal ──────────────────────────────────
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;

    // Panic hook: restore terminal on panic
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = stdout().execute(LeaveAlternateScreen);
        original_hook(info);
    }));

    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let memory_backend = config.memory.backend.clone();
    let mut app = App::new(provider_name, model_name, &memory_backend);

    app.push_message(
        MessageRole::System,
        "Welcome to Jarvis TUI! Type /help for commands.",
    );

    // ── Event channels ───────────────────────────────────────
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<AppEvent>();
    let (agent_tx, mut agent_rx) = mpsc::unbounded_channel::<AppEvent>();

    spawn_event_reader(event_tx);

    // ── Main loop ────────────────────────────────────────────
    let start = std::time::Instant::now();

    loop {
        terminal.draw(|f| ui::draw(f, &app))?;

        tokio::select! {
            Some(ev) = event_rx.recv() => {
                match ev {
                    AppEvent::Key(key) => {
                        if handle_key_event(
                            &mut app, key, &mem, &provider, &model_owned,
                            temperature, &system_prompt, &config, &agent_tx,
                            &tools, &tool_definitions, &security, &observer,
                            &history, max_history_turns,
                        ).await {
                            break;
                        }
                    }
                    AppEvent::Tick => {
                        if app.status == AppStatus::Waiting {
                            app.tick_spinner();
                        }
                    }
                    _ => {}
                }
            }
            Some(agent_ev) = agent_rx.recv() => {
                match agent_ev {
                    AppEvent::AgentResponse(response) => {
                        app.status = AppStatus::Idle;
                        app.push_message(MessageRole::Assistant, &response);

                        if config.memory.auto_save {
                            let summary = truncate_with_ellipsis(&response, 100);
                            let _ = mem.store("assistant_resp", &summary, MemoryCategory::Daily).await;
                        }
                    }
                    AppEvent::AgentError(err) => {
                        app.status = AppStatus::Idle;
                        app.push_message(MessageRole::System, &format!("Error: {err}"));
                    }
                    _ => {}
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    // ── Restore terminal ─────────────────────────────────────
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    observer.record_event(&ObserverEvent::AgentEnd {
        duration: start.elapsed(),
        tokens_used: None,
    });

    Ok(())
}

/// Handle a key event. Returns `true` if the app should quit.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn handle_key_event(
    app: &mut App,
    key: crossterm::event::KeyEvent,
    mem: &Arc<dyn Memory>,
    provider: &Arc<dyn Provider>,
    model_name: &Arc<String>,
    temperature: f64,
    system_prompt: &Arc<String>,
    config: &Config,
    agent_tx: &mpsc::UnboundedSender<AppEvent>,
    tools: &Arc<Vec<Box<dyn Tool>>>,
    tool_definitions: &Arc<Vec<ToolDefinition>>,
    security: &Arc<SecurityPolicy>,
    observer: &Arc<dyn Observer>,
    history: &Arc<tokio::sync::Mutex<Vec<ChatMessage>>>,
    max_history_turns: usize,
) -> bool {
    match (key.modifiers, key.code) {
        // Quit
        (KeyModifiers::CONTROL, KeyCode::Char('c')) | (_, KeyCode::Esc) => {
            app.should_quit = true;
            return true;
        }

        // Clear screen
        (KeyModifiers::CONTROL, KeyCode::Char('l')) => {
            app.messages.clear();
            app.scroll_offset = 0;
            let mut hist = history.lock().await;
            hist.clear();
            hist.push(ChatMessage::System {
                content: (**system_prompt).clone(),
            });
        }

        // Submit
        (_, KeyCode::Enter) => {
            if app.status == AppStatus::Waiting {
                return false;
            }

            let text = app.submit_input();
            if text.is_empty() {
                return false;
            }

            // Check for slash commands
            match App::handle_slash_command(&text) {
                SlashResult::Quit => {
                    app.should_quit = true;
                    return true;
                }
                SlashResult::Clear => {
                    app.messages.clear();
                    app.scroll_offset = 0;
                    let mut hist = history.lock().await;
                    hist.clear();
                    hist.push(ChatMessage::System {
                        content: (**system_prompt).clone(),
                    });
                    return false;
                }
                SlashResult::Help => {
                    app.push_message(MessageRole::System, HELP_TEXT);
                    return false;
                }
                SlashResult::None => {}
            }

            // Regular message
            app.push_message(MessageRole::User, &text);
            app.status = AppStatus::Waiting;

            // Auto-save
            if config.memory.auto_save {
                let _ = mem
                    .store("user_msg", &text, MemoryCategory::Conversation)
                    .await;
            }

            // Build context
            let context = build_context(mem.as_ref(), &text).await;
            let enriched = if context.is_empty() {
                text
            } else {
                format!("{context}{text}")
            };

            // Clone Arc references for the spawned task
            let prov = Arc::clone(provider);
            let model = Arc::clone(model_name);
            let tx = agent_tx.clone();
            let tools_clone = Arc::clone(tools);
            let tool_defs_clone = Arc::clone(tool_definitions);
            let sec = Arc::clone(security);
            let obs = Arc::clone(observer);
            let max_iter = config.autonomy.max_tool_iterations;
            let history_clone = Arc::clone(history);

            tokio::spawn(async move {
                let mut hist = history_clone.lock().await;
                trim_history(&mut hist, max_history_turns);
                hist.push(ChatMessage::User { content: enriched });
                let result = run_tool_loop(
                    prov.as_ref(),
                    &mut hist,
                    &tools_clone,
                    &tool_defs_clone,
                    &model,
                    temperature,
                    max_iter,
                    &sec,
                    obs.as_ref(),
                    true, // quiet: suppress stdout/stderr in TUI mode
                )
                .await;
                drop(hist); // explicitly release lock before sending
                match result {
                    Ok(response) => {
                        let _ = tx.send(AppEvent::AgentResponse(response));
                    }
                    Err(e) => {
                        let _ = tx.send(AppEvent::AgentError(e.to_string()));
                    }
                }
            });
        }

        // Text editing
        (_, KeyCode::Backspace) => app.delete_char_before(),
        (_, KeyCode::Delete) => app.delete_char_after(),
        (_, KeyCode::Left) => app.move_cursor_left(),
        (_, KeyCode::Right) => app.move_cursor_right(),
        (_, KeyCode::Home) => app.move_cursor_home(),
        (_, KeyCode::End) => app.move_cursor_end(),

        // Scrolling
        (_, KeyCode::Up) => app.scroll_up(1),
        (_, KeyCode::Down) => app.scroll_down(1),
        (_, KeyCode::PageUp) => app.scroll_up(10),
        (_, KeyCode::PageDown) => app.scroll_down(10),

        // Character input
        (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
            app.insert_char(c);
        }

        _ => {}
    }
    false
}

/// Build context preamble by searching memory for relevant entries.
async fn build_context(mem: &dyn Memory, user_msg: &str) -> String {
    use std::fmt::Write;
    let mut context = String::new();
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
