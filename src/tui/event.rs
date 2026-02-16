use crossterm::event::{self, Event, KeyEvent};
use std::time::Duration;
use tokio::sync::mpsc;

/// Events flowing through the TUI.
#[derive(Debug)]
pub enum AppEvent {
    /// A key press from the terminal.
    Key(KeyEvent),
    /// Periodic tick for spinner animation.
    Tick,
    /// Terminal was resized.
    Resize(u16, u16),
    /// Agent returned a response.
    AgentResponse(String),
    /// Agent encountered an error.
    AgentError(String),
}

/// Bridges crossterm blocking event reads into a tokio mpsc channel.
///
/// Spawns a dedicated `tokio::task::spawn_blocking` so we never block the
/// async runtime. Sends `Tick` every 200 ms when no terminal event arrives.
pub fn spawn_event_reader(tx: mpsc::UnboundedSender<AppEvent>) {
    tokio::task::spawn_blocking(move || {
        let tick = Duration::from_millis(200);
        loop {
            if tx.is_closed() {
                break;
            }
            match event::poll(tick) {
                Ok(true) => {
                    if let Ok(ev) = event::read() {
                        let app_ev = match ev {
                            Event::Key(k) => AppEvent::Key(k),
                            Event::Resize(w, h) => AppEvent::Resize(w, h),
                            _ => continue,
                        };
                        if tx.send(app_ev).is_err() {
                            break;
                        }
                    }
                }
                Ok(false) => {
                    // Timeout — emit a tick for spinner animation
                    if tx.send(AppEvent::Tick).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_event_debug() {
        let ev = AppEvent::Tick;
        let dbg = format!("{ev:?}");
        assert!(dbg.contains("Tick"));
    }

    #[test]
    fn test_agent_response_event() {
        let ev = AppEvent::AgentResponse("hello".to_string());
        assert!(matches!(ev, AppEvent::AgentResponse(s) if s == "hello"));
    }

    #[test]
    fn test_agent_error_event() {
        let ev = AppEvent::AgentError("oops".to_string());
        assert!(matches!(ev, AppEvent::AgentError(s) if s == "oops"));
    }
}
