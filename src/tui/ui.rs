use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::app::{App, AppStatus, MessageRole};

/// Render the entire TUI.
pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();

    // Four-part vertical layout: title(1) + chat(fill) + status(1) + input(3)
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(4),
        Constraint::Length(1),
        Constraint::Length(3),
    ])
    .split(area);

    draw_title_bar(f, chunks[0], app);
    draw_chat_area(f, chunks[1], app);
    draw_status_bar(f, chunks[2], app);
    draw_input_area(f, chunks[3], app);
}

/// Title bar: `Jarvis` TUI on the left, model info on the right.
fn draw_title_bar(f: &mut Frame, area: Rect, app: &App) {
    let model_info = format!("{}/{}", app.provider_display, app.model_display);
    let title_text = "Jarvis TUI";

    // Right-pad the title to push model info to the right
    let combined_len = title_text.len() + model_info.len();
    #[allow(clippy::cast_possible_truncation)]
    let padding = area.width.saturating_sub(combined_len as u16);

    let line = Line::from(vec![
        Span::styled(
            title_text,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" ".repeat(padding as usize)),
        Span::styled(model_info, Style::default().fg(Color::DarkGray)),
    ]);

    let para = Paragraph::new(line).style(Style::default().bg(Color::DarkGray).fg(Color::White));
    f.render_widget(para, area);
}

/// Chat area: scrollable list of messages.
///
/// All text is pre-wrapped into lines that each fit exactly one visual row.
/// This avoids relying on `Paragraph::Wrap` (whose word-wrapping produces a
/// different line count from any external estimate, especially with CJK text),
/// so skip/take scrolling is pixel-perfect.
#[allow(clippy::too_many_lines)]
fn draw_chat_area(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::LEFT | Borders::RIGHT)
        .border_style(Style::default().fg(Color::DarkGray));

    let inner = block.inner(area);
    let inner_height = inner.height as usize;
    let inner_width = inner.width as usize;

    let mut lines: Vec<Line<'_>> = Vec::new();

    for msg in &app.messages {
        // Blank line before each message
        lines.push(Line::from(""));

        let (label, label_style) = match msg.role {
            MessageRole::User => (
                "You: ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            MessageRole::Assistant => (
                "Jarvis: ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            MessageRole::System => (
                "System: ",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        };

        let content_style = match msg.role {
            MessageRole::System => Style::default().fg(Color::Yellow),
            MessageRole::User | MessageRole::Assistant => Style::default(),
        };

        let label_display_width = UnicodeWidthStr::width(label);
        let prefix_width = 2 + label_display_width; // "  " + label
        let indent = " ".repeat(prefix_width);
        let content_lines: Vec<&str> = msg.content.lines().collect();

        // First content line: label takes up prefix_width columns
        if let Some(first) = content_lines.first() {
            let first_avail = inner_width.saturating_sub(prefix_width);
            let wrapped = wrap_text(first, first_avail);

            if let Some((first_seg, rest)) = wrapped.split_first() {
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(label, label_style),
                    Span::styled(first_seg.clone(), content_style),
                ]));
                for seg in rest {
                    lines.push(Line::from(vec![
                        Span::raw(indent.clone()),
                        Span::styled(seg.clone(), content_style),
                    ]));
                }
            }
        }

        // Remaining content lines: all indented
        let rest_avail = inner_width.saturating_sub(prefix_width);
        for content_line in content_lines.iter().skip(1) {
            let wrapped = wrap_text(content_line, rest_avail);
            for seg in &wrapped {
                lines.push(Line::from(vec![
                    Span::raw(indent.clone()),
                    Span::styled(seg.clone(), content_style),
                ]));
            }
        }
    }

    // Spinner when waiting
    if app.status == AppStatus::Waiting {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!("Thinking... {}", app.spinner_char()),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }

    // Each Line now fits in exactly one visual row — skip/take is exact
    let total = lines.len();
    let skip = if total > inner_height {
        let max_scroll = total - inner_height;
        let user_offset = (app.scroll_offset as usize).min(max_scroll);
        max_scroll - user_offset
    } else {
        0
    };

    let visible: Vec<Line<'_>> = lines.into_iter().skip(skip).take(inner_height).collect();

    let para = Paragraph::new(visible).block(block);
    f.render_widget(para, area);
}

