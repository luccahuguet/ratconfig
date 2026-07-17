// Test lane: default

use super::{
    ConfigUiEditBehavior, ConfigUiField, ConfigUiFieldId, ConfigUiFileAction, ConfigUiModel,
    ConfigUiSettingsView, ConfigUiTheme, UiRowRef,
};
use crate::model::{
    ConfigUiFieldState, config_ui_theme_for_model, field_counts_for_tab, field_current_value,
    field_edit_value, snapshot_field_state, string_list_values_from_json,
    validate_string_choice_value, visible_rows_for_tab_search_in_view,
};
use serde_json::Value as JsonValue;
use std::collections::BTreeSet;
use std::path::PathBuf;
use unicode_segmentation::UnicodeSegmentation;

pub struct ConfigUiApp {
    pub(crate) model: ConfigUiModel,
    pub(crate) active_theme: ConfigUiTheme,
    pub(crate) selected_tab: usize,
    pub(crate) selected_row: usize,
    /// Current view outside active search.
    ///
    /// [`ConfigUiApp::try_new`] selects Core when the model contains a Core/All distinction. Models
    /// whose views contain the same fields start in All. Normal-mode `a` toggles the view when the
    /// selected tab has non-core fields. Search spans All without changing this saved view.
    pub(crate) settings_view: ConfigUiSettingsView,
    pub(crate) search: String,
    pub(crate) search_active: bool,
    pub(crate) edit: Option<ConfigUiEditState>,
    pub(crate) notice: Option<ConfigUiNotice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiNotice {
    pub text: String,
    pub is_error: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiEditState {
    pub field_id: ConfigUiFieldId,
    pub input: String,
    pub mode: ConfigUiEditMode,
    pub choice_index: usize,
    /// Byte offset at a grapheme boundary within `input`; used by text edits only.
    pub cursor: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigUiEditMode {
    Text,
    Choice,
    MultiChoice,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConfigUiRowId {
    Field(ConfigUiFieldId),
    FileAction {
        source_id: String,
        action_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigUiKey {
    Esc,
    Enter,
    Backspace,
    Delete,
    Home,
    End,
    Tab,
    BackTab,
    Up,
    Down,
    Left,
    Right,
    Char(char),
    Paste(String),
    Ctrl(char),
}

#[derive(Debug, Clone, PartialEq)]
pub enum ConfigUiIntent {
    None,
    Exit,
    BeginEdit {
        field_index: usize,
        source_id: String,
        path: String,
    },
    OpenFile {
        file_action_index: usize,
        source_id: String,
        action_id: String,
        path: PathBuf,
        create_if_missing: bool,
    },
    EditTextExternally {
        field_index: usize,
        source_id: String,
        path: String,
        input: String,
    },
    SetField {
        field_index: usize,
        source_id: String,
        path: String,
        value: JsonValue,
    },
    UnsetField {
        field_index: usize,
        source_id: String,
        path: String,
    },
}

pub(crate) fn grapheme_boundary_at_or_before(input: &str, index: usize) -> usize {
    let index = index.min(input.len());
    if index == input.len() {
        return index;
    }
    input
        .grapheme_indices(true)
        .map(|(start, _)| start)
        .take_while(|start| *start <= index)
        .last()
        .unwrap_or(0)
}

fn grapheme_boundary_at_or_after(input: &str, index: usize) -> usize {
    let index = index.min(input.len());
    input
        .grapheme_indices(true)
        .map(|(start, _)| start)
        .find(|start| *start >= index)
        .unwrap_or(input.len())
}

fn previous_grapheme_boundary(input: &str, cursor: usize) -> usize {
    let cursor = grapheme_boundary_at_or_before(input, cursor);
    input[..cursor]
        .grapheme_indices(true)
        .next_back()
        .map_or(0, |(start, _)| start)
}

fn next_grapheme_boundary(input: &str, cursor: usize) -> usize {
    let cursor = grapheme_boundary_at_or_before(input, cursor);
    input[cursor..]
        .graphemes(true)
        .next()
        .map_or(cursor, |grapheme| cursor + grapheme.len())
}

fn move_cursor_left(edit: &mut ConfigUiEditState) {
    edit.cursor = previous_grapheme_boundary(&edit.input, edit.cursor);
}

fn move_cursor_right(edit: &mut ConfigUiEditState) {
    edit.cursor = next_grapheme_boundary(&edit.input, edit.cursor);
}

fn backspace_grapheme(edit: &mut ConfigUiEditState) {
    let start = previous_grapheme_boundary(&edit.input, edit.cursor);
    edit.input.drain(start..edit.cursor);
    edit.cursor = start;
}

fn delete_grapheme(edit: &mut ConfigUiEditState) {
    let end = next_grapheme_boundary(&edit.input, edit.cursor);
    edit.input.drain(edit.cursor..end);
}

fn insert_char(edit: &mut ConfigUiEditState, ch: char) {
    let cursor = edit.cursor;
    edit.input.insert(cursor, ch);
    edit.cursor = grapheme_boundary_at_or_after(&edit.input, cursor + ch.len_utf8());
}

fn insert_at_cursor(edit: &mut ConfigUiEditState, text: &str) {
    let cursor = edit.cursor;
    edit.input.insert_str(cursor, text);
    edit.cursor = grapheme_boundary_at_or_after(&edit.input, cursor + text.len());
}

impl ConfigUiApp {
    /// Validates a model and creates an editor.
    ///
    /// Models with `core_fields: None` start in All because every field belongs to Core.
    pub fn try_new(model: ConfigUiModel) -> Result<Self, String> {
        model.validate()?;
        let active_theme = config_ui_theme_for_model(&model, ConfigUiTheme::Dark);
        let settings_view = if model_has_non_core_fields(&model) {
            ConfigUiSettingsView::Core
        } else {
            ConfigUiSettingsView::All
        };
        Ok(Self {
            model,
            active_theme,
            selected_tab: 0,
            selected_row: 0,
            settings_view,
            search: String::new(),
            search_active: false,
            edit: None,
            notice: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn new(model: ConfigUiModel) -> Self {
        Self::try_new(model).expect("test model must be valid")
    }

    pub fn model(&self) -> &ConfigUiModel {
        &self.model
    }

    pub fn active_theme(&self) -> ConfigUiTheme {
        self.active_theme
    }

    pub fn selected_tab(&self) -> usize {
        self.selected_tab
    }

    pub fn selected_row(&self) -> usize {
        self.selected_row
    }

    pub fn settings_view(&self) -> ConfigUiSettingsView {
        self.settings_view
    }

    pub fn search(&self) -> &str {
        &self.search
    }

    pub fn search_active(&self) -> bool {
        self.search_active
    }

    pub fn edit(&self) -> Option<&ConfigUiEditState> {
        self.edit.as_ref()
    }

    pub fn notice(&self) -> Option<&ConfigUiNotice> {
        self.notice.as_ref()
    }

    pub fn replace_model(&mut self, model: ConfigUiModel) -> Result<(), String> {
        self.replace_model_inner(model, None)
    }

    pub fn replace_model_after_success(
        &mut self,
        model: ConfigUiModel,
        completed_field: &ConfigUiFieldId,
    ) -> Result<(), String> {
        self.replace_model_inner(model, Some(completed_field))
    }

    fn replace_model_inner(
        &mut self,
        model: ConfigUiModel,
        completed_field: Option<&ConfigUiFieldId>,
    ) -> Result<(), String> {
        model.validate()?;

        let selected = self
            .visible_rows()
            .get(self.selected_row)
            .and_then(|row| self.row_id(*row));
        let selected_tab = self.model.tabs[self.selected_tab].clone();
        let mut edit = self
            .edit
            .as_ref()
            .filter(|active| completed_field != Some(&active.field_id))
            .cloned();
        let canceled_edit = edit.as_ref().is_some_and(|active| {
            let old = self
                .model
                .fields
                .iter()
                .find(|field| field.matches_id(&active.field_id));
            let new = model
                .fields
                .iter()
                .find(|field| field.matches_id(&active.field_id));
            !matches!((old, new), (Some(old), Some(new)) if edit_metadata_matches(old, new))
        });
        if canceled_edit {
            edit = None;
        }

        let active_theme = config_ui_theme_for_model(&model, self.active_theme);
        let settings_view = if self.settings_view == ConfigUiSettingsView::Core
            && !model_has_non_core_fields(&model)
        {
            ConfigUiSettingsView::All
        } else {
            self.settings_view
        };

        self.model = model;
        self.active_theme = active_theme;
        self.settings_view = settings_view;
        self.edit = edit;
        self.selected_tab = selected
            .as_ref()
            .and_then(|identity| self.tab_for_row_id(identity))
            .or_else(|| {
                self.model
                    .tabs
                    .iter()
                    .position(|candidate| candidate == &selected_tab)
            })
            .unwrap_or(0);
        let rows = self.visible_rows();
        self.selected_row = selected
            .as_ref()
            .and_then(|identity| {
                rows.iter()
                    .position(|row| self.row_id(*row).as_ref() == Some(identity))
            })
            .unwrap_or_else(|| self.selected_row.min(rows.len().saturating_sub(1)));
        if canceled_edit {
            self.notice = Some(cancellation_notice(self.notice.take()));
        }
        Ok(())
    }

    fn field_index_by_id(&self, identity: &ConfigUiFieldId) -> Option<usize> {
        self.model
            .fields
            .iter()
            .position(|field| field.matches_id(identity))
    }

    pub(crate) fn active_edit_field(&self) -> Option<&ConfigUiField> {
        let identity = &self.edit.as_ref()?.field_id;
        self.model
            .fields
            .iter()
            .find(|field| field.matches_id(identity))
    }

    fn row_id(&self, row: UiRowRef) -> Option<ConfigUiRowId> {
        match row {
            UiRowRef::Field(index) => self
                .model
                .fields
                .get(index)
                .map(|field| ConfigUiRowId::Field(field.id())),
            UiRowRef::FileAction(index) => {
                self.model
                    .file_actions
                    .get(index)
                    .map(|action| ConfigUiRowId::FileAction {
                        source_id: action.source_id.clone(),
                        action_id: action.action_id.clone(),
                    })
            }
            UiRowRef::Sidecar(_) | UiRowRef::NativeStatus(_) | UiRowRef::Diagnostic(_) => None,
        }
    }

    fn tab_for_row_id(&self, identity: &ConfigUiRowId) -> Option<usize> {
        let tab = match identity {
            ConfigUiRowId::Field(identity) => self
                .model
                .fields
                .iter()
                .find(|field| field.matches_id(identity))
                .map(|field| field.tab.as_str()),
            ConfigUiRowId::FileAction {
                source_id,
                action_id,
            } => self
                .model
                .file_actions
                .iter()
                .find(|action| action.source_id == *source_id && action.action_id == *action_id)
                .map(|action| action.tab.as_str()),
        }?;
        self.model
            .tabs
            .iter()
            .position(|candidate| candidate == tab)
    }

    pub fn visible_rows(&self) -> Vec<UiRowRef> {
        visible_rows_for_tab_search_in_view(
            &self.model,
            self.selected_tab,
            &self.search,
            self.settings_view,
        )
    }

    pub(crate) fn selected_tab_has_non_core_fields(&self) -> bool {
        let counts = field_counts_for_tab(&self.model, self.selected_tab);
        counts.core < counts.total
    }

    pub(crate) fn can_toggle_settings_view(&self) -> bool {
        !self.search_active && self.search.is_empty() && self.selected_tab_has_non_core_fields()
    }

    fn toggle_settings_view(&mut self) {
        if !self.can_toggle_settings_view() {
            return;
        }
        let selected = self.visible_rows().get(self.selected_row).copied();
        self.settings_view = match self.settings_view {
            ConfigUiSettingsView::Core => ConfigUiSettingsView::All,
            ConfigUiSettingsView::All => ConfigUiSettingsView::Core,
        };
        let rows = self.visible_rows();
        self.selected_row = selected
            .and_then(|selected| rows.iter().position(|row| *row == selected))
            .unwrap_or_else(|| self.selected_row.min(rows.len().saturating_sub(1)));
    }

    pub fn next_tab(&mut self) {
        let len = self.model.tabs.len();
        self.selected_tab = (self.selected_tab + 1) % len;
        self.selected_row = 0;
    }

    pub fn previous_tab(&mut self) {
        let len = self.model.tabs.len();
        self.selected_tab = (self.selected_tab + len - 1) % len;
        self.selected_row = 0;
    }

    pub fn move_down(&mut self) {
        let len = self.visible_rows().len();
        self.clamp_selection_for_len(len);
        if len > 0 {
            self.selected_row = (self.selected_row + 1) % len;
        }
    }

    pub fn move_up(&mut self) {
        let len = self.visible_rows().len();
        self.clamp_selection_for_len(len);
        if len > 0 {
            self.selected_row = self.selected_row.checked_sub(1).unwrap_or(len - 1);
        }
    }

    pub fn clamp_selection(&mut self) {
        self.clamp_selection_for_len(self.visible_rows().len());
    }

    pub fn clamp_selection_for_len(&mut self, len: usize) {
        self.selected_row = self.selected_row.min(len.saturating_sub(1));
    }

    pub fn selected_field_index(&self) -> Option<usize> {
        let row = self.visible_rows().get(self.selected_row).copied()?;
        match row {
            UiRowRef::Field(index) => Some(index),
            _ => None,
        }
    }

    pub fn selected_field(&self) -> Option<&ConfigUiField> {
        self.model.fields.get(self.selected_field_index()?)
    }

    pub(crate) fn selected_file_action(&self) -> Option<(usize, &ConfigUiFileAction)> {
        let UiRowRef::FileAction(index) = self.visible_rows().get(self.selected_row).copied()?
        else {
            return None;
        };
        self.model
            .file_actions
            .get(index)
            .map(|action| (index, action))
    }

    pub(crate) fn selected_structured_file_action(&self) -> Option<(usize, &ConfigUiFileAction)> {
        let field = self.selected_field()?;
        structured_only_edit_notice(field)?;
        let mut matches = self
            .model
            .file_actions
            .iter()
            .enumerate()
            .filter(|(_, action)| action.source_id == field.source_id);
        let action = matches.next()?;
        matches.next().is_none().then_some(action)
    }

    pub fn notice_info(&mut self, text: impl Into<String>) {
        self.notice = Some(ConfigUiNotice {
            text: text.into(),
            is_error: false,
        });
    }

    pub fn notice_error(&mut self, text: impl Into<String>) {
        self.notice = Some(ConfigUiNotice {
            text: text.into(),
            is_error: true,
        });
    }

    pub fn handle_key(&mut self, key: ConfigUiKey) -> ConfigUiIntent {
        if self.edit.is_some() {
            return self.handle_edit_key(key);
        }
        if self.search_active {
            self.handle_search_key(key);
            return ConfigUiIntent::None;
        }
        self.handle_normal_key(key)
    }

    pub fn begin_edit_field(&mut self, field_index: usize) {
        self.notice = None;
        let Some(field) = self.model.fields.get(field_index) else {
            self.notice_error("Only settings rows can be edited.");
            return;
        };
        if let Some(message) = structured_only_edit_notice(field).map(str::to_string) {
            self.notice_info(message);
            return;
        }
        let input = edit_input_for_field(field);
        let cursor = input.len();
        self.edit = Some(ConfigUiEditState {
            field_id: field.id(),
            choice_index: initial_edit_choice_index(field, &input),
            input,
            mode: edit_mode_for_field(field),
            cursor,
        });
    }

    pub fn apply_external_text_edit(
        &mut self,
        field_index: usize,
        input: impl Into<String>,
    ) -> Result<(), String> {
        let Some(edit) = &mut self.edit else {
            return Err("No text edit is active.".to_string());
        };
        let returned_field = self.model.fields.get(field_index).map(ConfigUiField::id);
        if returned_field.as_ref() != Some(&edit.field_id) {
            return Err(format!(
                "Returned text is for a different field than the active edit {}:{}.",
                edit.field_id.source_id, edit.field_id.path
            ));
        }
        if edit.mode != ConfigUiEditMode::Text {
            return Err("External editor text can only replace text edit buffers.".to_string());
        }

        edit.input = input.into();
        edit.cursor = edit.input.len();
        self.notice = None;
        Ok(())
    }

    fn cancel_edit(&mut self) -> ConfigUiIntent {
        self.edit = None;
        self.notice_info("Edit canceled.");
        ConfigUiIntent::None
    }

    fn update_text_edit(&mut self, update: impl FnOnce(&mut ConfigUiEditState)) -> ConfigUiIntent {
        self.notice = None;
        if let Some(edit) = &mut self.edit {
            edit.cursor = grapheme_boundary_at_or_before(&edit.input, edit.cursor);
            update(edit);
        }
        ConfigUiIntent::None
    }

    fn insert_inline_text(&mut self, text: String) -> ConfigUiIntent {
        if text.contains(['\r', '\n']) {
            self.notice_error(
                "Inline editing accepts one line; use the external editor for multiline text.",
            );
            return ConfigUiIntent::None;
        }
        self.update_text_edit(|edit| insert_at_cursor(edit, &text))
    }

    fn handle_search_key(&mut self, key: ConfigUiKey) {
        match key {
            ConfigUiKey::Esc | ConfigUiKey::Enter => self.search_active = false,
            ConfigUiKey::Backspace => {
                let end = previous_grapheme_boundary(&self.search, self.search.len());
                self.search.truncate(end);
            }
            ConfigUiKey::Ctrl('u' | 'U') => {
                self.search.clear();
            }
            ConfigUiKey::Char('\r' | '\n') => {}
            ConfigUiKey::Char(ch) => {
                self.search.push(ch);
                self.selected_row = 0;
            }
            ConfigUiKey::Paste(text) if !text.contains(['\r', '\n']) => {
                self.search.push_str(&text);
                self.selected_row = 0;
            }
            _ => {}
        }
        self.clamp_selection();
    }

    fn handle_normal_key(&mut self, key: ConfigUiKey) -> ConfigUiIntent {
        match key {
            ConfigUiKey::Esc if !self.search.is_empty() => {
                self.search.clear();
                self.clamp_selection();
                ConfigUiIntent::None
            }
            ConfigUiKey::Char('q') | ConfigUiKey::Esc | ConfigUiKey::Ctrl('c') => {
                ConfigUiIntent::Exit
            }
            ConfigUiKey::Char('a') => {
                self.toggle_settings_view();
                ConfigUiIntent::None
            }
            ConfigUiKey::Char('/') => {
                self.search_active = true;
                ConfigUiIntent::None
            }
            ConfigUiKey::Char('j') | ConfigUiKey::Down => {
                self.move_down();
                ConfigUiIntent::None
            }
            ConfigUiKey::Char('k') | ConfigUiKey::Up => {
                self.move_up();
                ConfigUiIntent::None
            }
            ConfigUiKey::Enter
                if self.selected_field().is_some_and(|field| {
                    is_bool_field(field) && structured_only_edit_notice(field).is_none()
                }) =>
            {
                self.notice_info("Press Space to stage this change, then Enter to save.");
                ConfigUiIntent::None
            }
            ConfigUiKey::Enter | ConfigUiKey::Char(' ') => self.activate_selected_row(),
            ConfigUiKey::Char('e') => self.edit_or_activate_selected_row(),
            ConfigUiKey::Char('u') => self.return_selected_field_to_default(),
            ConfigUiKey::Char(ch @ '1'..='9') => {
                let index = usize::from(ch as u8 - b'1');
                if index < self.model.tabs.len() {
                    self.selected_tab = index;
                    self.selected_row = 0;
                }
                ConfigUiIntent::None
            }
            ConfigUiKey::Tab | ConfigUiKey::Right | ConfigUiKey::Char('l') => {
                self.next_tab();
                ConfigUiIntent::None
            }
            ConfigUiKey::BackTab | ConfigUiKey::Left | ConfigUiKey::Char('h') => {
                self.previous_tab();
                ConfigUiIntent::None
            }
            _ => ConfigUiIntent::None,
        }
    }

    fn handle_edit_key(&mut self, key: ConfigUiKey) -> ConfigUiIntent {
        if let Some(mode @ (ConfigUiEditMode::Choice | ConfigUiEditMode::MultiChoice)) =
            self.edit.as_ref().map(|edit| edit.mode)
        {
            return self.handle_choice_edit_key(key, mode);
        }

        match key {
            ConfigUiKey::Esc => self.cancel_edit(),
            ConfigUiKey::Enter => self.save_edit(),
            ConfigUiKey::Ctrl('e' | 'E') => self.edit_text_externally(),
            ConfigUiKey::Left => self.update_text_edit(move_cursor_left),
            ConfigUiKey::Right => self.update_text_edit(move_cursor_right),
            ConfigUiKey::Home => self.update_text_edit(|edit| edit.cursor = 0),
            ConfigUiKey::End => self.update_text_edit(|edit| edit.cursor = edit.input.len()),
            ConfigUiKey::Backspace => self.update_text_edit(backspace_grapheme),
            ConfigUiKey::Delete => self.update_text_edit(delete_grapheme),
            ConfigUiKey::Ctrl('u' | 'U') => self.update_text_edit(|edit| {
                edit.input.clear();
                edit.cursor = 0;
            }),
            ConfigUiKey::Char(ch @ ('\r' | '\n')) => self.insert_inline_text(ch.to_string()),
            ConfigUiKey::Char(ch) => self.update_text_edit(|edit| insert_char(edit, ch)),
            ConfigUiKey::Paste(text) => self.insert_inline_text(text),
            _ => ConfigUiIntent::None,
        }
    }

    fn handle_choice_edit_key(
        &mut self,
        key: ConfigUiKey,
        mode: ConfigUiEditMode,
    ) -> ConfigUiIntent {
        let field = self.active_edit_field();
        let scalar_enum = field.is_some_and(is_scalar_enum_field);
        let ordered_string_list = field.is_some_and(is_ordered_string_list_field);
        let multi_choice = mode == ConfigUiEditMode::MultiChoice;
        match key {
            ConfigUiKey::Esc => self.cancel_edit(),
            ConfigUiKey::Enter if scalar_enum => {
                self.select_single_choice_edit();
                self.save_edit()
            }
            ConfigUiKey::Enter => self.save_edit(),
            ConfigUiKey::Char('K') if ordered_string_list => {
                self.notice = None;
                self.move_ordered_string_list_edit(-1);
                ConfigUiIntent::None
            }
            ConfigUiKey::Char('J') if ordered_string_list => {
                self.notice = None;
                self.move_ordered_string_list_edit(1);
                ConfigUiIntent::None
            }
            ConfigUiKey::Up | ConfigUiKey::Left | ConfigUiKey::Char('k' | 'h')
                if scalar_enum || multi_choice =>
            {
                self.notice = None;
                self.move_choice_edit(-1);
                ConfigUiIntent::None
            }
            ConfigUiKey::Down | ConfigUiKey::Right | ConfigUiKey::Char('j' | 'l')
                if scalar_enum || multi_choice =>
            {
                self.notice = None;
                self.move_choice_edit(1);
                ConfigUiIntent::None
            }
            ConfigUiKey::Char(' ') if multi_choice => {
                self.notice = None;
                self.toggle_multi_choice_edit();
                ConfigUiIntent::None
            }
            ConfigUiKey::Char(' ') if scalar_enum => {
                self.notice = None;
                self.select_single_choice_edit();
                ConfigUiIntent::None
            }
            ConfigUiKey::Up
            | ConfigUiKey::Right
            | ConfigUiKey::Down
            | ConfigUiKey::Left
            | ConfigUiKey::Char(' ')
                if !multi_choice =>
            {
                self.notice = None;
                self.cycle_choice_edit();
                ConfigUiIntent::None
            }
            _ => ConfigUiIntent::None,
        }
    }

    fn cycle_choice_edit(&mut self) {
        if let Some(edit) = &mut self.edit {
            edit.input = if edit.input.trim() == "true" {
                "false".to_string()
            } else {
                "true".to_string()
            };
            edit.cursor = edit.input.len();
        }
    }

    fn move_choice_edit(&mut self, delta: isize) {
        let len = self
            .active_edit_field()
            .map_or(0, |field| field.allowed_values.len());
        let Some(edit) = &mut self.edit else {
            return;
        };
        if len == 0 {
            return;
        }
        let index = edit.choice_index.min(len - 1);
        let next = if delta < 0 {
            index.checked_sub(1).unwrap_or(len - 1)
        } else {
            (index + 1) % len
        };
        edit.choice_index = next;
    }

    fn select_single_choice_edit(&mut self) {
        let Some(value) = self.active_edit_field().and_then(|field| {
            self.edit
                .as_ref()
                .and_then(|edit| field.allowed_values.get(edit.choice_index))
        }) else {
            return;
        };
        let value = value.clone();
        let Some(edit) = &mut self.edit else {
            return;
        };
        edit.input = value;
        edit.cursor = edit.input.len();
    }

    fn toggle_multi_choice_edit(&mut self) {
        self.replace_choice_input(|field, edit| {
            toggled_string_list_input(field, &edit.input, edit.choice_index)
        });
    }

    fn move_ordered_string_list_edit(&mut self, delta: isize) {
        self.replace_choice_input(|field, edit| {
            moved_ordered_string_list_input(field, &edit.input, edit.choice_index, delta)
        });
    }

    fn replace_choice_input(
        &mut self,
        next_input: impl FnOnce(&ConfigUiField, &ConfigUiEditState) -> Result<String, String>,
    ) {
        let Some(edit) = self.edit.as_ref() else {
            return;
        };
        let Some(field) = self.active_edit_field().cloned() else {
            return;
        };
        let selected = if is_ordered_string_list_field(&field) {
            string_list_choice_value(&field, &edit.input, edit.choice_index).ok()
        } else {
            None
        };
        let next = match next_input(&field, edit) {
            Ok(next) => next,
            Err(message) => {
                self.notice_error(message);
                return;
            }
        };
        if let Some(edit) = &mut self.edit {
            edit.input = next;
            edit.cursor = edit.input.len();
            if let Some(value) = selected
                && let Some(index) = string_list_choice_index(&field, &edit.input, &value)
            {
                edit.choice_index = index;
            }
        }
    }

    fn begin_edit_selected_field(&mut self) -> ConfigUiIntent {
        self.notice = None;
        let Some(field_index) = self.selected_field_index() else {
            self.notice_error("Only settings rows can be edited.");
            return ConfigUiIntent::None;
        };
        let field = &self.model.fields[field_index];
        ConfigUiIntent::BeginEdit {
            field_index,
            source_id: field.source_id.clone(),
            path: field.path.clone(),
        }
    }

    fn edit_or_activate_selected_row(&mut self) -> ConfigUiIntent {
        if let Some((index, _)) = self.selected_file_action() {
            return self.activate_file_action(index);
        }
        if let Some((index, _)) = self.selected_structured_file_action() {
            return self.activate_file_action(index);
        }
        if let Some(field_index) = self.selected_field_index()
            && edit_mode_for_field(&self.model.fields[field_index]) == ConfigUiEditMode::Text
        {
            self.begin_edit_field(field_index);
            return self.edit_text_externally();
        }
        self.begin_edit_selected_field()
    }

    fn activate_selected_row(&mut self) -> ConfigUiIntent {
        if let Some((index, _)) = self.selected_file_action() {
            return self.activate_file_action(index);
        }
        self.quick_edit_selected_field()
    }

    fn activate_file_action(&mut self, file_action_index: usize) -> ConfigUiIntent {
        self.notice = None;
        let action = &self.model.file_actions[file_action_index];
        if let Some(reason) = &action.disabled_reason {
            self.notice_error(reason.clone());
            return ConfigUiIntent::None;
        }
        ConfigUiIntent::OpenFile {
            file_action_index,
            source_id: action.source_id.clone(),
            action_id: action.action_id.clone(),
            path: action.path.clone(),
            create_if_missing: action.create_if_missing && !action.exists,
        }
    }

    fn edit_text_externally(&mut self) -> ConfigUiIntent {
        let Some(edit) = self.edit.as_ref() else {
            return ConfigUiIntent::None;
        };
        if edit.mode != ConfigUiEditMode::Text {
            return ConfigUiIntent::None;
        }
        let Some(field_index) = self.field_index_by_id(&edit.field_id) else {
            self.notice_error("Active edit field is unavailable.");
            return ConfigUiIntent::None;
        };
        let input = edit.input.clone();
        let field = &self.model.fields[field_index];
        let source_id = field.source_id.clone();
        let path = field.path.clone();
        self.notice = None;
        ConfigUiIntent::EditTextExternally {
            field_index,
            source_id,
            path,
            input,
        }
    }

    fn quick_edit_selected_field(&mut self) -> ConfigUiIntent {
        self.notice = None;
        let Some(field_index) = self.selected_field_index() else {
            self.notice_error("Only settings rows can be edited.");
            return ConfigUiIntent::None;
        };
        let field = &self.model.fields[field_index];
        if is_bool_field(field) {
            self.begin_edit_field(field_index);
            self.cycle_choice_edit();
            ConfigUiIntent::None
        } else {
            self.begin_edit_selected_field()
        }
    }

    fn return_selected_field_to_default(&mut self) -> ConfigUiIntent {
        self.notice = None;
        let Some(field_index) = self.selected_field_index() else {
            self.notice_error("Only settings rows can be returned to default.");
            return ConfigUiIntent::None;
        };
        let field = &self.model.fields[field_index];
        if let Some(message) = structured_only_edit_notice(field).map(str::to_string) {
            self.notice_info(message);
            return ConfigUiIntent::None;
        }
        if !field.has_baseline_value() {
            self.notice_info("This setting has no default value.");
            return ConfigUiIntent::None;
        }
        ConfigUiIntent::UnsetField {
            field_index,
            source_id: field.source_id.clone(),
            path: field.path.clone(),
        }
    }

    fn save_edit(&mut self) -> ConfigUiIntent {
        let Some(edit) = self.edit.as_ref() else {
            return ConfigUiIntent::None;
        };
        let Some(field_index) = self.field_index_by_id(&edit.field_id) else {
            self.notice_error("Active edit field is unavailable.");
            return ConfigUiIntent::None;
        };
        let value = match parse_edit_input(&self.model.fields[field_index], &edit.input) {
            Ok(value) => value,
            Err(message) => {
                self.notice_error(message);
                return ConfigUiIntent::None;
            }
        };
        let field = &self.model.fields[field_index];
        ConfigUiIntent::SetField {
            field_index,
            source_id: field.source_id.clone(),
            path: field.path.clone(),
            value,
        }
    }
}

fn model_has_non_core_fields(model: &ConfigUiModel) -> bool {
    (0..model.tabs.len()).any(|tab| {
        let counts = field_counts_for_tab(model, tab);
        counts.core < counts.total
    })
}

fn edit_metadata_matches(old: &ConfigUiField, new: &ConfigUiField) -> bool {
    old.kind == new.kind
        && old.allowed_values == new.allowed_values
        && old.edit_behavior == new.edit_behavior
}

fn cancellation_notice(existing: Option<ConfigUiNotice>) -> ConfigUiNotice {
    let reason = "Edit canceled after reload because the field disappeared or its editor changed.";
    match existing {
        Some(existing) => ConfigUiNotice {
            text: format!("{reason} {}", existing.text),
            is_error: true,
        },
        None => ConfigUiNotice {
            text: reason.to_string(),
            is_error: true,
        },
    }
}

pub fn edit_input_for_field(field: &ConfigUiField) -> String {
    let edit_value = field_edit_value(field);
    if snapshot_field_state(field) == ConfigUiFieldState::Absent {
        if is_bool_field(field) {
            return "false".to_string();
        }
        if is_scalar_enum_field(field) {
            return field.allowed_values[0].clone();
        }
        return String::new();
    }
    if field.edit_behavior == ConfigUiEditBehavior::FriendlyStringList {
        return friendly_string_list_edit_input(field);
    }
    if field.kind == "string" {
        return parse_rendered_json_string(&edit_value).unwrap_or(edit_value);
    }
    if edit_value.is_empty() {
        field_current_value(field)
    } else {
        edit_value
    }
}

pub fn edit_mode_for_field(field: &ConfigUiField) -> ConfigUiEditMode {
    if is_enum_string_list_field(field) {
        ConfigUiEditMode::MultiChoice
    } else if is_direct_choice_field(field) {
        ConfigUiEditMode::Choice
    } else {
        ConfigUiEditMode::Text
    }
}

pub fn initial_edit_choice_index(field: &ConfigUiField, input: &str) -> usize {
    if is_scalar_enum_field(field)
        && let Some(index) = field
            .allowed_values
            .iter()
            .position(|allowed| allowed == input)
    {
        return index;
    }
    if is_enum_string_list_field(field)
        && let Ok(values) = parse_string_list_values(field, input)
        && let Some(index) = values
            .first()
            .and_then(|value| string_list_choice_index(field, input, value))
    {
        return index;
    }
    0
}

pub fn parse_edit_input(field: &ConfigUiField, input: &str) -> Result<JsonValue, String> {
    let trimmed = input.trim();
    match field.kind.as_str() {
        "bool" | "boolean" => parse_bool_input(field, trimmed),
        "int" | "integer" => parse_i64_input(field, trimmed),
        "float" | "number" => parse_f64_input(field, trimmed),
        "string" => parse_string_field_input(field, input),
        "string_list" if field.edit_behavior == ConfigUiEditBehavior::FriendlyStringList => {
            parse_friendly_string_list_input(field, trimmed)
        }
        "string_list" => parse_string_list_input(field, trimmed),
        "array" => parse_json_input(field, trimmed, "JSON array").and_then(|value| {
            if value.is_array() {
                Ok(value)
            } else {
                Err(format!("{} must be a JSON array.", field.path))
            }
        }),
        "object" => parse_json_input(field, trimmed, "JSON object").and_then(|value| {
            if value.is_object() {
                Ok(value)
            } else {
                Err(format!("{} must be a JSON object.", field.path))
            }
        }),
        _ => parse_json_input(field, trimmed, "JSON value"),
    }
}

fn parse_bool_input(field: &ConfigUiField, input: &str) -> Result<JsonValue, String> {
    match input {
        "true" => Ok(JsonValue::Bool(true)),
        "false" => Ok(JsonValue::Bool(false)),
        _ => Err(format!("{} must be true or false.", field.path)),
    }
}

fn parse_i64_input(field: &ConfigUiField, input: &str) -> Result<JsonValue, String> {
    let value = input
        .parse::<i64>()
        .map_err(|_| format!("{} must be an integer.", field.path))?;
    Ok(JsonValue::Number(value.into()))
}

fn parse_f64_input(field: &ConfigUiField, input: &str) -> Result<JsonValue, String> {
    let value = input
        .parse::<f64>()
        .map_err(|_| format!("{} must be a number.", field.path))?;
    let number = serde_json::Number::from_f64(value)
        .ok_or_else(|| format!("{} must be a finite number.", field.path))?;
    Ok(JsonValue::Number(number))
}

fn parse_string_field_input(field: &ConfigUiField, input: &str) -> Result<JsonValue, String> {
    let value = parse_string_input(input)
        .map_err(|message| format!("{} must be a string: {message}.", field.path))?;
    ensure_allowed_value(field, &value)?;
    Ok(JsonValue::String(value))
}

fn parse_string_list_input(field: &ConfigUiField, input: &str) -> Result<JsonValue, String> {
    let strings = parse_string_list_values(field, input)?;
    Ok(JsonValue::Array(
        strings.into_iter().map(JsonValue::String).collect(),
    ))
}

fn parse_friendly_string_list_input(
    field: &ConfigUiField,
    input: &str,
) -> Result<JsonValue, String> {
    if input.starts_with('[') {
        return parse_string_list_input(field, input);
    }
    if input.is_empty() || input.eq_ignore_ascii_case("disabled") {
        return Ok(JsonValue::Array(Vec::new()));
    }
    let strings = input
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    for value in &strings {
        ensure_allowed_value(field, value)?;
    }
    Ok(JsonValue::Array(
        strings.into_iter().map(JsonValue::String).collect(),
    ))
}

pub fn parse_string_list_values(field: &ConfigUiField, input: &str) -> Result<Vec<String>, String> {
    let value = parse_json_input(field, input, "JSON string array")?;
    string_list_values_from_json(&field.path, &value, &field.allowed_values)
}

fn parse_json_input(
    field: &ConfigUiField,
    input: &str,
    expected: &str,
) -> Result<JsonValue, String> {
    serde_json::from_str::<JsonValue>(input)
        .map_err(|source| format!("{} must be a valid {expected}: {source}.", field.path))
}

fn parse_string_input(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.starts_with('"') {
        serde_json::from_str::<String>(trimmed).map_err(|source| source.to_string())
    } else {
        Ok(input.to_string())
    }
}

pub fn parse_rendered_json_string(value: &str) -> Option<String> {
    serde_json::from_str::<String>(value).ok()
}

fn ensure_allowed_value(field: &ConfigUiField, value: &str) -> Result<(), String> {
    validate_string_choice_value(&field.path, value, &field.allowed_values)
}

pub fn single_choice_status_value(field: &ConfigUiField, edit: &ConfigUiEditState) -> String {
    let highlighted = field
        .allowed_values
        .get(edit.choice_index)
        .map(String::as_str)
        .unwrap_or("none");
    if highlighted == edit.input {
        format!("selected {}", edit.input)
    } else {
        format!("selected {}, highlighted {highlighted}", edit.input)
    }
}

pub fn multi_choice_status_value(field: &ConfigUiField, edit: &ConfigUiEditState) -> String {
    let values = parse_string_list_values(field, &edit.input).unwrap_or_default();
    let enabled = values.len();
    let selected = string_list_choice_values(field, &edit.input)
        .ok()
        .and_then(|choices| choices.get(edit.choice_index).cloned())
        .unwrap_or_else(|| "none".to_string());
    if is_ordered_string_list_field(field) {
        return format!(
            "{enabled}/{} enabled, selected {selected}, order {}",
            field.allowed_values.len(),
            string_list_order_label(&values)
        );
    }
    format!(
        "{enabled}/{} enabled, selected {selected}",
        field.allowed_values.len()
    )
}

pub(crate) fn string_list_order_label(values: &[String]) -> String {
    if values.is_empty() {
        "none".to_string()
    } else {
        values.join(", ")
    }
}

pub fn toggled_string_list_input(
    field: &ConfigUiField,
    input: &str,
    choice_index: usize,
) -> Result<String, String> {
    let target = string_list_choice_value(field, input, choice_index)?;
    let mut values = parse_string_list_values(field, input)?;
    if values.iter().any(|value| value == &target) {
        values.retain(|value| value != &target);
    } else {
        values.push(target);
    }
    if !is_ordered_string_list_field(field) {
        values = ordered_string_list_values(field, &values);
    }
    render_string_list_input(field, &values)
}

fn moved_ordered_string_list_input(
    field: &ConfigUiField,
    input: &str,
    choice_index: usize,
    delta: isize,
) -> Result<String, String> {
    let target = string_list_choice_value(field, input, choice_index)?;
    let mut values = parse_string_list_values(field, input)?;
    let Some(index) = values.iter().position(|value| value == &target) else {
        return render_string_list_input(field, &values);
    };
    let next = if delta < 0 {
        index.checked_sub(1)
    } else {
        (index + 1 < values.len()).then_some(index + 1)
    };
    let Some(next) = next else {
        return render_string_list_input(field, &values);
    };
    values.swap(index, next);
    render_string_list_input(field, &values)
}

fn string_list_choice_value(
    field: &ConfigUiField,
    input: &str,
    choice_index: usize,
) -> Result<String, String> {
    string_list_choice_values(field, input)?
        .get(choice_index)
        .cloned()
        .ok_or_else(|| format!("{} has no value selected.", field.path))
}

fn string_list_choice_index(field: &ConfigUiField, input: &str, value: &str) -> Option<usize> {
    string_list_choice_values(field, input)
        .ok()?
        .iter()
        .position(|choice| choice == value)
}

pub(crate) fn string_list_choice_values(
    field: &ConfigUiField,
    input: &str,
) -> Result<Vec<String>, String> {
    if !is_ordered_string_list_field(field) {
        return Ok(field.allowed_values.clone());
    }
    let mut values = parse_string_list_values(field, input)?;
    let enabled = values.iter().cloned().collect::<BTreeSet<_>>();
    values.extend(
        field
            .allowed_values
            .iter()
            .filter(|value| !enabled.contains(*value))
            .cloned(),
    );
    Ok(values)
}

fn render_string_list_input(field: &ConfigUiField, values: &[String]) -> Result<String, String> {
    serde_json::to_string(values)
        .map_err(|source| format!("Could not render {} string list: {source}.", field.path))
}

fn ordered_string_list_values(field: &ConfigUiField, values: &[String]) -> Vec<String> {
    let selected = values.iter().cloned().collect::<BTreeSet<_>>();
    field
        .allowed_values
        .iter()
        .filter(|value| selected.contains(*value))
        .cloned()
        .collect()
}

pub fn is_bool_field(field: &ConfigUiField) -> bool {
    matches!(field.kind.as_str(), "bool" | "boolean")
}

fn is_direct_choice_field(field: &ConfigUiField) -> bool {
    is_bool_field(field) || is_scalar_enum_field(field)
}

pub fn is_scalar_enum_field(field: &ConfigUiField) -> bool {
    field.kind == "string" && !field.allowed_values.is_empty()
}

pub fn is_enum_string_list_field(field: &ConfigUiField) -> bool {
    field.kind == "string_list" && !field.allowed_values.is_empty()
}

pub(crate) fn is_ordered_string_list_field(field: &ConfigUiField) -> bool {
    is_enum_string_list_field(field)
        && field.edit_behavior == ConfigUiEditBehavior::OrderedStringList
}

pub fn structured_only_edit_notice(field: &ConfigUiField) -> Option<&str> {
    if let ConfigUiEditBehavior::StructuredOnly { notice } = &field.edit_behavior {
        return Some(notice.as_str());
    }
    if matches!(field.kind.as_str(), "array" | "object" | "string_list_map") {
        return Some(
            "Structured editor unavailable for this complex field; edit the source config directly.",
        );
    }
    None
}

fn friendly_string_list_edit_input(field: &ConfigUiField) -> String {
    let edit_value = field_edit_value(field);
    serde_json::from_str::<Vec<String>>(&edit_value)
        .map(|keys| keys.join(", "))
        .unwrap_or(edit_value)
}

pub fn field_bool_value(field: &ConfigUiField) -> Option<bool> {
    field_current_value(field).parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "ui")]
    use crate::row_line_for_model;
    use crate::{
        ConfigUiFieldId, ConfigUiFieldState, ConfigUiOverride, ConfigUiResolvedValue,
        ConfigUiTheme, ConfigUiThemeMapping, ConfigUiThemeSwitcher, ConfigUiTomlDocumentSpec,
        DEFAULT_CONFIG_SOURCE_ID, build_toml_document_fields,
        test_support::{after_save_status, field, field_with_source, model_with_fields},
    };
    #[cfg(feature = "ui")]
    use crate::{patch::PatchMutation, toml_adapter::set_toml_value_text};
    use serde_json::json;
    use std::path::PathBuf;

    // Defends: the reusable ratconfig layer can drive a non-Yazelix config fixture with bool, select, multiselect, rendering, and TOML patching.
    #[cfg(feature = "ui")]
    #[test]
    fn non_yazelix_fixture_uses_generic_model_editor_render_and_toml_patch() {
        let model = test_model();
        let app = ConfigUiApp::new(model);

        assert_eq!(app.visible_rows().len(), 3);
        assert_eq!(
            row_line_for_model(&app.model, app.visible_rows()[0])
                .spans
                .iter()
                .map(|span| span.content.trim().to_string())
                .collect::<Vec<_>>(),
            vec!["after save", "server.enabled", "false"]
        );
        assert_eq!(
            parse_edit_input(&app.model.fields[0], "true").expect("bool"),
            json!(true)
        );
        assert_eq!(
            parse_edit_input(&app.model.fields[1], "dark").expect("select"),
            json!("dark")
        );
        assert_eq!(
            toggled_string_list_input(&app.model.fields[2], r#"["git"]"#, 1).expect("toggle"),
            r#"["git","search"]"#
        );

        let raw = r#"# host-owned config
[ui]
theme = "light"
"#;
        let patched =
            set_toml_value_text(raw, "ui.theme", &json!("dark")).expect("generic TOML patch");
        assert_eq!(patched.mutation, PatchMutation::Replaced);
        assert!(patched.text.contains("# host-owned config"));
        assert!(patched.text.contains(r#"theme = "dark""#));
    }

    // Defends: normal-mode keyboard reduction is project-agnostic and emits semantic edit/write intents for the host.
    #[test]
    fn reducer_emits_normal_mode_intents_without_host_policy() {
        let mut app = ConfigUiApp::new(test_model());

        assert_eq!(app.handle_key(ConfigUiKey::Char('j')), ConfigUiIntent::None);
        assert_eq!(app.selected_row, 1);
        assert_eq!(
            app.handle_key(ConfigUiKey::Char('e')),
            ConfigUiIntent::BeginEdit {
                field_index: 1,
                source_id: DEFAULT_CONFIG_SOURCE_ID.to_string(),
                path: "ui.theme".to_string()
            }
        );
        assert_eq!(
            app.handle_key(ConfigUiKey::Char('u')),
            ConfigUiIntent::UnsetField {
                field_index: 1,
                source_id: DEFAULT_CONFIG_SOURCE_ID.to_string(),
                path: "ui.theme".to_string()
            }
        );

        app.selected_row = 0;
        assert_eq!(app.handle_key(ConfigUiKey::Char(' ')), ConfigUiIntent::None);
        assert_eq!(app.edit.as_ref().expect("staged bool").input, "true");
        assert_eq!(app.handle_key(ConfigUiKey::Esc), ConfigUiIntent::None);
        assert!(app.edit.is_none());

        assert_eq!(app.handle_key(ConfigUiKey::Enter), ConfigUiIntent::None);
        assert!(app.edit.is_none());
        assert_eq!(field_current_value(&app.model.fields[0]), "false");
        assert_eq!(
            app.notice.as_ref().map(|notice| notice.text.as_str()),
            Some("Press Space to stage this change, then Enter to save.")
        );

        assert_eq!(app.handle_key(ConfigUiKey::Char(' ')), ConfigUiIntent::None);
        assert_eq!(app.edit.as_ref().expect("staged bool").input, "true");
        assert_eq!(
            app.handle_key(ConfigUiKey::Enter),
            ConfigUiIntent::SetField {
                field_index: 0,
                source_id: DEFAULT_CONFIG_SOURCE_ID.to_string(),
                path: "server.enabled".to_string(),
                value: json!(true),
            }
        );
        complete_set(
            &mut app,
            ConfigUiFieldId::new(DEFAULT_CONFIG_SOURCE_ID, "server.enabled"),
            json!(true),
        );
        assert_eq!(app.handle_key(ConfigUiKey::Esc), ConfigUiIntent::Exit);
    }

    // Defends: edit intents carry source identity and completed writes return to normal routing.
    #[test]
    fn edit_intents_preserve_selected_field_source() {
        let mut model = model_with_fields(vec![
            field_with_source("server", "server.enabled", "bool", "false", &[]),
            field_with_source("ui", "ui.title", "string", "\"light\"", &[]),
        ]);
        model.fields[0].display_label = "Server enabled".to_string();
        model.fields[1].display_label = "Window title".to_string();
        let mut app = ConfigUiApp::new(model);

        assert_eq!(app.handle_key(ConfigUiKey::Char(' ')), ConfigUiIntent::None);
        assert_eq!(app.edit.as_ref().expect("staged bool").input, "true");
        assert_eq!(
            app.handle_key(ConfigUiKey::Enter),
            ConfigUiIntent::SetField {
                field_index: 0,
                source_id: "server".to_string(),
                path: "server.enabled".to_string(),
                value: json!(true),
            }
        );
        complete_set(
            &mut app,
            ConfigUiFieldId::new("server", "server.enabled"),
            json!(true),
        );

        app.selected_row = 1;
        assert_eq!(
            app.handle_key(ConfigUiKey::Char('e')),
            ConfigUiIntent::EditTextExternally {
                field_index: 1,
                source_id: "ui".to_string(),
                path: "ui.title".to_string(),
                input: "light".to_string(),
            }
        );
        assert_eq!(app.edit.as_ref().expect("text edit").cursor, "light".len());
        app.edit.as_mut().expect("edit").input = "temporary".to_string();
        assert_eq!(app.handle_key(ConfigUiKey::Ctrl('U')), ConfigUiIntent::None);
        assert!(app.edit.as_ref().expect("edit").input.is_empty());
        app.edit.as_mut().expect("edit").input = "dark".to_string();
        assert_eq!(
            app.handle_key(ConfigUiKey::Enter),
            ConfigUiIntent::SetField {
                field_index: 1,
                source_id: "ui".to_string(),
                path: "ui.title".to_string(),
                value: json!("dark"),
            }
        );

        complete_set(
            &mut app,
            ConfigUiFieldId::new("ui", "ui.title"),
            json!("dark"),
        );
        assert!(app.edit.is_none());
        assert_eq!(
            app.handle_key(ConfigUiKey::Char('u')),
            ConfigUiIntent::UnsetField {
                field_index: 1,
                source_id: "ui".to_string(),
                path: "ui.title".to_string(),
            }
        );
    }

    // Defends: normal Enter and Space retain their existing edit activation for non-boolean fields.
    #[test]
    fn non_boolean_fields_keep_enter_and_space_activation() {
        let mut app = ConfigUiApp::new(test_model());
        app.selected_row = 1;

        let expected = ConfigUiIntent::BeginEdit {
            field_index: 1,
            source_id: DEFAULT_CONFIG_SOURCE_ID.to_string(),
            path: "ui.theme".to_string(),
        };
        assert_eq!(app.handle_key(ConfigUiKey::Enter), expected);
        assert_eq!(app.handle_key(ConfigUiKey::Char(' ')), expected);
        assert!(app.edit.is_none());
    }

    // Defends: a host-declared theme switcher resolves the initial theme from committed model fields.
    #[test]
    fn model_theme_switcher_resolves_initial_theme() {
        let mut model = test_model();
        model.theme_switcher = Some(theme_switcher());
        assert_eq!(ConfigUiApp::new(model).active_theme, ConfigUiTheme::Light);
    }

    // Defends: theme changes are applied only after the host reports a successful write.
    #[test]
    fn successful_theme_field_save_switches_theme() {
        let mut model = test_model();
        model.fields[1] = field("ui.theme", "string", "\"dark\"", &["light", "dark"]);
        model.theme_switcher = Some(theme_switcher());
        let mut app = ConfigUiApp::new(model);
        app.selected_row = 1;

        assert_eq!(app.active_theme, ConfigUiTheme::Dark);
        app.begin_edit_field(1);
        app.edit.as_mut().expect("theme edit").choice_index = 0;
        let ConfigUiIntent::SetField {
            field_index,
            source_id,
            path,
            value,
        } = app.handle_key(ConfigUiKey::Enter)
        else {
            panic!("expected theme SetField intent");
        };
        assert_eq!(field_index, 1);
        assert_eq!(source_id, DEFAULT_CONFIG_SOURCE_ID);
        assert_eq!(path, "ui.theme");
        assert_eq!(value, json!("light"));
        assert_eq!(app.active_theme, ConfigUiTheme::Dark);

        let identity = ConfigUiFieldId::new(&source_id, &path);
        let mut reloaded = app.model.clone();
        reloaded.fields.swap(0, 1);
        set_committed_value(&mut reloaded, &identity, value);
        app.replace_model_after_success(reloaded, &identity)
            .expect("valid committed reload");

        assert_eq!(app.active_theme, ConfigUiTheme::Light);
        assert!(app.edit.is_none());
    }

    // Defends: replacement completion resolves themes only from the reloaded committed snapshot.
    #[test]
    fn successful_theme_completion_uses_reloaded_snapshot() {
        let mut model = test_model();
        model.fields[1] = field("ui.theme", "string", "\"dark\"", &["light", "dark"]);
        model.fields[1].snapshot.baseline = Some(ConfigUiResolvedValue::new(json!("light")));
        model.theme_switcher = Some(theme_switcher());
        let mut app = ConfigUiApp::new(model);
        let identity = ConfigUiFieldId::new(DEFAULT_CONFIG_SOURCE_ID, "ui.theme");

        assert_eq!(app.active_theme, ConfigUiTheme::Dark);
        complete_set(&mut app, identity.clone(), json!("light"));
        assert_eq!(app.active_theme, ConfigUiTheme::Light);

        complete_set(&mut app, identity.clone(), json!("dark"));
        assert_eq!(app.active_theme, ConfigUiTheme::Dark);

        complete_unset(&mut app, identity);
        assert_eq!(app.active_theme, ConfigUiTheme::Light);
    }

    // Defends: successful reset-to-default writes can switch a theme field without an active edit.
    #[test]
    fn successful_theme_field_unset_switches_to_default_theme() {
        let mut model = test_model();
        model.fields[1] = field("ui.theme", "string", "\"dark\"", &["light", "dark"]);
        model.fields[1].snapshot.baseline = Some(ConfigUiResolvedValue::new(json!("light")));
        model.theme_switcher = Some(theme_switcher());
        let mut app = ConfigUiApp::new(model);
        app.selected_row = 1;

        assert_eq!(app.active_theme, ConfigUiTheme::Dark);
        let ConfigUiIntent::UnsetField {
            field_index,
            source_id,
            path,
        } = app.handle_key(ConfigUiKey::Char('u'))
        else {
            panic!("expected theme UnsetField intent");
        };
        assert_eq!(field_index, 1);
        assert_eq!(source_id, DEFAULT_CONFIG_SOURCE_ID);
        assert_eq!(path, "ui.theme");
        assert_eq!(app.active_theme, ConfigUiTheme::Dark);

        let identity = ConfigUiFieldId::new(&source_id, &path);
        let mut reloaded = app.model.clone();
        reloaded.fields.swap(0, 1);
        set_unset_value(&mut reloaded, &identity);
        app.replace_model_after_success(reloaded, &identity)
            .expect("valid committed reload");

        assert_eq!(app.active_theme, ConfigUiTheme::Light);
        assert!(app.edit.is_none());
    }

    // Defends: failed validation/writeback leaves the existing theme unchanged while the edit stays staged.
    #[test]
    fn failed_theme_field_save_does_not_switch_theme() {
        let mut model = test_model();
        model.fields[1] = field("ui.theme", "string", "\"dark\"", &["light", "dark"]);
        model.theme_switcher = Some(theme_switcher());
        let mut app = ConfigUiApp::new(model);
        app.selected_row = 1;
        app.begin_edit_field(1);
        app.edit.as_mut().expect("theme edit").choice_index = 0;

        assert!(matches!(
            app.handle_key(ConfigUiKey::Enter),
            ConfigUiIntent::SetField { .. }
        ));
        app.notice_error("Host rejected the write.");

        assert_eq!(app.active_theme, ConfigUiTheme::Dark);
        assert!(app.edit.is_some());
    }

    // Defends: valid reloads preserve interaction state and rebind both selection and compatible
    // edits by stable identity rather than by the old tab or field index.
    #[test]
    fn replacement_preserves_interaction_state_across_reordering() {
        let mut model = test_model();
        model.tabs.push("appearance".to_string());
        model.fields[1].tab = "appearance".to_string();
        model.fields[0].snapshot.intent = ConfigUiOverride::Absent;
        model.fields[0].snapshot.effective = model.fields[0].snapshot.baseline.clone();
        model.core_fields = Some(vec![model.fields[1].id()]);
        let mut app = ConfigUiApp::new(model);
        app.selected_tab = 1;
        app.begin_edit_field(1);
        app.edit.as_mut().expect("edit").input = "staged".to_string();
        app.search = "theme".to_string();
        app.search_active = true;
        app.notice_error("Host reload warning.");

        let mut replacement = app.model.clone();
        replacement.tabs.swap(0, 1);
        replacement.fields.swap(0, 1);
        app.replace_model(replacement).expect("valid replacement");

        assert_eq!(app.selected_tab(), 0);
        assert_eq!(
            app.model.fields[app.selected_field_index().expect("selected field")].id(),
            ConfigUiFieldId::new(DEFAULT_CONFIG_SOURCE_ID, "ui.theme")
        );
        assert_eq!(
            app.edit().map(|edit| (&edit.field_id, edit.input.as_str())),
            Some((
                &ConfigUiFieldId::new(DEFAULT_CONFIG_SOURCE_ID, "ui.theme"),
                "staged"
            ))
        );
        assert_eq!(app.search(), "theme");
        assert!(app.search_active());
        assert_eq!(app.settings_view(), ConfigUiSettingsView::Core);
        assert_eq!(
            app.notice().map(|notice| notice.text.as_str()),
            Some("Host reload warning.")
        );
    }

    // Defends: invalid construction/replacement cannot partially mutate any editor state.
    #[test]
    fn invalid_replacement_rolls_back_the_entire_app() {
        let mut model = test_model();
        model.theme_switcher = Some(theme_switcher());
        let mut app = ConfigUiApp::new(model);
        app.selected_row = 1;
        app.begin_edit_field(1);
        app.edit.as_mut().expect("edit").input = "staged".to_string();
        app.search = "theme".to_string();
        app.search_active = true;
        app.notice_error("Host rejected a prior write.");

        let before_model = app.model.clone();
        let before_theme = app.active_theme;
        let before_tab = app.selected_tab;
        let before_row = app.selected_row;
        let before_view = app.settings_view;
        let before_search = app.search.clone();
        let before_search_active = app.search_active;
        let before_edit = app.edit.clone();
        let before_notice = app.notice.clone();
        let mut invalid = app.model.clone();
        invalid.tabs.push("general".to_string());
        let active_field = app.edit.as_ref().expect("edit").field_id.clone();

        assert!(ConfigUiApp::try_new(invalid.clone()).is_err());
        assert!(app.replace_model(invalid.clone()).is_err());
        assert!(
            app.replace_model_after_success(invalid, &active_field)
                .is_err()
        );
        assert_eq!(app.model, before_model);
        assert_eq!(app.active_theme, before_theme);
        assert_eq!(app.selected_tab, before_tab);
        assert_eq!(app.selected_row, before_row);
        assert_eq!(app.settings_view, before_view);
        assert_eq!(app.search, before_search);
        assert_eq!(app.search_active, before_search_active);
        assert_eq!(app.edit, before_edit);
        assert_eq!(app.notice, before_notice);
    }

    // Defends: ordinary reloads report buffer loss without erasing an existing host failure,
    // whether the active field disappears or its transitional editor metadata changes.
    #[test]
    fn ordinary_replacement_cancels_incompatible_edits_and_combines_notices() {
        fn assert_canceled(
            mut replacement: ConfigUiModel,
            mutate: impl FnOnce(&mut ConfigUiModel),
        ) {
            let mut app = ConfigUiApp::new(replacement.clone());
            app.begin_edit_field(1);
            app.edit.as_mut().expect("edit").input = "staged".to_string();
            app.notice_error("Host write failed.");
            mutate(&mut replacement);

            app.replace_model(replacement).expect("valid replacement");

            assert!(app.edit().is_none());
            let notice = app.notice().expect("combined cancellation notice");
            assert!(notice.is_error);
            assert!(notice.text.contains("Edit canceled after reload"));
            assert!(notice.text.contains("Host write failed."));
        }

        assert_canceled(test_model(), |model| {
            model.fields.remove(1);
        });
        assert_canceled(test_model(), |model| {
            model.fields[1].allowed_values.push("system".to_string());
        });
    }

    // Defends: a matching atomic completion clears the committed buffer before ordinary reload
    // compatibility checks, even when the host removes or changes the committed field.
    #[test]
    fn atomic_completion_avoids_false_reload_cancellation() {
        fn assert_completed(
            mut replacement: ConfigUiModel,
            mutate: impl FnOnce(&mut ConfigUiModel),
        ) {
            let mut app = ConfigUiApp::new(replacement.clone());
            let identity = app.model.fields[1].id();
            app.begin_edit_field(1);
            app.edit.as_mut().expect("edit").input = "staged".to_string();
            app.notice_error("Host kept this notice.");
            mutate(&mut replacement);

            app.replace_model_after_success(replacement, &identity)
                .expect("valid committed reload");

            assert!(app.edit().is_none());
            assert_eq!(
                app.notice().map(|notice| notice.text.as_str()),
                Some("Host kept this notice.")
            );
        }

        assert_completed(test_model(), |model| {
            model.fields.remove(1);
        });
        assert_completed(test_model(), |model| {
            model.fields[1].kind = "opaque".to_string();
            model.fields[1].allowed_values.clear();
            model.fields[1].edit_behavior = ConfigUiEditBehavior::StructuredOnly {
                notice: "Use the host editor.".to_string(),
            };
        });
    }

    // Defends: theme ownership lives at validated model ingestion, including both documented
    // replacement fallbacks rather than any value emitted by an edit intent.
    #[test]
    fn replacement_theme_resolution_handles_unmapped_and_missing_switchers() {
        let mut model = test_model();
        model.theme_switcher = Some(theme_switcher());
        let mut app = ConfigUiApp::new(model);
        assert_eq!(app.active_theme(), ConfigUiTheme::Light);

        let mut unresolved = app.model.clone();
        set_committed_value(
            &mut unresolved,
            &ConfigUiFieldId::new(DEFAULT_CONFIG_SOURCE_ID, "ui.theme"),
            json!("system"),
        );
        app.replace_model(unresolved)
            .expect("valid unmapped replacement");
        assert_eq!(app.active_theme(), ConfigUiTheme::Light);

        let mut no_switcher = app.model.clone();
        no_switcher.theme_switcher = None;
        app.replace_model(no_switcher)
            .expect("valid neutral replacement");
        assert_eq!(app.active_theme(), ConfigUiTheme::Dark);

        let mut initially_unmapped = test_model();
        initially_unmapped.theme_switcher = Some(theme_switcher());
        set_committed_value(
            &mut initially_unmapped,
            &ConfigUiFieldId::new(DEFAULT_CONFIG_SOURCE_ID, "ui.theme"),
            json!("system"),
        );
        assert_eq!(
            ConfigUiApp::new(initially_unmapped).active_theme(),
            ConfigUiTheme::Dark
        );
    }

    // Defends: standalone file-action selection follows its stable identity across both action
    // and tab reordering instead of silently activating the row that inherited its old index.
    #[test]
    fn replacement_preserves_file_action_selection_by_identity() {
        let mut model = model_with_fields(Vec::new());
        model.tabs.push("files".to_string());
        let mut first = file_action("first", "/tmp/first", true, false);
        let mut selected = file_action("selected", "/tmp/selected", true, false);
        first.tab = "files".to_string();
        selected.tab = "files".to_string();
        model.file_actions = vec![first, selected];
        let mut app = ConfigUiApp::new(model);
        app.selected_tab = 1;
        app.selected_row = 1;

        let mut replacement = app.model.clone();
        replacement.tabs.swap(0, 1);
        replacement.file_actions.swap(0, 1);
        app.replace_model(replacement).expect("valid replacement");

        assert_eq!(app.selected_tab(), 0);
        assert_eq!(app.selected_row(), 0);
        assert_eq!(
            app.handle_key(ConfigUiKey::Enter),
            open_file_intent(0, "selected", "/tmp/selected", false)
        );
    }

    // Defends: e opens free-form fields directly in the host editor, while Ctrl+e can externalize an inline staged buffer.
    #[test]
    fn text_edit_mode_emits_external_editor_intent_with_staged_input() {
        let model = model_with_fields(vec![
            field_with_source("ui", "ui.title", "string", "\"light\"", &[]),
            field_with_source("server", "server.enabled", "bool", "false", &[]),
        ]);
        let mut app = ConfigUiApp::new(model);

        assert_eq!(app.handle_key(ConfigUiKey::Ctrl('e')), ConfigUiIntent::None);

        app.begin_edit_field(1);
        assert_eq!(app.handle_key(ConfigUiKey::Ctrl('e')), ConfigUiIntent::None);
        app.handle_key(ConfigUiKey::Esc);

        assert_eq!(
            app.handle_key(ConfigUiKey::Char('e')),
            ConfigUiIntent::EditTextExternally {
                field_index: 0,
                source_id: "ui".to_string(),
                path: "ui.title".to_string(),
                input: "light".to_string(),
            }
        );
        let edit = app.edit.as_mut().expect("text edit");
        edit.input = "temporary title".to_string();
        edit.cursor = edit.input.len();
        assert_eq!(
            app.handle_key(ConfigUiKey::Ctrl('e')),
            ConfigUiIntent::EditTextExternally {
                field_index: 0,
                source_id: "ui".to_string(),
                path: "ui.title".to_string(),
                input: "temporary title".to_string(),
            }
        );
        assert!(app.edit.is_some());
    }

    // Defends: single-line editing operates on graphemes and supports insertion, deletion, and paste at the cursor.
    #[test]
    fn text_edit_keys_are_cursor_aware_and_unicode_safe() {
        let mut model = test_model();
        model.fields = vec![field("ui.title", "string", r#""a👩‍💻b""#, &[])];
        let mut app = ConfigUiApp::new(model);
        app.begin_edit_field(0);

        app.handle_key(ConfigUiKey::Home);
        app.handle_key(ConfigUiKey::Right);
        assert_eq!(app.edit.as_ref().expect("edit").cursor, 1);
        app.handle_key(ConfigUiKey::Delete);
        assert_eq!(app.edit.as_ref().expect("edit").input, "ab");

        app.handle_key(ConfigUiKey::Paste("👩‍💻".to_string()));
        let edit = app.edit.as_ref().expect("edit");
        assert_eq!(edit.input, "a👩‍💻b");
        assert_eq!(edit.cursor, 1 + "👩‍💻".len());

        app.handle_key(ConfigUiKey::Left);
        assert_eq!(app.edit.as_ref().expect("edit").cursor, 1);
        app.handle_key(ConfigUiKey::Right);
        app.handle_key(ConfigUiKey::Backspace);
        assert_eq!(app.edit.as_ref().expect("edit").input, "ab");
        app.handle_key(ConfigUiKey::Char('Z'));
        assert_eq!(app.edit.as_ref().expect("edit").input, "aZb");

        app.handle_key(ConfigUiKey::End);
        app.handle_key(ConfigUiKey::Right);
        assert_eq!(app.edit.as_ref().expect("edit").cursor, 3);
        app.handle_key(ConfigUiKey::Home);
        app.handle_key(ConfigUiKey::Left);
        assert_eq!(app.edit.as_ref().expect("edit").cursor, 0);

        app.handle_key(ConfigUiKey::Paste("two\nlines".to_string()));
        assert_eq!(app.edit.as_ref().expect("edit").input, "aZb");
        assert!(app.notice.as_ref().is_some_and(|notice| notice.is_error));
        app.notice = None;
        app.handle_key(ConfigUiKey::Char('\r'));
        assert_eq!(app.edit.as_ref().expect("edit").input, "aZb");
        assert!(app.notice.as_ref().is_some_and(|notice| notice.is_error));
    }

    // Defends: returned host-editor text updates only the active staged buffer and still saves through normal parsing.
    #[test]
    fn external_editor_text_is_staged_until_normal_save() {
        let model = model_with_fields(vec![
            field_with_source("ui", "ui.title", "string", "\"light\"", &[]),
            field_with_source("server", "server.enabled", "bool", "false", &[]),
        ]);
        let mut app = ConfigUiApp::new(model);

        assert!(app.apply_external_text_edit(0, "ignored").is_err());

        app.begin_edit_field(0);
        assert!(app.apply_external_text_edit(1, "wrong field").is_err());
        assert_eq!(app.edit.as_ref().expect("text edit").input, "light");

        app.apply_external_text_edit(0, "edited title")
            .expect("apply returned text");
        let edit = app.edit.as_ref().expect("text edit");
        assert_eq!(edit.input, "edited title");
        assert_eq!(edit.cursor, "edited title".len());
        assert_eq!(
            app.handle_key(ConfigUiKey::Enter),
            ConfigUiIntent::SetField {
                field_index: 0,
                source_id: "ui".to_string(),
                path: "ui.title".to_string(),
                value: json!("edited title"),
            }
        );
    }

    // Defends: generic TOML document rows reuse the normal structured edit intent route.
    #[test]
    fn toml_document_scalar_rows_emit_standard_set_field_intents() {
        let document = build_toml_document_fields(ConfigUiTomlDocumentSpec {
            source_id: "helix",
            tab: "native",
            section_label: "",
            current_toml: r#"
[editor]
line-number = "relative"
"#,
            default_toml: None,
            validation: "",
            rebuild_required: false,
            apply_status: after_save_status(),
        })
        .expect("toml document");
        let mut model = model_with_fields(document.fields);
        model.tabs = vec!["native".to_string()];
        model
            .tab_list_tables
            .insert("native".to_string(), document.list_table);
        let field_index = model
            .fields
            .iter()
            .position(|field| field.path == "editor.line-number")
            .expect("line number");
        let mut app = ConfigUiApp::new(model);
        app.selected_row = field_index;

        assert_eq!(
            app.handle_key(ConfigUiKey::Enter),
            ConfigUiIntent::BeginEdit {
                field_index,
                source_id: "helix".to_string(),
                path: "editor.line-number".to_string(),
            }
        );
        app.begin_edit_field(field_index);
        assert_eq!(
            app.handle_key(ConfigUiKey::Enter),
            ConfigUiIntent::SetField {
                field_index,
                source_id: "helix".to_string(),
                path: "editor.line-number".to_string(),
                value: json!("relative"),
            }
        );
    }

    // Defends: return-to-default stays on the host-owned unset intent and is unavailable without a default.
    #[test]
    fn return_to_default_requires_default_value() {
        let mut model = model_with_fields(vec![
            field_with_source("ui", "ui.theme", "string", "\"custom\"", &[]),
            field_with_source("scratch", "scratch.note", "string", "\"custom\"", &[]),
        ]);
        model.fields[1].snapshot.baseline = None;
        let mut app = ConfigUiApp::new(model);

        assert_eq!(
            app.handle_key(ConfigUiKey::Char('u')),
            ConfigUiIntent::UnsetField {
                field_index: 0,
                source_id: "ui".to_string(),
                path: "ui.theme".to_string(),
            }
        );

        app.selected_row = 1;
        assert_eq!(app.handle_key(ConfigUiKey::Char('u')), ConfigUiIntent::None);
        assert_eq!(
            app.notice.as_ref().map(|notice| notice.text.as_str()),
            Some("This setting has no default value.")
        );
    }

    // Defends: typed edit parsing and allowed-value checks stay reusable rather than Yazelix-specific.
    #[test]
    fn edit_parser_uses_field_type_and_allowed_values() {
        let bool_field = field("server.enabled", "bool", "false", &[]);
        assert_eq!(
            parse_edit_input(&bool_field, "true").expect("bool"),
            json!(true)
        );
        assert!(parse_edit_input(&bool_field, "yes").is_err());

        let enum_field = field("ui.theme", "string", "\"light\"", &["light", "dark"]);
        assert_eq!(
            parse_edit_input(&enum_field, "dark").expect("enum"),
            json!("dark")
        );
        assert!(parse_edit_input(&enum_field, "wide").is_err());

        let list_field = field(
            "plugins.enabled",
            "string_list",
            r#"["git"]"#,
            &["git", "search"],
        );
        assert_eq!(
            parse_edit_input(&list_field, r#"["git","search"]"#).expect("list"),
            json!(["git", "search"])
        );
        assert_eq!(
            parse_edit_input(&list_field, r#"["search","git"]"#).expect("ordered list"),
            json!(["search", "git"])
        );
        assert!(parse_edit_input(&list_field, r#"["unknown"]"#).is_err());

        let mut friendly_list_field = list_field.clone();
        friendly_list_field.edit_behavior = ConfigUiEditBehavior::FriendlyStringList;
        assert_eq!(
            parse_edit_input(&friendly_list_field, "search, git").expect("friendly list"),
            json!(["search", "git"])
        );
        assert!(parse_edit_input(&friendly_list_field, "search, unknown").is_err());
    }

    // Defends: edit initialization follows sparse state rather than confusing invalid input with
    // display text, while bools and scalar enums keep their native choice modes.
    #[test]
    fn edit_helpers_use_choice_modes_for_bool_and_enum() {
        let bool_field = field("server.enabled", "bool", "true", &[]);
        assert_eq!(field_bool_value(&bool_field), Some(true));
        assert_eq!(edit_mode_for_field(&bool_field), ConfigUiEditMode::Choice);

        let enum_field = field("ui.theme", "string", "\"light\"", &["light", "dark"]);
        assert_eq!(edit_input_for_field(&enum_field), "light");
        assert_eq!(edit_mode_for_field(&enum_field), ConfigUiEditMode::Choice);

        let mut invalid_field = field("server.port", "int", "80", &[]);
        invalid_field.snapshot.intent = ConfigUiOverride::Invalid {
            input: "not set".to_string(),
        };
        assert_eq!(edit_input_for_field(&invalid_field), "not set");
    }

    // Defends: default string-list multiselect remains set-like and canonicalizes selected values to allowed-value order.
    #[test]
    fn default_string_list_multiselect_keeps_allowed_value_order() {
        let field = field(
            "widgets.enabled",
            "string_list",
            r#"["status"]"#,
            &["clock", "status", "mode"],
        );

        assert_eq!(
            toggled_string_list_input(&field, r#"["status"]"#, 0).expect("toggle clock"),
            r#"["clock","status"]"#
        );
    }

    // Defends: ordered string-list editing is opt-in and preserves config order when toggling selected ids.
    #[test]
    fn ordered_string_list_multiselect_preserves_order_when_toggling() {
        let mut field = field(
            "widgets.enabled",
            "string_list",
            r#"["status","clock"]"#,
            &["clock", "status", "mode"],
        );
        field.edit_behavior = ConfigUiEditBehavior::OrderedStringList;

        assert!(is_ordered_string_list_field(&field));
        assert_eq!(edit_mode_for_field(&field), ConfigUiEditMode::MultiChoice);
        assert_eq!(
            toggled_string_list_input(&field, r#"["status","clock"]"#, 2).expect("toggle mode"),
            r#"["status","clock","mode"]"#
        );
        assert_eq!(
            toggled_string_list_input(&field, r#"["status","clock","mode"]"#, 1)
                .expect("remove clock"),
            r#"["status","mode"]"#
        );
    }

    // Defends: ordered string-list fields can move enabled ids without changing default multiselect semantics.
    #[test]
    fn ordered_string_list_reducer_reorders_enabled_values() {
        let mut model = test_model();
        model.fields = vec![field(
            "widgets.enabled",
            "string_list",
            r#"["status","clock"]"#,
            &["clock", "status", "mode"],
        )];
        model.fields[0].edit_behavior = ConfigUiEditBehavior::OrderedStringList;
        let mut app = ConfigUiApp::new(model);

        app.begin_edit_field(0);
        assert_eq!(app.edit.as_ref().expect("ordered edit").choice_index, 0);
        assert_eq!(app.handle_key(ConfigUiKey::Char('J')), ConfigUiIntent::None);
        assert_eq!(
            app.edit.as_ref().expect("ordered edit").input,
            r#"["clock","status"]"#
        );
        assert_eq!(app.edit.as_ref().expect("ordered edit").choice_index, 1);
        assert_eq!(app.handle_key(ConfigUiKey::Char('K')), ConfigUiIntent::None);
        assert_eq!(app.edit.as_ref().expect("ordered edit").choice_index, 0);
        assert_eq!(
            app.handle_key(ConfigUiKey::Enter),
            ConfigUiIntent::SetField {
                field_index: 0,
                source_id: DEFAULT_CONFIG_SOURCE_ID.to_string(),
                path: "widgets.enabled".to_string(),
                value: json!(["status", "clock"]),
            }
        );
    }

    // Defends: search input, cancellation, and row clamping live in the reusable reducer.
    #[test]
    fn reducer_updates_search_state() {
        let mut app = ConfigUiApp::new(test_model());

        assert_eq!(app.handle_key(ConfigUiKey::Char('/')), ConfigUiIntent::None);
        assert!(app.search_active);
        for ch in "theme".chars() {
            assert_eq!(app.handle_key(ConfigUiKey::Char(ch)), ConfigUiIntent::None);
        }
        assert_eq!(app.search, "theme");
        assert_eq!(app.visible_rows(), vec![UiRowRef::Field(1)]);
        assert_eq!(app.selected_row, 0);
        assert_eq!(app.handle_key(ConfigUiKey::Backspace), ConfigUiIntent::None);
        assert_eq!(app.search, "them");
        assert_eq!(app.handle_key(ConfigUiKey::Ctrl('u')), ConfigUiIntent::None);
        assert!(app.search.is_empty());
        assert_eq!(
            app.handle_key(ConfigUiKey::Paste("👩‍💻".to_string())),
            ConfigUiIntent::None
        );
        assert_eq!(app.search, "👩‍💻");
        assert_eq!(app.handle_key(ConfigUiKey::Backspace), ConfigUiIntent::None);
        assert!(app.search.is_empty());
        assert_eq!(
            app.handle_key(ConfigUiKey::Char('\n')),
            ConfigUiIntent::None
        );
        assert!(app.search.is_empty());
        for ch in "theme".chars() {
            assert_eq!(app.handle_key(ConfigUiKey::Char(ch)), ConfigUiIntent::None);
        }
        assert_eq!(app.handle_key(ConfigUiKey::Ctrl('U')), ConfigUiIntent::None);
        assert!(app.search.is_empty());
        assert_eq!(app.handle_key(ConfigUiKey::Enter), ConfigUiIntent::None);
        assert!(!app.search_active);
    }

    // Defends: Core is the focused default, search widens without changing it, and toggles preserve a surviving selection.
    #[test]
    fn reducer_controls_core_all_views_without_hidden_selection_or_search_state_leaks() {
        let mut core = field("core.visible", "string", r#""core""#, &[]);
        crate::model::set_field_state_for_test(&mut core, ConfigUiFieldState::Inherited);
        let mut hidden = field("advanced.hidden", "string", r#""hidden""#, &[]);
        crate::model::set_field_state_for_test(&mut hidden, ConfigUiFieldState::Inherited);
        let explicit = field("advanced.explicit", "string", r#""set""#, &[]);
        let mut other_tab = field("other.visible", "string", r#""other""#, &[]);
        crate::model::set_field_state_for_test(&mut other_tab, ConfigUiFieldState::Inherited);
        other_tab.tab = "other".to_string();
        let mut model = model_with_fields(vec![core, hidden, explicit, other_tab]);
        model.tabs.push("other".to_string());
        model.core_fields = Some(vec![
            ConfigUiFieldId::new(DEFAULT_CONFIG_SOURCE_ID, "core.visible"),
            ConfigUiFieldId::new(DEFAULT_CONFIG_SOURCE_ID, "other.visible"),
        ]);
        let mut app = ConfigUiApp::new(model);

        assert_eq!(app.settings_view, ConfigUiSettingsView::Core);
        assert_eq!(
            app.visible_rows(),
            vec![UiRowRef::Field(0), UiRowRef::Field(2)]
        );

        assert_eq!(app.handle_key(ConfigUiKey::Char('a')), ConfigUiIntent::None);
        assert_eq!(app.settings_view, ConfigUiSettingsView::All);
        app.selected_row = 1;
        assert_eq!(
            app.selected_field().map(|field| field.path.as_str()),
            Some("advanced.hidden")
        );

        app.handle_key(ConfigUiKey::Char('a'));
        assert_eq!(app.settings_view, ConfigUiSettingsView::Core);
        assert_eq!(
            app.selected_field().map(|field| field.path.as_str()),
            Some("advanced.explicit")
        );
        app.handle_key(ConfigUiKey::Char('a'));
        assert_eq!(app.selected_row, 2);

        app.handle_key(ConfigUiKey::Char('a'));
        app.handle_key(ConfigUiKey::Char('/'));
        app.handle_key(ConfigUiKey::Char('a'));
        assert_eq!(app.search, "a");
        assert_eq!(app.settings_view, ConfigUiSettingsView::Core);
        app.handle_key(ConfigUiKey::Ctrl('u'));
        for ch in "advanced.hidden".chars() {
            app.handle_key(ConfigUiKey::Char(ch));
        }
        assert_eq!(app.visible_rows(), vec![UiRowRef::Field(1)]);
        app.handle_key(ConfigUiKey::Enter);
        app.handle_key(ConfigUiKey::Char('a'));
        assert_eq!(app.settings_view, ConfigUiSettingsView::Core);
        app.handle_key(ConfigUiKey::Esc);
        assert!(app.search.is_empty());
        assert_eq!(
            app.visible_rows(),
            vec![UiRowRef::Field(0), UiRowRef::Field(2)]
        );

        app.next_tab();
        assert!(!app.selected_tab_has_non_core_fields());
        app.handle_key(ConfigUiKey::Char('a'));
        assert_eq!(app.settings_view, ConfigUiSettingsView::Core);
    }

    // Defends: normal-mode digits select the matching first-nine tab and reset row selection without changing out-of-range state.
    #[test]
    fn reducer_selects_numbered_tabs_directly() {
        let mut model = test_model();
        model.tabs = (1..=10).map(|index| format!("tab_{index}")).collect();
        model.tabs[0] = "general".to_string();
        let mut app = ConfigUiApp::new(model);
        app.selected_row = 2;

        assert_eq!(app.handle_key(ConfigUiKey::Char('2')), ConfigUiIntent::None);
        assert_eq!((app.selected_tab, app.selected_row), (1, 0));

        app.selected_row = 2;
        assert_eq!(app.handle_key(ConfigUiKey::Char('9')), ConfigUiIntent::None);
        assert_eq!((app.selected_tab, app.selected_row), (8, 0));

        app.model.tabs.truncate(3);
        app.selected_tab = 0;
        app.selected_row = 2;
        assert_eq!(app.handle_key(ConfigUiKey::Char('9')), ConfigUiIntent::None);
        assert_eq!((app.selected_tab, app.selected_row), (0, 2));
        assert_eq!(app.handle_key(ConfigUiKey::Char('0')), ConfigUiIntent::None);
        assert_eq!((app.selected_tab, app.selected_row), (0, 2));
    }

    // Regression: digit shortcuts remain ordinary text while search or scalar text editing is active.
    #[test]
    fn numbered_tab_digits_remain_search_and_edit_input() {
        let mut model = model_with_fields(vec![field("ui.scale", "integer", "1", &[])]);
        model.tabs = vec!["general".to_string(), "advanced".to_string()];
        let mut app = ConfigUiApp::new(model);

        app.search_active = true;
        assert_eq!(app.handle_key(ConfigUiKey::Char('2')), ConfigUiIntent::None);
        assert_eq!(app.search, "2");
        assert_eq!(app.selected_tab, 0);

        app.search_active = false;
        app.search.clear();
        app.begin_edit_field(0);
        assert_eq!(app.handle_key(ConfigUiKey::Char('2')), ConfigUiIntent::None);
        assert_eq!(app.handle_key(ConfigUiKey::Char('a')), ConfigUiIntent::None);
        assert_eq!(app.edit.as_ref().expect("text edit").input, "12a");
        assert_eq!(app.selected_tab, 0);
    }

    // Defends: vertical row navigation wraps within the visible rows and stays stable for empty views.
    #[test]
    fn reducer_wraps_vertical_row_navigation() {
        let mut app = ConfigUiApp::new(test_model());
        let last_row = app.visible_rows().len() - 1;

        app.move_up();
        assert_eq!(app.selected_row, last_row);
        app.move_down();
        assert_eq!(app.selected_row, 0);

        app.selected_row = last_row;
        app.move_down();
        assert_eq!(app.selected_row, 0);
        app.move_up();
        assert_eq!(app.selected_row, last_row);

        app.search = "no matching rows".to_string();
        app.move_down();
        assert_eq!(app.selected_row, 0);
        app.move_up();
        assert_eq!(app.selected_row, 0);
    }

    // Defends: section headings remain render-only and cannot alter keyboard selection or edit-intent routing.
    #[test]
    fn section_labels_do_not_enter_editor_row_navigation() {
        let mut model = test_model();
        model.fields[0].section_label = "Runtime".to_string();
        model.fields[1].section_label = "Appearance".to_string();
        model.fields[2].section_label = "Extensions".to_string();
        let mut app = ConfigUiApp::new(model);

        assert_eq!(app.visible_rows().len(), 3);
        assert_eq!(app.handle_key(ConfigUiKey::Char('j')), ConfigUiIntent::None);
        assert_eq!(app.selected_row, 1);
        assert_eq!(
            app.handle_key(ConfigUiKey::Char('e')),
            ConfigUiIntent::BeginEdit {
                field_index: 1,
                source_id: DEFAULT_CONFIG_SOURCE_ID.to_string(),
                path: "ui.theme".to_string(),
            }
        );
    }

    // Defends: single-select and multiselect edit keys are generic reducer behavior.
    #[test]
    fn reducer_drives_single_select_and_multiselect_edits() {
        let mut app = ConfigUiApp::new(test_model());

        app.begin_edit_field(1);
        assert_eq!(app.edit.as_ref().expect("single edit").choice_index, 0);
        assert_eq!(app.handle_key(ConfigUiKey::Char('j')), ConfigUiIntent::None);
        assert_eq!(app.edit.as_ref().expect("single edit").choice_index, 1);
        assert_eq!(app.handle_key(ConfigUiKey::Char('h')), ConfigUiIntent::None);
        assert_eq!(app.edit.as_ref().expect("single edit").choice_index, 0);
        assert_eq!(app.handle_key(ConfigUiKey::Char('l')), ConfigUiIntent::None);
        assert_eq!(app.edit.as_ref().expect("single edit").choice_index, 1);
        assert_eq!(app.handle_key(ConfigUiKey::Char(' ')), ConfigUiIntent::None);
        assert_eq!(app.edit.as_ref().expect("single edit").input, "dark");
        assert_eq!(
            app.handle_key(ConfigUiKey::Enter),
            ConfigUiIntent::SetField {
                field_index: 1,
                source_id: DEFAULT_CONFIG_SOURCE_ID.to_string(),
                path: "ui.theme".to_string(),
                value: json!("dark"),
            }
        );

        app.begin_edit_field(2);
        assert_eq!(app.edit.as_ref().expect("multi edit").choice_index, 0);
        assert_eq!(app.handle_key(ConfigUiKey::Char('j')), ConfigUiIntent::None);
        assert_eq!(app.edit.as_ref().expect("multi edit").choice_index, 1);
        assert_eq!(app.handle_key(ConfigUiKey::Char(' ')), ConfigUiIntent::None);
        assert_eq!(
            app.handle_key(ConfigUiKey::Enter),
            ConfigUiIntent::SetField {
                field_index: 2,
                source_id: DEFAULT_CONFIG_SOURCE_ID.to_string(),
                path: "plugins.enabled".to_string(),
                value: json!(["git", "search"]),
            }
        );
    }

    fn test_model() -> ConfigUiModel {
        model_with_fields(vec![
            field("server.enabled", "bool", "false", &[]),
            field("ui.theme", "string", "\"light\"", &["light", "dark"]),
            field(
                "plugins.enabled",
                "string_list",
                r#"["git"]"#,
                &["git", "search"],
            ),
        ])
    }

    fn complete_set(app: &mut ConfigUiApp, identity: ConfigUiFieldId, value: JsonValue) {
        let mut model = app.model.clone();
        set_committed_value(&mut model, &identity, value);
        app.replace_model_after_success(model, &identity)
            .expect("valid committed reload");
    }

    fn set_committed_value(
        model: &mut ConfigUiModel,
        identity: &ConfigUiFieldId,
        value: JsonValue,
    ) {
        let field = model
            .fields
            .iter_mut()
            .find(|field| field.matches_id(identity))
            .expect("field to commit");
        field.snapshot.intent = ConfigUiOverride::Explicit(value.clone());
        field.snapshot.effective = Some(ConfigUiResolvedValue::new(value));
    }

    fn complete_unset(app: &mut ConfigUiApp, identity: ConfigUiFieldId) {
        let mut model = app.model.clone();
        set_unset_value(&mut model, &identity);
        app.replace_model_after_success(model, &identity)
            .expect("valid committed reload");
    }

    fn set_unset_value(model: &mut ConfigUiModel, identity: &ConfigUiFieldId) {
        let field = model
            .fields
            .iter_mut()
            .find(|field| field.matches_id(identity))
            .expect("field to unset");
        field.snapshot.intent = ConfigUiOverride::Absent;
        field.snapshot.effective = field.snapshot.baseline.clone();
    }

    fn theme_switcher() -> ConfigUiThemeSwitcher {
        ConfigUiThemeSwitcher {
            field: ConfigUiFieldId::new(DEFAULT_CONFIG_SOURCE_ID, "ui.theme"),
            mappings: vec![
                ConfigUiThemeMapping {
                    value: json!("dark"),
                    theme: ConfigUiTheme::Dark,
                },
                ConfigUiThemeMapping {
                    value: json!("light"),
                    theme: ConfigUiTheme::Light,
                },
            ],
        }
    }

    fn file_action(
        action_id: &str,
        path: &str,
        exists: bool,
        create_if_missing: bool,
    ) -> ConfigUiFileAction {
        ConfigUiFileAction {
            source_id: "native".to_string(),
            action_id: action_id.to_string(),
            tab: "general".to_string(),
            label: format!("{action_id} config"),
            description: "Host-owned native config file".to_string(),
            path: PathBuf::from(path),
            exists,
            read_only: false,
            create_if_missing,
            disabled_reason: None,
        }
    }

    fn open_file_intent(
        file_action_index: usize,
        action_id: &str,
        path: &str,
        create_if_missing: bool,
    ) -> ConfigUiIntent {
        ConfigUiIntent::OpenFile {
            file_action_index,
            source_id: "native".to_string(),
            action_id: action_id.to_string(),
            path: PathBuf::from(path),
            create_if_missing,
        }
    }

    // Defends: e on a structured field opens only its uniquely matching host-owned source file.
    #[test]
    fn structured_field_edit_opens_unique_source_file_action() {
        let mut structured = field_with_source("native", "editor.rulers", "bool", "true", &[]);
        structured.edit_behavior = ConfigUiEditBehavior::StructuredOnly {
            notice: "Edit the source file directly.".to_string(),
        };
        let mut model = model_with_fields(vec![structured]);
        model.file_actions = vec![file_action("settings", "/tmp/settings", true, true)];
        let mut app = ConfigUiApp::new(model);

        assert_eq!(
            app.handle_key(ConfigUiKey::Char('e')),
            open_file_intent(0, "settings", "/tmp/settings", false)
        );
        for key in [ConfigUiKey::Enter, ConfigUiKey::Char('u')] {
            assert_eq!(app.handle_key(key), ConfigUiIntent::None);
            assert_eq!(
                app.notice.as_ref().map(|notice| notice.text.as_str()),
                Some("Edit the source file directly.")
            );
        }

        app.model
            .file_actions
            .push(file_action("other", "/tmp/other", true, true));
        assert!(matches!(
            app.handle_key(ConfigUiKey::Char('e')),
            ConfigUiIntent::BeginEdit { .. }
        ));
    }

    // Defends: file action rows emit stable host-owned open intents for existing and missing files.
    #[test]
    fn file_action_rows_emit_open_file_intents() {
        let mut model = test_model();
        model.fields.clear();
        model.file_actions = vec![
            file_action("existing", "/tmp/acme/existing.toml", true, true),
            file_action("missing", "/tmp/acme/missing.toml", false, true),
        ];
        let mut app = ConfigUiApp::new(model);

        assert_eq!(
            app.handle_key(ConfigUiKey::Enter),
            open_file_intent(0, "existing", "/tmp/acme/existing.toml", false)
        );

        app.selected_row = 1;
        assert_eq!(
            app.handle_key(ConfigUiKey::Char(' ')),
            open_file_intent(1, "missing", "/tmp/acme/missing.toml", true)
        );
    }

    // Defends: disabled file action rows render as actions but do not enter scalar edit flow.
    #[test]
    fn disabled_file_action_rows_do_not_emit_edit_or_open_intents() {
        let mut model = test_model();
        model.fields.clear();
        let mut action = file_action("broken", "/tmp/acme/broken.toml", false, true);
        action.disabled_reason = Some("Native config path is unavailable.".to_string());
        model.file_actions = vec![action];
        let mut app = ConfigUiApp::new(model);

        assert_eq!(app.handle_key(ConfigUiKey::Enter), ConfigUiIntent::None);
        assert_eq!(app.handle_key(ConfigUiKey::Char('e')), ConfigUiIntent::None);
        assert!(app.edit.is_none());
        assert_eq!(
            app.notice.as_ref().map(|notice| notice.text.as_str()),
            Some("Native config path is unavailable.")
        );
    }
}
