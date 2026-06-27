// Test lane: default

//! Optional crossterm terminal runner for [`ConfigUiApp`].
//!
//! Ratconfig owns terminal setup, rendering, crossterm event reads, and key
//! conversion in this module. Host applications still own validation,
//! persistence, model reloads, notices, and post-save apply behavior through
//! the intent callback.

use crate::{ConfigUiApp, ConfigUiIntent, ConfigUiKey, UiRowRef, draw_config_ui_with_details};
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{DefaultTerminal, text::Line};
use std::{error::Error, fmt, io};

#[derive(Debug)]
pub enum CrosstermRunnerError<HostError> {
    Terminal(io::Error),
    Host(HostError),
}

impl<HostError> From<io::Error> for CrosstermRunnerError<HostError> {
    fn from(error: io::Error) -> Self {
        Self::Terminal(error)
    }
}

impl<HostError: fmt::Display> fmt::Display for CrosstermRunnerError<HostError> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Terminal(error) => write!(formatter, "terminal error: {error}"),
            Self::Host(error) => write!(formatter, "host callback error: {error}"),
        }
    }
}

impl<HostError: Error + 'static> Error for CrosstermRunnerError<HostError> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Terminal(error) => Some(error),
            Self::Host(error) => Some(error),
        }
    }
}

pub fn crossterm_key_to_config_ui_key(key: KeyEvent) -> Option<ConfigUiKey> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    match key.code {
        KeyCode::Esc => Some(ConfigUiKey::Esc),
        KeyCode::Enter => Some(ConfigUiKey::Enter),
        KeyCode::Backspace => Some(ConfigUiKey::Backspace),
        KeyCode::Tab => Some(ConfigUiKey::Tab),
        KeyCode::BackTab => Some(ConfigUiKey::BackTab),
        KeyCode::Up => Some(ConfigUiKey::Up),
        KeyCode::Down => Some(ConfigUiKey::Down),
        KeyCode::Left => Some(ConfigUiKey::Left),
        KeyCode::Right => Some(ConfigUiKey::Right),
        KeyCode::Char(ch) => char_key_to_config_ui_key(ch, key.modifiers),
        _ => None,
    }
}

pub fn crossterm_event_to_config_ui_key(event: Event) -> Option<ConfigUiKey> {
    match event {
        Event::Key(key) => crossterm_key_to_config_ui_key(key),
        _ => None,
    }
}

pub fn handle_crossterm_key(app: &mut ConfigUiApp, key: KeyEvent) -> ConfigUiIntent {
    crossterm_key_to_config_ui_key(key).map_or(ConfigUiIntent::None, |key| app.handle_key(key))
}

pub fn handle_crossterm_event(app: &mut ConfigUiApp, event: Event) -> ConfigUiIntent {
    crossterm_event_to_config_ui_key(event).map_or(ConfigUiIntent::None, |key| app.handle_key(key))
}

pub fn run_config_ui<HostError>(
    app: &mut ConfigUiApp,
    handle_intent: impl FnMut(&mut ConfigUiApp, ConfigUiIntent) -> Result<(), HostError>,
) -> Result<(), CrosstermRunnerError<HostError>> {
    run_config_ui_with_details(app, ConfigUiApp::render_details, handle_intent)
}

pub fn run_config_ui_with_details<DetailLines, HandleIntent, HostError>(
    app: &mut ConfigUiApp,
    detail_lines: DetailLines,
    mut handle_intent: HandleIntent,
) -> Result<(), CrosstermRunnerError<HostError>>
where
    DetailLines: Fn(&ConfigUiApp, UiRowRef) -> Vec<Line<'static>>,
    HandleIntent: FnMut(&mut ConfigUiApp, ConfigUiIntent) -> Result<(), HostError>,
{
    let mut terminal = ratatui::try_init().inspect_err(|_| {
        let _ = ratatui::try_restore();
    })?;
    let run_result = run_config_ui_terminal(&mut terminal, app, detail_lines, &mut handle_intent);
    let restore_result = ratatui::try_restore();

    match (run_result, restore_result) {
        (Err(error), _) => Err(error),
        (Ok(()), Err(error)) => Err(error.into()),
        (Ok(()), Ok(())) => Ok(()),
    }
}

fn char_key_to_config_ui_key(ch: char, modifiers: KeyModifiers) -> Option<ConfigUiKey> {
    let unsupported =
        KeyModifiers::ALT | KeyModifiers::SUPER | KeyModifiers::HYPER | KeyModifiers::META;
    if modifiers.intersects(unsupported) {
        return None;
    }
    if modifiers.contains(KeyModifiers::CONTROL) {
        Some(ConfigUiKey::Ctrl(ch))
    } else {
        Some(ConfigUiKey::Char(ch))
    }
}