/// Status bar: memory backend + current status.
fn draw_status_bar(f: &mut Frame, area: Rect, app: &App) {
    let status_text = match app.status {
        AppStatus::Idle => "Idle",
        AppStatus::Waiting => "Waiting...",
    };

    let line = Line::from(vec![
        Span::styled(
            format!(" Memory: {} (auto)", app.memory_display),
            Style::default().fg(Color::White),
        ),
        Span::styled(" | ", Style::default().fg(Color::DarkGray)),
        Span::styled(status_text, Style::default().fg(Color::White)),
    ]);

    let para = Paragraph::new(line).style(Style::default().bg(Color::DarkGray).fg(Color::White));
    f.render_widget(para, area);
}

/// Input area: bordered text input with cursor.
fn draw_input_area(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Input ");

    // Calculate cursor display width (Chinese/fullwidth chars = 2 columns each)
    let display_cursor = unicode_width::UnicodeWidthStr::width(&app.input[..app.cursor_pos]);

    let para = Paragraph::new(app.input.as_str())
        .block(block)
        .wrap(Wrap { trim: false });

    f.render_widget(para, area);

    // Place the cursor inside the input block (accounting for border)
    #[allow(clippy::cast_possible_truncation)]
    let cursor_x = area.x + 1 + display_cursor as u16;
    let cursor_y = area.y + 1;
    if cursor_x < area.x + area.width - 1 {
        f.set_cursor_position((cursor_x, cursor_y));
    }
}

/// Wrap text into segments that each fit within `max_width` display columns.
///
/// Handles CJK characters (2 columns each) correctly.
/// Returns at least one entry (empty string for empty input).
fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![text.to_string()];
    }

    let mut result = Vec::new();
    let mut current = String::new();
    let mut current_width: usize = 0;

    for ch in text.chars() {
        let ch_width = UnicodeWidthChar::width(ch).unwrap_or(0);

        // If adding this char would overflow, start a new line
        if current_width + ch_width > max_width && !current.is_empty() {
            result.push(std::mem::take(&mut current));
            current_width = 0;
        }

        current.push(ch);
        current_width += ch_width;
    }

    // Always push the last segment (even if empty — represents an empty content line)
    result.push(current);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_text_ascii() {
        // 10 chars in width 5 → 2 segments
        let result = wrap_text("abcdefghij", 5);
        assert_eq!(result, vec!["abcde", "fghij"]);
    }

    #[test]
    fn wrap_text_cjk() {
        // Each Chinese char is 2 columns; "你好世界" = 8 columns
        let result = wrap_text("你好世界", 4);
        // 你好 = 4 cols, 世界 = 4 cols
        assert_eq!(result, vec!["你好", "世界"]);
    }

    #[test]
    fn wrap_text_cjk_boundary() {
        // Width 5: 你(2)+好(2)=4 fits, 世(2) would be 6 → overflow
        let result = wrap_text("你好世界", 5);
        assert_eq!(result, vec!["你好", "世界"]);
    }

    #[test]
    fn wrap_text_mixed() {
        // "Hi你好" = H(1)+i(1)+你(2)+好(2) = 6 cols
        let result = wrap_text("Hi你好", 4);
        // "Hi你" = 1+1+2 = 4, "好" = 2
        assert_eq!(result, vec!["Hi你", "好"]);
    }

    #[test]
    fn wrap_text_empty() {
        let result = wrap_text("", 10);
        assert_eq!(result, vec![""]);
    }

    #[test]
    fn wrap_text_no_wrap_needed() {
        let result = wrap_text("short", 80);
        assert_eq!(result, vec!["short"]);
    }

    #[test]
    fn test_draw_does_not_panic() {
        let app = App::new("openrouter", "test-model", "sqlite");
        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn test_draw_with_messages() {
        let mut app = App::new("openrouter", "test-model", "sqlite");
        app.push_message(MessageRole::User, "Hello");
        app.push_message(MessageRole::Assistant, "Hi there!");
        app.push_message(MessageRole::System, "System info");

        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn test_draw_waiting_spinner() {
        let mut app = App::new("openrouter", "test-model", "sqlite");
        app.status = AppStatus::Waiting;
        app.push_message(MessageRole::User, "question");

        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn test_draw_small_terminal() {
        let app = App::new("p", "m", "none");
        let backend = ratatui::backend::TestBackend::new(20, 10);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();
    }

    #[test]
    fn test_draw_with_input() {
        let mut app = App::new("openrouter", "test-model", "sqlite");
        app.input = "Hello world".to_string();
        app.cursor_pos = 5;

        let backend = ratatui::backend::TestBackend::new(80, 24);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();
    }
}
