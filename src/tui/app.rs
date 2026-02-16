use chrono::Local;

/// A single chat message.
#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub role: MessageRole,
    pub content: String,
    pub timestamp: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

/// App status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppStatus {
    Idle,
    Waiting,
}

/// Slash-command result.
pub enum SlashResult {
    Quit,
    Clear,
    Help,
    None,
}

/// Core TUI application state.
pub struct App {
    pub messages: Vec<ChatMessage>,
    pub input: String,
    pub cursor_pos: usize,
    pub scroll_offset: u16,
    pub status: AppStatus,
    pub should_quit: bool,
    pub provider_display: String,
    pub model_display: String,
    pub memory_display: String,
    pub spinner_tick: usize,
}

const SPINNER_FRAMES: &[char] = &['|', '/', '-', '\\'];

impl App {
    pub fn new(provider: &str, model: &str, memory: &str) -> Self {
        Self {
            messages: Vec::new(),
            input: String::new(),
            cursor_pos: 0,
            scroll_offset: 0,
            status: AppStatus::Idle,
            should_quit: false,
            provider_display: provider.to_string(),
            model_display: model.to_string(),
            memory_display: memory.to_string(),
            spinner_tick: 0,
        }
    }

    pub fn push_message(&mut self, role: MessageRole, content: &str) {
        self.messages.push(ChatMessage {
            role,
            content: content.to_string(),
            timestamp: Local::now().format("%H:%M:%S").to_string(),
        });
        // Auto-scroll to bottom
        self.scroll_offset = 0;
    }

    pub fn insert_char(&mut self, c: char) {
        self.input.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
    }

    pub fn delete_char_before(&mut self) {
        if self.cursor_pos > 0 {
            let prev = self.input[..self.cursor_pos]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i);
            self.input.drain(prev..self.cursor_pos);
            self.cursor_pos = prev;
        }
    }

    pub fn delete_char_after(&mut self) {
        if self.cursor_pos < self.input.len() {
            let next = self.input[self.cursor_pos..]
                .char_indices()
                .nth(1)
                .map_or(self.input.len(), |(i, _)| self.cursor_pos + i);
            self.input.drain(self.cursor_pos..next);
        }
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos = self.input[..self.cursor_pos]
                .char_indices()
                .next_back()
                .map_or(0, |(i, _)| i);
        }
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor_pos < self.input.len() {
            self.cursor_pos = self.input[self.cursor_pos..]
                .char_indices()
                .nth(1)
                .map_or(self.input.len(), |(i, _)| self.cursor_pos + i);
        }
    }

    pub fn move_cursor_home(&mut self) {
        self.cursor_pos = 0;
    }

    pub fn move_cursor_end(&mut self) {
        self.cursor_pos = self.input.len();
    }

    /// Submit the current input. Returns the submitted text (or empty if blank).
    pub fn submit_input(&mut self) -> String {
        let text = self.input.trim().to_string();
        self.input.clear();
        self.cursor_pos = 0;
        text
    }

    /// Handle slash commands. Returns the action to take.
    pub fn handle_slash_command(input: &str) -> SlashResult {
        match input {
            "/quit" | "/exit" | "/q" => SlashResult::Quit,
            "/clear" | "/cls" => SlashResult::Clear,
            "/help" | "/h" | "/?" => SlashResult::Help,
            _ => SlashResult::None,
        }
    }

    pub fn scroll_up(&mut self, amount: u16) {
        self.scroll_offset = self.scroll_offset.saturating_add(amount);
    }

    pub fn scroll_down(&mut self, amount: u16) {
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    pub fn spinner_char(&self) -> char {
        SPINNER_FRAMES[self.spinner_tick % SPINNER_FRAMES.len()]
    }

    pub fn tick_spinner(&mut self) {
        self.spinner_tick = self.spinner_tick.wrapping_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_app() {
        let app = App::new("openrouter", "gpt-4", "sqlite");
        assert!(app.messages.is_empty());
        assert!(app.input.is_empty());
        assert_eq!(app.cursor_pos, 0);
        assert_eq!(app.status, AppStatus::Idle);
        assert!(!app.should_quit);
    }

    #[test]
    fn test_push_message() {
        let mut app = App::new("test", "test", "none");
        app.push_message(MessageRole::User, "hello");
        assert_eq!(app.messages.len(), 1);
        assert_eq!(app.messages[0].role, MessageRole::User);
        assert_eq!(app.messages[0].content, "hello");
    }

    #[test]
    fn test_insert_and_delete_char() {
        let mut app = App::new("test", "test", "none");
        app.insert_char('a');
        app.insert_char('b');
        app.insert_char('c');
        assert_eq!(app.input, "abc");
        assert_eq!(app.cursor_pos, 3);

        app.delete_char_before();
        assert_eq!(app.input, "ab");
        assert_eq!(app.cursor_pos, 2);
    }

    #[test]
    fn test_cursor_movement() {
        let mut app = App::new("test", "test", "none");
        app.input = "hello".to_string();
        app.cursor_pos = 5;

        app.move_cursor_left();
        assert_eq!(app.cursor_pos, 4);

        app.move_cursor_home();
        assert_eq!(app.cursor_pos, 0);

        app.move_cursor_end();
        assert_eq!(app.cursor_pos, 5);

        app.move_cursor_right();
        assert_eq!(app.cursor_pos, 5); // already at end
    }

    #[test]
    fn test_submit_input() {
        let mut app = App::new("test", "test", "none");
        app.input = "  hello world  ".to_string();
        app.cursor_pos = 10;
        let text = app.submit_input();
        assert_eq!(text, "hello world");
        assert!(app.input.is_empty());
        assert_eq!(app.cursor_pos, 0);
    }

    #[test]
    fn test_slash_commands() {
        assert!(matches!(
            App::handle_slash_command("/quit"),
            SlashResult::Quit
        ));
        assert!(matches!(
            App::handle_slash_command("/clear"),
            SlashResult::Clear
        ));
        assert!(matches!(
            App::handle_slash_command("/help"),
            SlashResult::Help
        ));
        assert!(matches!(
            App::handle_slash_command("hello"),
            SlashResult::None
        ));
    }

    #[test]
    fn test_scroll() {
        let mut app = App::new("test", "test", "none");
        app.scroll_up(5);
        assert_eq!(app.scroll_offset, 5);
        app.scroll_down(3);
        assert_eq!(app.scroll_offset, 2);
        app.scroll_down(10);
        assert_eq!(app.scroll_offset, 0);
    }

    #[test]
    fn test_spinner() {
        let mut app = App::new("test", "test", "none");
        let c0 = app.spinner_char();
        app.tick_spinner();
        let c1 = app.spinner_char();
        assert_ne!(c0, c1);
    }

    #[test]
    fn test_unicode_input() {
        let mut app = App::new("test", "test", "none");
        app.insert_char('你');
        app.insert_char('好');
        assert_eq!(app.input, "你好");
        assert_eq!(app.cursor_pos, 6); // 3 bytes each

        app.move_cursor_left();
        assert_eq!(app.cursor_pos, 3);

        app.delete_char_before();
        assert_eq!(app.input, "好");
        assert_eq!(app.cursor_pos, 0);
    }
}