fn run_config_ui_terminal<DetailLines, HandleIntent, HostError>(
    terminal: &mut DefaultTerminal,
    app: &mut ConfigUiApp,
    detail_lines: DetailLines,
    handle_intent: &mut HandleIntent,
) -> Result<(), CrosstermRunnerError<HostError>>
where
    DetailLines: Fn(&ConfigUiApp, UiRowRef) -> Vec<Line<'static>>,
    HandleIntent: FnMut(&mut ConfigUiApp, ConfigUiIntent) -> Result<(), HostError>,
{
    loop {
        terminal.draw(|frame| draw_config_ui_with_details(frame, app, &detail_lines))?;
        let intent = handle_crossterm_event(app, event::read()?);
        match intent {
            ConfigUiIntent::None => {}
            ConfigUiIntent::Exit => return Ok(()),
            intent => handle_intent(app, intent).map_err(CrosstermRunnerError::Host)?,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ConfigUiApplyStatus, ConfigUiEditBehavior, ConfigUiField, ConfigUiModel, ConfigUiPathOwner,
        ConfigUiValueState, DEFAULT_CONFIG_SOURCE_ID,
    };
    use serde_json::json;
    use std::path::PathBuf;

    // Defends: crossterm input conversion preserves ratconfig's project-agnostic key vocabulary.
    #[test]
    fn converts_supported_crossterm_keys() {
        assert_key(KeyCode::Esc, ConfigUiKey::Esc);
        assert_key(KeyCode::Enter, ConfigUiKey::Enter);
        assert_key(KeyCode::Backspace, ConfigUiKey::Backspace);
        assert_key(KeyCode::Tab, ConfigUiKey::Tab);
        assert_key(KeyCode::BackTab, ConfigUiKey::BackTab);
        assert_key(KeyCode::Up, ConfigUiKey::Up);
        assert_key(KeyCode::Down, ConfigUiKey::Down);
        assert_key(KeyCode::Left, ConfigUiKey::Left);
        assert_key(KeyCode::Right, ConfigUiKey::Right);
        assert_key(KeyCode::Char('j'), ConfigUiKey::Char('j'));
        assert_eq!(
            crossterm_key_to_config_ui_key(key(KeyCode::Char('u'), KeyModifiers::CONTROL)),
            Some(ConfigUiKey::Ctrl('u'))
        );
        assert_eq!(
            crossterm_key_to_config_ui_key(key(
                KeyCode::Char('U'),
                KeyModifiers::SHIFT | KeyModifiers::CONTROL
            )),
            Some(ConfigUiKey::Ctrl('U'))
        );
    }

    // Regression: unsupported crossterm keys and release events do not leak into editor commands.
    #[test]
    fn ignores_unsupported_crossterm_keys() {
        assert_eq!(
            crossterm_key_to_config_ui_key(KeyEvent::new_with_kind(
                KeyCode::Char('q'),
                KeyModifiers::NONE,
                KeyEventKind::Release,
            )),
            None
        );
        assert_eq!(
            crossterm_key_to_config_ui_key(key(KeyCode::Char('u'), KeyModifiers::ALT)),
            None
        );
        assert_eq!(
            crossterm_key_to_config_ui_key(key(KeyCode::F(1), KeyModifiers::NONE)),
            None
        );
    }

    // Defends: event dispatch is only crossterm conversion plus the existing reusable reducer.
    #[test]
    fn dispatches_crossterm_events_to_reducer() {
        let mut app = ConfigUiApp::new(test_model());

        assert_eq!(
            handle_crossterm_event(&mut app, key_event(KeyCode::Char('j'), KeyModifiers::NONE)),
            ConfigUiIntent::None
        );
        assert_eq!(app.selected_row, 1);
        assert_eq!(
            handle_crossterm_key(&mut app, key(KeyCode::Char('e'), KeyModifiers::NONE)),
            ConfigUiIntent::BeginEdit {
                field_index: 1,
                source_id: DEFAULT_CONFIG_SOURCE_ID.to_string(),
                path: "ui.theme".to_string(),
            }
        );

        app.selected_row = 0;
        assert_eq!(
            handle_crossterm_key(&mut app, key(KeyCode::Char(' '), KeyModifiers::NONE)),
            ConfigUiIntent::SetField {
                field_index: 0,
                source_id: DEFAULT_CONFIG_SOURCE_ID.to_string(),
                path: "server.enabled".to_string(),
                value: json!(true),
            }
        );
    }

    // Regression: non-key terminal events are ignored by the crossterm dispatch helper.
    #[test]
    fn ignores_non_key_events() {
        let mut app = ConfigUiApp::new(test_model());

        assert_eq!(
            handle_crossterm_event(&mut app, Event::Resize(120, 40)),
            ConfigUiIntent::None
        );
    }

    fn assert_key(code: KeyCode, expected: ConfigUiKey) {
        assert_eq!(
            crossterm_key_to_config_ui_key(key(code, KeyModifiers::NONE)),
            Some(expected)
        );
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn key_event(code: KeyCode, modifiers: KeyModifiers) -> Event {
        Event::Key(key(code, modifiers))
    }

    fn field(path: &str, kind: &str, value: &str, allowed: &[&str]) -> ConfigUiField {
        ConfigUiField {
            source_id: DEFAULT_CONFIG_SOURCE_ID.to_string(),
            path: path.to_string(),
            display_label: String::new(),
            tab: "general".to_string(),
            kind: kind.to_string(),
            current_value: value.to_string(),
            edit_value: value.to_string(),
            default_value: value.to_string(),
            state: ConfigUiValueState::Explicit,
            description: String::new(),
            allowed_values: allowed.iter().map(|value| (*value).to_string()).collect(),
            validation: String::new(),
            rebuild_required: false,
            apply_status: ConfigUiApplyStatus {
                summary: "after save".to_string(),
                label: "after save".to_string(),
                detail: "The host application applies this field after saving.".to_string(),
                pending: true,
            },
            edit_behavior: ConfigUiEditBehavior::Default,
        }
    }

    fn test_model() -> ConfigUiModel {
        ConfigUiModel {
            active_config_path: PathBuf::from("/tmp/acme/settings.jsonc"),
            cursor_config_path: PathBuf::from("/tmp/acme/cursors.jsonc"),
            default_cursor_config_path: PathBuf::from("/tmp/acme/default_cursors.jsonc"),
            active_config_exists: true,
            config_owner: ConfigUiPathOwner::User,
            config_read_only: false,
            sources: Vec::new(),
            tabs: vec!["general".to_string()],
            fields: vec![
                field("server.enabled", "bool", "false", &[]),
                field("ui.theme", "string", "\"light\"", &["light", "dark"]),
            ],
            sidecars: Vec::new(),
            native_config_statuses: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}
