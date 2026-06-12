use crate::app::App;
use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub fn handle_key_event(key_event: KeyEvent, app: &mut App) -> Result<()> {
    match key_event.code {
        KeyCode::Char('q') | KeyCode::Esc => app.quit(),
        KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => app.quit(),
        KeyCode::Tab | KeyCode::Char('v') | KeyCode::Char(' ') => app.toggle_view_mode(),
        KeyCode::Up | KeyCode::Char('k') => app.on_up(),
        KeyCode::Down | KeyCode::Char('j') => app.on_down(),
        _ => {}
    }
    Ok(())
}
