// Test lane: default

use super::{
    ConfigUiCapability, ConfigUiChoice, ConfigUiDiagnostic, ConfigUiField, ConfigUiFieldId,
    ConfigUiFileAction, ConfigUiModel, ConfigUiNativeStatus, ConfigUiOverride,
    ConfigUiSettingsView, ConfigUiSidecar, ConfigUiTextEncoding, ConfigUiTheme, UiRowRef,
};
use crate::model::{
    config_ui_theme_for_model, field_counts_for_tab, render_json_edit_value,
    visible_rows_for_tab_search_in_view,
};
use serde_json::Value as JsonValue;
use std::path::PathBuf;
use unicode_segmentation::UnicodeSegmentation;

pub struct ConfigUiApp {
    pub(crate) model: ConfigUiModel,
    pub(crate) active_theme: ConfigUiTheme,
    pub(crate) selected_tab: usize,
    pub(crate) selected_row: usize,
    /// Current view outside active search.
    ///
    /// [`ConfigUiApp::try_new`] selects Overview when the model contains an Overview/All distinction. Models
    /// whose views contain the same fields start in All. Normal-mode `a` toggles the view when the
    /// selected tab has non-Overview fields. Search spans All without changing this saved view.
    pub(crate) settings_view: ConfigUiSettingsView,
    pub(crate) search: String,
    pub(crate) search_active: bool,
    pub(crate) edit: Option<ConfigUiEditState>,
    pub(crate) notice: Option<ConfigUiNotice>,
    pub(crate) shortcut_help_scroll: Option<usize>,
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
    OpenFile {
        source_id: String,
        action_id: String,
        path: PathBuf,
        create_if_missing: bool,
    },
    EditTextExternally {
        field: ConfigUiFieldId,
        input: String,
    },
    SetField {
        field: ConfigUiFieldId,
        value: JsonValue,
    },
    UnsetField {
        field: ConfigUiFieldId,
    },
}

/// Validated borrowed data for a renderer-provided row reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigUiRow<'a> {
    Field(&'a ConfigUiField),
    FileAction(&'a ConfigUiFileAction),
    Sidecar(&'a ConfigUiSidecar),
    NativeStatus(&'a ConfigUiNativeStatus),
    Diagnostic(&'a ConfigUiDiagnostic),
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
    /// Models with `recommended_fields: None` start in All because every field belongs to Overview.
    pub fn try_new(model: ConfigUiModel) -> Result<Self, String> {
        model.validate()?;
        let active_theme = config_ui_theme_for_model(&model, ConfigUiTheme::Dark);
        let settings_view = if model_has_non_overview_fields(&model) {
            ConfigUiSettingsView::Overview
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
            shortcut_help_scroll: None,
        })
    }

    #[cfg(test)]
    pub(crate) fn new(model: ConfigUiModel) -> Self {
        Self::try_new(model).expect("test model must be valid")
    }

    pub fn model(&self) -> &ConfigUiModel {
        &self.model
    }

    /// Resolves a renderer-provided row without requiring unchecked model indexing.
    ///
    /// Out-of-range references return `None`. Row references are ephemeral and should be
    /// resolved when received rather than retained across model replacement or reordering.
    pub fn resolve_row(&self, row: UiRowRef) -> Option<ConfigUiRow<'_>> {
        match row {
            UiRowRef::Field(index) => self.model.fields.get(index).map(ConfigUiRow::Field),
            UiRowRef::FileAction(index) => self
                .model
                .file_actions
                .get(index)
                .map(ConfigUiRow::FileAction),
            UiRowRef::Sidecar(index) => self.model.sidecars.get(index).map(ConfigUiRow::Sidecar),
            UiRowRef::NativeStatus(index) => self
                .model
                .native_config_statuses
                .get(index)
                .map(ConfigUiRow::NativeStatus),
            UiRowRef::Diagnostic(index) => self
                .model
                .diagnostics
                .get(index)
                .map(ConfigUiRow::Diagnostic),
        }
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
        let canceled_edit = edit.as_mut().is_some_and(|active| {
            let old = self
                .model
                .fields
                .iter()
                .find(|field| field.matches_id(&active.field_id));
            let new = model
                .fields
                .iter()
                .find(|field| field.matches_id(&active.field_id));
            !matches!((old, new), (Some(old), Some(new)) if reconcile_replacement_edit(old, new, active))
        });
        if canceled_edit {
            edit = None;
        }

        let active_theme = config_ui_theme_for_model(&model, self.active_theme);
        let settings_view = if self.settings_view == ConfigUiSettingsView::Overview
            && !model_has_non_overview_fields(&model)
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

    pub(crate) fn active_edit_field(&self) -> Option<&ConfigUiField> {
        let identity = &self.edit.as_ref()?.field_id;
        self.model
            .fields
            .iter()
            .find(|field| field.matches_id(identity))
    }

    fn row_id(&self, row: UiRowRef) -> Option<ConfigUiRowId> {
        match self.resolve_row(row)? {
            ConfigUiRow::Field(field) => Some(ConfigUiRowId::Field(field.id())),
            ConfigUiRow::FileAction(action) => Some(ConfigUiRowId::FileAction {
                source_id: action.source_id.clone(),
                action_id: action.action_id.clone(),
            }),
            ConfigUiRow::Sidecar(_) | ConfigUiRow::NativeStatus(_) | ConfigUiRow::Diagnostic(_) => {
                None
            }
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

    pub(crate) fn selected_tab_has_non_overview_fields(&self) -> bool {
        let counts = field_counts_for_tab(&self.model, self.selected_tab);
        counts.overview < counts.total
    }

    pub(crate) fn can_toggle_settings_view(&self) -> bool {
        !self.search_active && self.search.is_empty() && self.selected_tab_has_non_overview_fields()
    }

    fn toggle_settings_view(&mut self) {
        if !self.can_toggle_settings_view() {
            return;
        }
        let selected = self.visible_rows().get(self.selected_row).copied();
        self.settings_view = match self.settings_view {
            ConfigUiSettingsView::Overview => ConfigUiSettingsView::All,
            ConfigUiSettingsView::All => ConfigUiSettingsView::Overview,
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

    pub(crate) fn selected_field_index(&self) -> Option<usize> {
        let row = self.visible_rows().get(self.selected_row).copied()?;
        match row {
            UiRowRef::Field(index) => Some(index),
            _ => None,
        }
    }

    pub fn selected_field(&self) -> Option<&ConfigUiField> {
        self.model.fields.get(self.selected_field_index()?)
    }

    /// Reports whether normal-mode input may remove this field's override.
    pub(crate) fn can_unset_field(&self, field: &ConfigUiField) -> bool {
        !self.search_active
            && self.edit.is_none()
            && field.can_unset
            && matches!(
                field.snapshot.intent,
                ConfigUiOverride::Explicit(_) | ConfigUiOverride::Invalid { .. }
            )
            && self
                .model
                .sources
                .iter()
                .any(|source| source.id == field.source_id && !source.read_only)
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

    pub(crate) fn selected_capability_file_action(&self) -> Option<(usize, &ConfigUiFileAction)> {
        let field = self.selected_field()?;
        let ConfigUiCapability::ReadOnly {
            file_action_id: Some(action_id),
            ..
        } = &field.capability
        else {
            return None;
        };
        self.model
            .file_actions
            .iter()
            .enumerate()
            .find(|(_, action)| {
                action.source_id == field.source_id && action.action_id == *action_id
            })
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
        if self.shortcut_help_scroll.is_some() {
            self.handle_shortcut_help_key(key);
            return ConfigUiIntent::None;
        }
        if self.edit.is_some() {
            return self.handle_edit_key(key);
        }
        if self.search_active {
            self.handle_search_key(key);
            return ConfigUiIntent::None;
        }
        self.handle_normal_key(key)
    }

    fn handle_shortcut_help_key(&mut self, key: ConfigUiKey) {
        self.shortcut_help_scroll = match key {
            ConfigUiKey::Esc | ConfigUiKey::Char('?') => None,
            ConfigUiKey::Down | ConfigUiKey::Char('j') => self
                .shortcut_help_scroll
                .map(|scroll| scroll.saturating_add(1)),
            ConfigUiKey::Up | ConfigUiKey::Char('k') => self
                .shortcut_help_scroll
                .map(|scroll| scroll.saturating_sub(1)),
            _ => self.shortcut_help_scroll,
        };
    }

    fn begin_edit_field(&mut self, field_index: usize) {
        self.notice = None;
        let Some(field) = self.model.fields.get(field_index) else {
            self.notice_error("Only settings rows can be edited.");
            return;
        };
        match edit_state_for_field(field) {
            Ok(edit) => self.edit = Some(edit),
            Err(message) => self.notice_info(message),
        }
    }

    pub fn apply_external_text_edit(
        &mut self,
        field: &ConfigUiFieldId,
        input: impl Into<String>,
    ) -> Result<(), String> {
        let Some(edit) = &mut self.edit else {
            return Err("No text edit is active.".to_string());
        };
        if field != &edit.field_id {
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
            ConfigUiKey::Char('?') => {
                self.shortcut_help_scroll = Some(0);
                ConfigUiIntent::None
            }
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
                    matches!(field.capability, ConfigUiCapability::Toggle { .. })
                }) =>
            {
                self.notice_info("Press Space to stage this change, then Enter to save.");
                ConfigUiIntent::None
            }
            ConfigUiKey::Enter | ConfigUiKey::Char(' ') => self.activate_selected_row(),
            ConfigUiKey::Char('e') => self.edit_or_activate_selected_row(),
            ConfigUiKey::Char('u') => self.unset_selected_field(),
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
        let choice_picker = field.is_some_and(is_choice_picker);
        let ordered_multi_choice = field.is_some_and(is_ordered_multi_choice_field);
        let multi_choice = mode == ConfigUiEditMode::MultiChoice;
        match key {
            ConfigUiKey::Esc => self.cancel_edit(),
            ConfigUiKey::Enter if choice_picker => {
                self.select_single_choice_edit();
                self.save_edit()
            }
            ConfigUiKey::Enter => self.save_edit(),
            ConfigUiKey::Char('K') if ordered_multi_choice => {
                self.notice = None;
                self.move_ordered_multi_choice_edit(-1);
                ConfigUiIntent::None
            }
            ConfigUiKey::Char('J') if ordered_multi_choice => {
                self.notice = None;
                self.move_ordered_multi_choice_edit(1);
                ConfigUiIntent::None
            }
            ConfigUiKey::Up | ConfigUiKey::Left | ConfigUiKey::Char('k' | 'h')
                if choice_picker || multi_choice =>
            {
                self.notice = None;
                self.move_choice_edit(-1);
                ConfigUiIntent::None
            }
            ConfigUiKey::Down | ConfigUiKey::Right | ConfigUiKey::Char('j' | 'l')
                if choice_picker || multi_choice =>
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
            ConfigUiKey::Char(' ') if choice_picker => {
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
                self.cycle_toggle_edit();
                ConfigUiIntent::None
            }
            _ => ConfigUiIntent::None,
        }
    }

    fn cycle_toggle_edit(&mut self) {
        self.replace_choice_input(|field, edit| toggled_choice_input(field, &edit.input));
    }

    fn move_choice_edit(&mut self, delta: isize) {
        let len = self
            .active_edit_field()
            .map_or(0, |field| capability_choices(field).len());
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
            self.edit.as_ref().and_then(|edit| {
                edit_choices(field, &edit.input)
                    .get(edit.choice_index)
                    .copied()
            })
        }) else {
            return;
        };
        let value = render_json_edit_value(&value.value);
        let Some(edit) = &mut self.edit else {
            return;
        };
        edit.input = value;
        edit.cursor = edit.input.len();
    }

    fn toggle_multi_choice_edit(&mut self) {
        self.replace_choice_input(|field, edit| {
            toggled_multi_choice_input(field, &edit.input, edit.choice_index)
        });
    }

    fn move_ordered_multi_choice_edit(&mut self, delta: isize) {
        self.replace_choice_input(|field, edit| {
            moved_ordered_multi_choice_input(field, &edit.input, edit.choice_index, delta)
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
        let selected = if is_ordered_multi_choice_field(&field) {
            edit_choice_value(&field, &edit.input, edit.choice_index).ok()
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
                && let Some(index) = edit_choice_index(&field, &edit.input, &value)
            {
                edit.choice_index = index;
            }
        }
    }

    fn edit_or_activate_selected_row(&mut self) -> ConfigUiIntent {
        if let Some((index, _)) = self.selected_file_action() {
            return self.activate_file_action(index);
        }
        if let Some((index, _)) = self.selected_capability_file_action() {
            return self.activate_file_action(index);
        }
        self.notice = None;
        let Some(field_index) = self.selected_field_index() else {
            self.notice_error("Only settings rows can be edited.");
            return ConfigUiIntent::None;
        };
        let external = matches!(
            self.model.fields[field_index].capability,
            ConfigUiCapability::FreeText { .. }
        );
        self.begin_edit_field(field_index);
        if external {
            self.edit_text_externally()
        } else {
            ConfigUiIntent::None
        }
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
        let input = edit.input.clone();
        let Some(field) = self.active_edit_field().map(ConfigUiField::id) else {
            self.notice_error("Active edit field is unavailable.");
            return ConfigUiIntent::None;
        };
        self.notice = None;
        ConfigUiIntent::EditTextExternally { field, input }
    }

    fn quick_edit_selected_field(&mut self) -> ConfigUiIntent {
        self.notice = None;
        let Some(field_index) = self.selected_field_index() else {
            self.notice_error("Only settings rows can be edited.");
            return ConfigUiIntent::None;
        };
        let toggle = matches!(
            self.model.fields[field_index].capability,
            ConfigUiCapability::Toggle { .. }
        );
        self.begin_edit_field(field_index);
        if toggle {
            self.cycle_toggle_edit();
        }
        ConfigUiIntent::None
    }

    fn unset_selected_field(&mut self) -> ConfigUiIntent {
        self.notice = None;
        let Some(field) = self
            .selected_field()
            .filter(|field| self.can_unset_field(field))
            .map(ConfigUiField::id)
        else {
            self.notice_info("This row has no removable override.");
            return ConfigUiIntent::None;
        };
        ConfigUiIntent::UnsetField { field }
    }

    fn save_edit(&mut self) -> ConfigUiIntent {
        let Some(edit) = self.edit.as_ref() else {
            return ConfigUiIntent::None;
        };
        let Some(field) = self.active_edit_field() else {
            self.notice_error("Active edit field is unavailable.");
            return ConfigUiIntent::None;
        };
        let value = match parse_edit_input(field, &edit.input) {
            Ok(value) => value,
            Err(message) => {
                self.notice_error(message);
                return ConfigUiIntent::None;
            }
        };
        ConfigUiIntent::SetField {
            field: field.id(),
            value,
        }
    }
}

fn model_has_non_overview_fields(model: &ConfigUiModel) -> bool {
    (0..model.tabs.len()).any(|tab| {
        let counts = field_counts_for_tab(model, tab);
        counts.overview < counts.total
    })
}

fn edit_is_compatible(old: &ConfigUiField, new: &ConfigUiField, edit: &ConfigUiEditState) -> bool {
    match (&old.capability, &new.capability) {
        (
            ConfigUiCapability::FreeText { encoding: old },
            ConfigUiCapability::FreeText { encoding: new },
        ) => old == new,
        (ConfigUiCapability::Toggle { .. }, ConfigUiCapability::Toggle { .. })
        | (ConfigUiCapability::Choice { .. }, ConfigUiCapability::Choice { .. }) => {
            parse_choice_input(new, &edit.input)
                .is_ok_and(|value| capability_has_value(new, &value))
        }
        (
            ConfigUiCapability::MultiChoice { ordered: old, .. },
            ConfigUiCapability::MultiChoice {
                ordered: new_ordered,
                ..
            },
        ) => old == new_ordered && parse_multi_choice_values(new, &edit.input).is_ok(),
        _ => false,
    }
}

fn reconcile_replacement_edit(
    old: &ConfigUiField,
    new: &ConfigUiField,
    edit: &mut ConfigUiEditState,
) -> bool {
    if !edit_is_compatible(old, new, edit) {
        return false;
    }
    let highlighted = edit_choices(old, &edit.input)
        .get(edit.choice_index)
        .copied();
    let choices = edit_choices(new, &edit.input);
    let choice_index = |value: &JsonValue| choices.iter().position(|choice| choice.value == *value);
    edit.choice_index = highlighted
        .and_then(|highlighted| choice_index(&highlighted.value))
        .or_else(|| {
            is_choice_picker(new)
                .then(|| parse_choice_input(new, &edit.input).ok())
                .flatten()
                .and_then(|selected| choice_index(&selected))
        })
        .unwrap_or_else(|| edit.choice_index.min(choices.len().saturating_sub(1)));
    true
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

fn edit_state_for_field(field: &ConfigUiField) -> Result<ConfigUiEditState, String> {
    let (input, mode, choice_index) = match &field.capability {
        ConfigUiCapability::ReadOnly { reason, .. } => return Err(reason.clone()),
        ConfigUiCapability::FreeText { encoding } => {
            (free_text_seed(field, *encoding)?, ConfigUiEditMode::Text, 0)
        }
        ConfigUiCapability::Toggle { .. } | ConfigUiCapability::Choice { .. } => {
            let value = direct_choice_seed(field)?;
            let index = capability_choice_index(field, value).ok_or_else(|| {
                format!("{} has no choice matching its editable value.", field.path)
            })?;
            (
                render_json_edit_value(value),
                ConfigUiEditMode::Choice,
                index,
            )
        }
        ConfigUiCapability::MultiChoice { .. } => {
            let value = direct_choice_seed(field)?;
            let input = render_json_edit_value(value);
            parse_multi_choice_values(field, &input)?;
            (input, ConfigUiEditMode::MultiChoice, 0)
        }
    };
    let cursor = input.len();
    Ok(ConfigUiEditState {
        field_id: field.id(),
        input,
        mode,
        choice_index,
        cursor,
    })
}

fn free_text_seed(field: &ConfigUiField, encoding: ConfigUiTextEncoding) -> Result<String, String> {
    let value = match &field.snapshot.intent {
        crate::ConfigUiOverride::Explicit(value) => value,
        crate::ConfigUiOverride::Absent => match &field.snapshot.effective {
            Some(resolved) => &resolved.value,
            None => return Ok(String::new()),
        },
        crate::ConfigUiOverride::Invalid { input } => return Ok(input.clone()),
    };
    match (encoding, value) {
        (ConfigUiTextEncoding::String, JsonValue::String(value)) => Ok(value.clone()),
        (ConfigUiTextEncoding::String, _) => Err(format!(
            "{} has a non-string value that cannot seed its string editor.",
            field.path
        )),
        (ConfigUiTextEncoding::Json, value) => Ok(render_json_edit_value(value)),
    }
}

fn direct_choice_seed(field: &ConfigUiField) -> Result<&JsonValue, String> {
    match &field.snapshot.intent {
        crate::ConfigUiOverride::Explicit(value) => Ok(value),
        crate::ConfigUiOverride::Absent => field
            .snapshot
            .effective
            .as_ref()
            .map(|resolved| &resolved.value)
            .ok_or_else(|| format!("{} has no resolved value to edit.", field.path)),
        crate::ConfigUiOverride::Invalid { .. } => Err(format!(
            "{} has invalid input; remove the override or use a free-text editor.",
            field.path
        )),
    }
}

fn parse_edit_input(field: &ConfigUiField, input: &str) -> Result<JsonValue, String> {
    match &field.capability {
        ConfigUiCapability::ReadOnly { .. } => Err(format!("{} is read-only.", field.path)),
        ConfigUiCapability::FreeText {
            encoding: ConfigUiTextEncoding::String,
        } => Ok(JsonValue::String(input.to_string())),
        ConfigUiCapability::FreeText {
            encoding: ConfigUiTextEncoding::Json,
        } => parse_choice_input(field, input),
        ConfigUiCapability::Toggle { .. } | ConfigUiCapability::Choice { .. } => {
            let value = parse_choice_input(field, input)?;
            if capability_has_value(field, &value) {
                Ok(value)
            } else {
                Err(format!("{} is not an available choice.", field.path))
            }
        }
        ConfigUiCapability::MultiChoice { choices, ordered } => {
            let values = parse_multi_choice_values(field, input)?;
            if *ordered {
                Ok(JsonValue::Array(values))
            } else {
                Ok(JsonValue::Array(
                    choices
                        .iter()
                        .filter(|choice| values.contains(&choice.value))
                        .map(|choice| choice.value.clone())
                        .collect(),
                ))
            }
        }
    }
}

fn parse_choice_input(field: &ConfigUiField, input: &str) -> Result<JsonValue, String> {
    serde_json::from_str(input.trim())
        .map_err(|source| format!("{} must be valid JSON: {source}.", field.path))
}

pub(crate) fn capability_choices(field: &ConfigUiField) -> Vec<&ConfigUiChoice> {
    match &field.capability {
        ConfigUiCapability::Toggle { off, on } => vec![off, on],
        ConfigUiCapability::Choice { choices }
        | ConfigUiCapability::MultiChoice { choices, .. } => choices.iter().collect(),
        ConfigUiCapability::ReadOnly { .. } | ConfigUiCapability::FreeText { .. } => Vec::new(),
    }
}

fn capability_choice_index(field: &ConfigUiField, value: &JsonValue) -> Option<usize> {
    capability_choices(field)
        .iter()
        .position(|choice| choice.value == *value)
}

fn capability_has_value(field: &ConfigUiField, value: &JsonValue) -> bool {
    capability_choice_index(field, value).is_some()
}

#[cfg(feature = "ui")]
pub(crate) fn single_choice_status_value(
    field: &ConfigUiField,
    edit: &ConfigUiEditState,
) -> String {
    let selected = parse_choice_input(field, &edit.input)
        .ok()
        .and_then(|value| choice_label_for_value(field, &value))
        .unwrap_or_else(|| "none".to_string());
    if !is_choice_picker(field) {
        return format!("selected {selected}");
    }
    let highlighted = edit_choices(field, &edit.input)
        .get(edit.choice_index)
        .map(|choice| choice.display_label())
        .unwrap_or_else(|| "none".to_string());
    if highlighted == selected {
        format!("selected {selected}")
    } else {
        format!("selected {selected}, highlighted {highlighted}")
    }
}

#[cfg(feature = "ui")]
pub(crate) fn multi_choice_status_value(field: &ConfigUiField, edit: &ConfigUiEditState) -> String {
    let values = parse_multi_choice_values(field, &edit.input).unwrap_or_default();
    let enabled = values.len();
    let selected = edit_choices(field, &edit.input)
        .get(edit.choice_index)
        .map(|choice| choice.display_label())
        .unwrap_or_else(|| "none".to_string());
    if is_ordered_multi_choice_field(field) {
        return format!(
            "{enabled}/{} enabled, selected {selected}, order {}",
            capability_choices(field).len(),
            multi_choice_order_label(field, &values)
        );
    }
    format!(
        "{enabled}/{} enabled, selected {selected}",
        capability_choices(field).len()
    )
}

#[cfg(feature = "ui")]
pub(crate) fn multi_choice_order_label(field: &ConfigUiField, values: &[JsonValue]) -> String {
    if values.is_empty() {
        "none".to_string()
    } else {
        values
            .iter()
            .map(|value| {
                choice_label_for_value(field, value)
                    .unwrap_or_else(|| render_json_edit_value(value))
            })
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn toggled_choice_input(field: &ConfigUiField, input: &str) -> Result<String, String> {
    let ConfigUiCapability::Toggle { off, on } = &field.capability else {
        return Err(format!("{} is not a toggle.", field.path));
    };
    let current = parse_choice_input(field, input)?;
    Ok(render_json_edit_value(if current == off.value {
        &on.value
    } else {
        &off.value
    }))
}

pub(crate) fn toggled_multi_choice_input(
    field: &ConfigUiField,
    input: &str,
    choice_index: usize,
) -> Result<String, String> {
    let target = edit_choice_value(field, input, choice_index)?;
    let mut values = parse_multi_choice_values(field, input)?;
    if values.contains(&target) {
        values.retain(|value| value != &target);
    } else {
        values.push(target);
    }
    if !is_ordered_multi_choice_field(field) {
        values = capability_choices(field)
            .into_iter()
            .filter(|choice| values.contains(&choice.value))
            .map(|choice| choice.value.clone())
            .collect();
    }
    Ok(render_json_edit_value(&JsonValue::Array(values)))
}

fn moved_ordered_multi_choice_input(
    field: &ConfigUiField,
    input: &str,
    choice_index: usize,
    delta: isize,
) -> Result<String, String> {
    let target = edit_choice_value(field, input, choice_index)?;
    let mut values = parse_multi_choice_values(field, input)?;
    if let Some(index) = values.iter().position(|value| value == &target) {
        let next = if delta < 0 {
            index.checked_sub(1)
        } else {
            (index + 1 < values.len()).then_some(index + 1)
        };
        if let Some(next) = next {
            values.swap(index, next);
        }
    }
    Ok(render_json_edit_value(&JsonValue::Array(values)))
}

fn edit_choice_value(
    field: &ConfigUiField,
    input: &str,
    choice_index: usize,
) -> Result<JsonValue, String> {
    edit_choices(field, input)
        .get(choice_index)
        .map(|choice| choice.value.clone())
        .ok_or_else(|| format!("{} has no value selected.", field.path))
}

fn edit_choice_index(field: &ConfigUiField, input: &str, value: &JsonValue) -> Option<usize> {
    edit_choices(field, input)
        .iter()
        .position(|choice| choice.value == *value)
}

pub(crate) fn edit_choices<'a>(field: &'a ConfigUiField, input: &str) -> Vec<&'a ConfigUiChoice> {
    let choices = capability_choices(field);
    if !is_ordered_multi_choice_field(field) {
        return choices;
    }
    let Ok(values) = parse_multi_choice_values(field, input) else {
        return choices;
    };
    let mut ordered = values
        .iter()
        .filter_map(|value| {
            choices
                .iter()
                .find(|choice| choice.value == *value)
                .copied()
        })
        .collect::<Vec<_>>();
    ordered.extend(
        choices
            .into_iter()
            .filter(|choice| !values.contains(&choice.value)),
    );
    ordered
}

fn parse_multi_choice_values(field: &ConfigUiField, input: &str) -> Result<Vec<JsonValue>, String> {
    let value = parse_choice_input(field, input)?;
    let JsonValue::Array(values) = value else {
        return Err(format!("{} must be a JSON array.", field.path));
    };
    for (index, value) in values.iter().enumerate() {
        if values[..index].contains(value) {
            return Err(format!("{} contains a duplicate selection.", field.path));
        }
        if !capability_has_value(field, value) {
            return Err(format!("{} contains an unavailable choice.", field.path));
        }
    }
    Ok(values)
}

#[cfg(feature = "ui")]
pub(crate) fn choice_label_for_value(field: &ConfigUiField, value: &JsonValue) -> Option<String> {
    capability_choices(field)
        .into_iter()
        .find(|choice| choice.value == *value)
        .map(ConfigUiChoice::display_label)
}

pub(crate) fn is_choice_picker(field: &ConfigUiField) -> bool {
    matches!(field.capability, ConfigUiCapability::Choice { .. })
}

pub(crate) fn is_ordered_multi_choice_field(field: &ConfigUiField) -> bool {
    matches!(
        field.capability,
        ConfigUiCapability::MultiChoice { ordered: true, .. }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::field_list_value;
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
            parse_edit_input(&app.model.fields[1], r#""dark""#).expect("select"),
            json!("dark")
        );
        assert_eq!(
            toggled_multi_choice_input(&app.model.fields[2], r#"["git"]"#, 1).expect("toggle"),
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
        assert_eq!(app.handle_key(ConfigUiKey::Char('e')), ConfigUiIntent::None);
        assert!(app.edit.is_some());
        app.handle_key(ConfigUiKey::Esc);
        assert_eq!(
            app.handle_key(ConfigUiKey::Char('u')),
            ConfigUiIntent::UnsetField {
                field: ConfigUiFieldId::new(DEFAULT_CONFIG_SOURCE_ID, "ui.theme")
            }
        );

        app.selected_row = 0;
        assert_eq!(app.handle_key(ConfigUiKey::Char(' ')), ConfigUiIntent::None);
        assert_eq!(app.edit.as_ref().expect("staged bool").input, "true");
        assert_eq!(app.handle_key(ConfigUiKey::Esc), ConfigUiIntent::None);
        assert!(app.edit.is_none());

        assert_eq!(app.handle_key(ConfigUiKey::Enter), ConfigUiIntent::None);
        assert!(app.edit.is_none());
        assert_eq!(field_list_value(&app.model.fields[0]), "false");
        assert_eq!(
            app.notice.as_ref().map(|notice| notice.text.as_str()),
            Some("Press Space to stage this change, then Enter to save.")
        );

        assert_eq!(app.handle_key(ConfigUiKey::Char(' ')), ConfigUiIntent::None);
        assert_eq!(app.edit.as_ref().expect("staged bool").input, "true");
        assert_eq!(
            app.handle_key(ConfigUiKey::Enter),
            ConfigUiIntent::SetField {
                field: ConfigUiFieldId::new(DEFAULT_CONFIG_SOURCE_ID, "server.enabled"),
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
                field: ConfigUiFieldId::new("server", "server.enabled"),
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
                field: ConfigUiFieldId::new("ui", "ui.title"),
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
                field: ConfigUiFieldId::new("ui", "ui.title"),
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
                field: ConfigUiFieldId::new("ui", "ui.title"),
            }
        );
    }

    // Defends: normal Enter and Space retain their existing edit activation for non-boolean fields.
    #[test]
    fn non_boolean_fields_keep_enter_and_space_activation() {
        let mut app = ConfigUiApp::new(test_model());
        app.selected_row = 1;

        assert_eq!(app.handle_key(ConfigUiKey::Enter), ConfigUiIntent::None);
        assert!(app.edit.is_some());
        app.handle_key(ConfigUiKey::Esc);
        assert_eq!(app.handle_key(ConfigUiKey::Char(' ')), ConfigUiIntent::None);
        assert!(app.edit.is_some());
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
        assert_eq!(app.handle_key(ConfigUiKey::Char('e')), ConfigUiIntent::None);
        app.edit.as_mut().expect("theme edit").choice_index = 0;
        let ConfigUiIntent::SetField { field, value } = app.handle_key(ConfigUiKey::Enter) else {
            panic!("expected theme SetField intent");
        };
        assert_eq!(
            field,
            ConfigUiFieldId::new(DEFAULT_CONFIG_SOURCE_ID, "ui.theme")
        );
        assert_eq!(value, json!("light"));
        assert_eq!(app.active_theme, ConfigUiTheme::Dark);

        let identity = field;
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

    // Defends: a successful reset-to-inherited write can switch a theme field without an active edit.
    #[test]
    fn successful_theme_field_unset_switches_to_inherited_theme() {
        let mut model = test_model();
        model.fields[1] = field("ui.theme", "string", "\"dark\"", &["light", "dark"]);
        model.fields[1].snapshot.baseline = Some(ConfigUiResolvedValue::new(json!("light")));
        model.theme_switcher = Some(theme_switcher());
        let mut app = ConfigUiApp::new(model);
        app.selected_row = 1;

        assert_eq!(app.active_theme, ConfigUiTheme::Dark);
        let ConfigUiIntent::UnsetField { field } = app.handle_key(ConfigUiKey::Char('u')) else {
            panic!("expected theme UnsetField intent");
        };
        assert_eq!(
            field,
            ConfigUiFieldId::new(DEFAULT_CONFIG_SOURCE_ID, "ui.theme")
        );
        assert_eq!(app.active_theme, ConfigUiTheme::Dark);

        let identity = field;
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
        model.recommended_fields = Some(vec![model.fields[1].id()]);
        let mut app = ConfigUiApp::new(model);
        app.selected_tab = 1;
        app.begin_edit_field(1);
        let edit = app.edit.as_mut().expect("edit");
        edit.input = r#""dark""#.to_string();
        edit.choice_index = 1;
        app.search = "theme".to_string();
        app.search_active = true;
        app.notice_error("Host reload warning.");

        let mut replacement = app.model.clone();
        replacement.tabs.swap(0, 1);
        replacement.fields.swap(0, 1);
        let ConfigUiCapability::Choice { choices } = &mut replacement.fields[0].capability else {
            panic!("theme choice capability");
        };
        choices.reverse();
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
                r#""dark""#
            ))
        );
        assert_eq!(app.search(), "theme");
        assert!(app.search_active());
        assert_eq!(app.settings_view(), ConfigUiSettingsView::Overview);
        assert_eq!(
            app.notice().map(|notice| notice.text.as_str()),
            Some("Host reload warning.")
        );
        assert_eq!(
            app.handle_key(ConfigUiKey::Enter),
            ConfigUiIntent::SetField {
                field: ConfigUiFieldId::new(DEFAULT_CONFIG_SOURCE_ID, "ui.theme"),
                value: json!("dark"),
            }
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
    // whether the active field disappears or its editor capability changes.
    #[test]
    fn ordinary_replacement_cancels_incompatible_edits_and_combines_notices() {
        fn assert_canceled(
            mut replacement: ConfigUiModel,
            mutate: impl FnOnce(&mut ConfigUiModel),
        ) {
            let mut app = ConfigUiApp::new(replacement.clone());
            app.begin_edit_field(1);
            app.edit.as_mut().expect("edit").input = r#""dark""#.to_string();
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
            model.fields[1].capability = ConfigUiCapability::ReadOnly {
                reason: "Host editor only.".to_string(),
                file_action_id: None,
            };
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
            app.edit.as_mut().expect("edit").input = r#""dark""#.to_string();
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
            model.fields[1].capability = ConfigUiCapability::ReadOnly {
                reason: "Use the host editor.".to_string(),
                file_action_id: None,
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
            open_file_intent("selected", "/tmp/selected", false)
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
                field: ConfigUiFieldId::new("ui", "ui.title"),
                input: "light".to_string(),
            }
        );
        let edit = app.edit.as_mut().expect("text edit");
        edit.input = "temporary title".to_string();
        edit.cursor = edit.input.len();
        assert_eq!(
            app.handle_key(ConfigUiKey::Ctrl('e')),
            ConfigUiIntent::EditTextExternally {
                field: ConfigUiFieldId::new("ui", "ui.title"),
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

        let title = ConfigUiFieldId::new("ui", "ui.title");
        let enabled = ConfigUiFieldId::new("server", "server.enabled");
        assert!(app.apply_external_text_edit(&title, "ignored").is_err());

        app.begin_edit_field(0);
        assert!(
            app.apply_external_text_edit(&enabled, "wrong field")
                .is_err()
        );
        assert_eq!(app.edit.as_ref().expect("text edit").input, "light");

        app.apply_external_text_edit(&title, "edited title")
            .expect("apply returned text");
        let edit = app.edit.as_ref().expect("text edit");
        assert_eq!(edit.input, "edited title");
        assert_eq!(edit.cursor, "edited title".len());
        assert_eq!(
            app.handle_key(ConfigUiKey::Enter),
            ConfigUiIntent::SetField {
                field: title,
                value: json!("edited title"),
            }
        );
    }

    // Defends: inferred TOML rows do not gain write authority from scalar syntax.
    #[test]
    fn toml_document_scalar_rows_remain_read_only() {
        let document = build_toml_document_fields(ConfigUiTomlDocumentSpec {
            source_id: "helix",
            tab: "native",
            section_label: "",
            current_toml: r#"
[editor]
line-number = "relative"
"#,
            baseline_toml: None,
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

        assert_eq!(app.handle_key(ConfigUiKey::Enter), ConfigUiIntent::None);
        assert!(app.edit().is_none());
        assert!(
            app.notice().is_some_and(|notice| {
                notice.text.contains("No editor capability was declared")
            })
        );
    }

    // Defends: reset removes only an authorized present override; capability and baseline
    // knowledge do not grant or revoke that independent source operation.
    #[test]
    fn reset_availability_follows_override_authority_and_source_writability() {
        let identity = ConfigUiFieldId::new("ui", "ui.theme");
        let mut explicit = field_with_source("ui", "ui.theme", "string", "\"custom\"", &[]);
        explicit.snapshot.baseline = None;
        explicit.capability = ConfigUiCapability::ReadOnly {
            reason: "Edit values elsewhere.".to_string(),
            file_action_id: None,
        };
        let mut app = ConfigUiApp::new(model_with_fields(vec![explicit.clone()]));

        assert_eq!(
            app.handle_key(ConfigUiKey::Char('u')),
            ConfigUiIntent::UnsetField {
                field: identity.clone(),
            }
        );
        app.notice_error("Host rejected reset.");
        assert_eq!(
            app.handle_key(ConfigUiKey::Char('u')),
            ConfigUiIntent::UnsetField {
                field: identity.clone(),
            }
        );

        complete_unset(&mut app, identity.clone());
        assert_eq!(app.handle_key(ConfigUiKey::Char('u')), ConfigUiIntent::None);

        let mut invalid = explicit.clone();
        invalid.snapshot.intent = ConfigUiOverride::Invalid {
            input: "not valid".to_string(),
        };
        let mut app = ConfigUiApp::new(model_with_fields(vec![invalid]));
        assert_eq!(
            app.handle_key(ConfigUiKey::Char('u')),
            ConfigUiIntent::UnsetField {
                field: identity.clone(),
            }
        );

        let mut forbidden = explicit.clone();
        forbidden.can_unset = false;
        let mut app = ConfigUiApp::new(model_with_fields(vec![forbidden]));
        assert_eq!(app.handle_key(ConfigUiKey::Char('u')), ConfigUiIntent::None);

        let mut read_only_model = model_with_fields(vec![explicit.clone()]);
        read_only_model.sources[0].read_only = true;
        let mut app = ConfigUiApp::new(read_only_model);
        assert_eq!(app.handle_key(ConfigUiKey::Char('u')), ConfigUiIntent::None);

        let mut action_model = model_with_fields(vec![explicit]);
        action_model.file_actions = vec![file_action("open", "/tmp/settings", true, true)];
        let mut app = ConfigUiApp::new(action_model);
        app.selected_row = 1;
        assert_eq!(app.handle_key(ConfigUiKey::Char('u')), ConfigUiIntent::None);
    }

    // Defends: capability parsing preserves exact host values and text encodings.
    #[test]
    fn edit_parser_obeys_declared_capabilities() {
        let bool_field = field("server.enabled", "bool", "false", &[]);
        assert_eq!(
            parse_edit_input(&bool_field, "true").expect("bool"),
            json!(true)
        );
        assert!(parse_edit_input(&bool_field, "yes").is_err());

        let enum_field = field("ui.theme", "string", "\"light\"", &["light", "dark"]);
        assert_eq!(
            parse_edit_input(&enum_field, r#""dark""#).expect("enum"),
            json!("dark")
        );
        assert!(parse_edit_input(&enum_field, r#""wide""#).is_err());

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
            parse_edit_input(&list_field, r#"["search","git"]"#).expect("unordered list"),
            json!(["git", "search"])
        );
        assert!(parse_edit_input(&list_field, r#"["unknown"]"#).is_err());
        assert!(parse_edit_input(&list_field, r#"["git","git"]"#).is_err());

        let string_field = field("ui.title", "string", r#""title""#, &[]);
        assert_eq!(
            parse_edit_input(&string_field, "search, git").expect("free string"),
            json!("search, git")
        );
    }

    // Defends: display type text cannot grant editing, while a declared toggle can use any two
    // exact JSON values instead of being restricted to booleans.
    #[test]
    fn capability_not_type_label_authorizes_toggle_editing() {
        let mut display_bool = field("display.bool", "bool", "false", &[]);
        display_bool.capability = ConfigUiCapability::ReadOnly {
            reason: "Managed by the host.".to_string(),
            file_action_id: None,
        };
        let mut app = ConfigUiApp::new(model_with_fields(vec![display_bool]));

        assert_eq!(app.handle_key(ConfigUiKey::Char(' ')), ConfigUiIntent::None);
        assert!(app.edit().is_none());
        assert_eq!(
            app.notice().map(|notice| notice.text.as_str()),
            Some("Managed by the host.")
        );

        let mut encoded = field("service.mode", "string", r#""disabled""#, &[]);
        encoded.capability = ConfigUiCapability::Toggle {
            off: ConfigUiChoice::new(json!("disabled")),
            on: ConfigUiChoice {
                value: json!("enabled"),
                label: Some("Enabled".to_string()),
            },
        };
        let mut app = ConfigUiApp::new(model_with_fields(vec![encoded]));

        assert_eq!(app.handle_key(ConfigUiKey::Char(' ')), ConfigUiIntent::None);
        assert_eq!(
            app.edit().expect("encoded toggle edit").input,
            r#""enabled""#
        );
        #[cfg(feature = "ui")]
        assert_eq!(
            single_choice_status_value(&app.model.fields[0], app.edit().expect("toggle edit")),
            "selected Enabled"
        );
        assert_eq!(
            app.handle_key(ConfigUiKey::Enter),
            ConfigUiIntent::SetField {
                field: ConfigUiFieldId::new(DEFAULT_CONFIG_SOURCE_ID, "service.mode"),
                value: json!("enabled"),
            }
        );
    }

    // Defends: free-text creation starts empty only for an unknown absent value and never coerces
    // a known value that is incompatible with the declared encoding.
    #[test]
    fn free_text_seeds_preserve_sparse_state_and_encoding() {
        let mut absent = field("ui.title", "string", r#""default""#, &[]);
        absent.snapshot.intent = ConfigUiOverride::Absent;
        absent.snapshot.effective = None;
        absent.snapshot.baseline = None;
        assert_eq!(
            edit_state_for_field(&absent)
                .expect("empty creation edit")
                .input,
            ""
        );

        let mut incompatible = absent.clone();
        incompatible.snapshot.intent = ConfigUiOverride::Explicit(json!(12));
        incompatible.snapshot.effective = Some(ConfigUiResolvedValue::new(json!(12)));
        assert!(
            edit_state_for_field(&incompatible)
                .expect_err("string editor must reject a number seed")
                .contains("non-string value")
        );

        incompatible.capability = ConfigUiCapability::FreeText {
            encoding: ConfigUiTextEncoding::Json,
        };
        assert_eq!(
            edit_state_for_field(&incompatible)
                .expect("JSON editor accepts any JSON value")
                .input,
            "12"
        );
    }

    // Defends: reload compatibility is capability-specific and never reinterprets a staged buffer
    // under a different editor contract.
    #[test]
    fn staged_edit_compatibility_is_exact() {
        let choice = field("ui.theme", "string", r#""light""#, &["light", "dark"]);
        let edit = edit_state_for_field(&choice).expect("choice edit");
        assert!(edit_is_compatible(&choice, &choice, &edit));

        let mut missing = choice.clone();
        missing.capability = ConfigUiCapability::Choice {
            choices: vec![ConfigUiChoice::new(json!("dark"))],
        };
        assert!(!edit_is_compatible(&choice, &missing, &edit));

        let mut toggle = choice.clone();
        toggle.capability = ConfigUiCapability::Toggle {
            off: ConfigUiChoice::new(json!("light")),
            on: ConfigUiChoice::new(json!("dark")),
        };
        assert!(!edit_is_compatible(&choice, &toggle, &edit));

        let string_text = field("ui.title", "string", r#""title""#, &[]);
        let text_edit = edit_state_for_field(&string_text).expect("text edit");
        let mut json_text = string_text.clone();
        json_text.capability = ConfigUiCapability::FreeText {
            encoding: ConfigUiTextEncoding::Json,
        };
        assert!(!edit_is_compatible(&string_text, &json_text, &text_edit));

        let ordered = field(
            "plugins.enabled",
            "string_list",
            r#"["git"]"#,
            &["git", "search"],
        );
        let multi_edit = edit_state_for_field(&ordered).expect("multichoice edit");
        let mut reordered = ordered.clone();
        if let ConfigUiCapability::MultiChoice { ordered, .. } = &mut reordered.capability {
            *ordered = true;
        }
        assert!(!edit_is_compatible(&ordered, &reordered, &multi_edit));

        let mut highlighted_removed = multi_edit;
        highlighted_removed.choice_index = 1;
        let mut reduced = ordered.clone();
        let ConfigUiCapability::MultiChoice { choices, .. } = &mut reduced.capability else {
            panic!("multichoice capability");
        };
        choices.pop();
        assert!(reconcile_replacement_edit(
            &ordered,
            &reduced,
            &mut highlighted_removed
        ));
        assert_eq!(highlighted_removed.choice_index, 0);
        assert_eq!(highlighted_removed.input, r#"["git"]"#);
    }

    // Defends: edit initialization follows sparse state rather than confusing invalid input with
    // display text, while bools and scalar enums keep their native choice modes.
    #[test]
    fn edit_initialization_uses_sparse_state_and_capabilities() {
        let bool_field = field("server.enabled", "bool", "true", &[]);
        let bool_edit = edit_state_for_field(&bool_field).expect("toggle edit");
        assert_eq!(bool_edit.input, "true");
        assert_eq!(bool_edit.mode, ConfigUiEditMode::Choice);

        let enum_field = field("ui.theme", "string", "\"light\"", &["light", "dark"]);
        let enum_edit = edit_state_for_field(&enum_field).expect("choice edit");
        assert_eq!(enum_edit.input, r#""light""#);
        assert_eq!(enum_edit.mode, ConfigUiEditMode::Choice);

        let mut invalid_field = field("server.port", "int", "80", &[]);
        invalid_field.snapshot.intent = ConfigUiOverride::Invalid {
            input: "not set".to_string(),
        };
        assert_eq!(
            edit_state_for_field(&invalid_field)
                .expect("invalid free-text edit")
                .input,
            "not set"
        );
    }

    // Defends: unordered multichoice remains set-like and canonicalizes selected values to
    // capability order.
    #[test]
    fn unordered_multichoice_keeps_capability_order() {
        let field = field(
            "widgets.enabled",
            "string_list",
            r#"["status"]"#,
            &["clock", "status", "mode"],
        );

        assert_eq!(
            toggled_multi_choice_input(&field, r#"["status"]"#, 0).expect("toggle clock"),
            r#"["clock","status"]"#
        );
    }

    // Defends: ordered multichoice editing is opt-in and preserves config order when toggling
    // selected values.
    #[test]
    fn ordered_multichoice_preserves_order_when_toggling() {
        let mut field = field(
            "widgets.enabled",
            "string_list",
            r#"["status","clock"]"#,
            &["clock", "status", "mode"],
        );
        if let ConfigUiCapability::MultiChoice { ordered, .. } = &mut field.capability {
            *ordered = true;
        }

        assert!(is_ordered_multi_choice_field(&field));
        assert_eq!(
            edit_state_for_field(&field).expect("ordered edit").mode,
            ConfigUiEditMode::MultiChoice
        );
        assert_eq!(
            toggled_multi_choice_input(&field, r#"["status","clock"]"#, 2).expect("toggle mode"),
            r#"["status","clock","mode"]"#
        );
        assert_eq!(
            toggled_multi_choice_input(&field, r#"["status","clock","mode"]"#, 1)
                .expect("remove clock"),
            r#"["status","mode"]"#
        );
    }

    // Defends: ordered multichoice fields can move enabled values without changing unordered
    // semantics.
    #[test]
    fn ordered_multichoice_reducer_reorders_enabled_values() {
        let mut model = test_model();
        model.fields = vec![field(
            "widgets.enabled",
            "string_list",
            r#"["status","clock"]"#,
            &["clock", "status", "mode"],
        )];
        if let ConfigUiCapability::MultiChoice { ordered, .. } = &mut model.fields[0].capability {
            *ordered = true;
        }
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
                field: ConfigUiFieldId::new(DEFAULT_CONFIG_SOURCE_ID, "widgets.enabled"),
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

    // Defends: Overview is the focused default, search widens without changing it, and toggles preserve a surviving selection.
    #[test]
    fn reducer_controls_overview_all_views_without_hidden_selection_or_search_state_leaks() {
        let mut recommended = field("core.visible", "string", r#""core""#, &[]);
        crate::model::set_field_state_for_test(&mut recommended, ConfigUiFieldState::Inherited);
        let mut hidden = field("advanced.hidden", "string", r#""hidden""#, &[]);
        crate::model::set_field_state_for_test(&mut hidden, ConfigUiFieldState::Inherited);
        let explicit = field("advanced.explicit", "string", r#""set""#, &[]);
        let mut other_tab = field("other.visible", "string", r#""other""#, &[]);
        crate::model::set_field_state_for_test(&mut other_tab, ConfigUiFieldState::Inherited);
        other_tab.tab = "other".to_string();
        let mut model = model_with_fields(vec![recommended, hidden, explicit, other_tab]);
        model.tabs.push("other".to_string());
        model.recommended_fields = Some(vec![
            ConfigUiFieldId::new(DEFAULT_CONFIG_SOURCE_ID, "core.visible"),
            ConfigUiFieldId::new(DEFAULT_CONFIG_SOURCE_ID, "other.visible"),
        ]);
        let mut app = ConfigUiApp::new(model);

        assert_eq!(app.settings_view, ConfigUiSettingsView::Overview);
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
        assert_eq!(app.settings_view, ConfigUiSettingsView::Overview);
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
        assert_eq!(app.settings_view, ConfigUiSettingsView::Overview);
        app.handle_key(ConfigUiKey::Ctrl('u'));
        for ch in "advanced.hidden".chars() {
            app.handle_key(ConfigUiKey::Char(ch));
        }
        assert_eq!(app.visible_rows(), vec![UiRowRef::Field(1)]);
        app.handle_key(ConfigUiKey::Enter);
        app.handle_key(ConfigUiKey::Char('a'));
        assert_eq!(app.settings_view, ConfigUiSettingsView::Overview);
        app.handle_key(ConfigUiKey::Esc);
        assert!(app.search.is_empty());
        assert_eq!(
            app.visible_rows(),
            vec![UiRowRef::Field(0), UiRowRef::Field(2)]
        );

        app.next_tab();
        assert!(!app.selected_tab_has_non_overview_fields());
        app.handle_key(ConfigUiKey::Char('a'));
        assert_eq!(app.settings_view, ConfigUiSettingsView::Overview);
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

    // Regression: normal-mode shortcuts remain ordinary text while search or scalar text editing
    // is active.
    #[test]
    fn normal_shortcuts_remain_search_and_edit_input() {
        let mut model = model_with_fields(vec![field("ui.scale", "integer", "1", &[])]);
        model.tabs = vec!["general".to_string(), "advanced".to_string()];
        let mut app = ConfigUiApp::new(model);

        app.search_active = true;
        assert!(!app.can_unset_field(&app.model.fields[0]));
        assert_eq!(app.handle_key(ConfigUiKey::Char('u')), ConfigUiIntent::None);
        assert_eq!(app.handle_key(ConfigUiKey::Char('2')), ConfigUiIntent::None);
        assert_eq!(app.handle_key(ConfigUiKey::Char('?')), ConfigUiIntent::None);
        assert_eq!(app.search, "u2?");
        assert_eq!(app.selected_tab, 0);

        app.search_active = false;
        app.search.clear();
        app.begin_edit_field(0);
        assert!(!app.can_unset_field(&app.model.fields[0]));
        assert_eq!(app.handle_key(ConfigUiKey::Char('u')), ConfigUiIntent::None);
        assert_eq!(app.handle_key(ConfigUiKey::Char('2')), ConfigUiIntent::None);
        assert_eq!(app.handle_key(ConfigUiKey::Char('a')), ConfigUiIntent::None);
        assert_eq!(app.handle_key(ConfigUiKey::Char('?')), ConfigUiIntent::None);
        assert_eq!(app.edit.as_ref().expect("text edit").input, "1u2a?");
        assert_eq!(app.selected_tab, 0);
    }

    // Defends: shortcut help captures only its private navigation keys and closes without
    // mutating editor, selection, search, notice, or host-visible intent state.
    #[test]
    fn shortcut_help_is_a_non_mutating_private_mode() {
        let mut app = ConfigUiApp::new(test_model());
        app.selected_row = 1;
        app.search = "theme".to_string();
        app.notice_info("Keep this notice.");
        let before = (
            app.selected_tab,
            app.selected_row,
            app.settings_view,
            app.search.clone(),
            app.search_active,
            app.edit.clone(),
            app.notice.clone(),
        );

        assert_eq!(app.handle_key(ConfigUiKey::Char('?')), ConfigUiIntent::None);
        assert_eq!(app.shortcut_help_scroll, Some(0));
        assert_eq!(app.handle_key(ConfigUiKey::Char('q')), ConfigUiIntent::None);
        assert!(app.shortcut_help_scroll.is_some());
        app.handle_key(ConfigUiKey::Down);
        app.handle_key(ConfigUiKey::Char('j'));
        assert_eq!(app.shortcut_help_scroll, Some(2));
        app.handle_key(ConfigUiKey::Up);
        app.handle_key(ConfigUiKey::Char('k'));
        assert_eq!(app.shortcut_help_scroll, Some(0));
        app.handle_key(ConfigUiKey::Esc);
        assert!(app.shortcut_help_scroll.is_none());
        assert_eq!(
            (
                app.selected_tab,
                app.selected_row,
                app.settings_view,
                app.search.clone(),
                app.search_active,
                app.edit.clone(),
                app.notice.clone(),
            ),
            before
        );

        app.handle_key(ConfigUiKey::Char('?'));
        app.handle_key(ConfigUiKey::Char('?'));
        assert!(app.shortcut_help_scroll.is_none());
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
        assert_eq!(app.handle_key(ConfigUiKey::Char('e')), ConfigUiIntent::None);
        assert_eq!(
            app.edit.as_ref().map(|edit| &edit.field_id),
            Some(&ConfigUiFieldId::new(DEFAULT_CONFIG_SOURCE_ID, "ui.theme"))
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
        assert_eq!(app.edit.as_ref().expect("single edit").input, r#""dark""#);
        assert_eq!(
            app.handle_key(ConfigUiKey::Enter),
            ConfigUiIntent::SetField {
                field: ConfigUiFieldId::new(DEFAULT_CONFIG_SOURCE_ID, "ui.theme"),
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
                field: ConfigUiFieldId::new(DEFAULT_CONFIG_SOURCE_ID, "plugins.enabled"),
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

    fn open_file_intent(action_id: &str, path: &str, create_if_missing: bool) -> ConfigUiIntent {
        ConfigUiIntent::OpenFile {
            source_id: "native".to_string(),
            action_id: action_id.to_string(),
            path: PathBuf::from(path),
            create_if_missing,
        }
    }

    // Defends: a read-only field opens the exact host-declared action even when its source has
    // other file actions.
    #[test]
    fn read_only_field_opens_its_declared_file_action() {
        let mut structured = field_with_source("native", "editor.rulers", "bool", "true", &[]);
        structured.capability = ConfigUiCapability::ReadOnly {
            reason: "Edit the source file directly.".to_string(),
            file_action_id: Some("settings".to_string()),
        };
        let mut model = model_with_fields(vec![structured]);
        model.file_actions = vec![file_action("settings", "/tmp/settings", true, true)];
        let mut app = ConfigUiApp::new(model);

        assert_eq!(
            app.handle_key(ConfigUiKey::Char('e')),
            open_file_intent("settings", "/tmp/settings", false)
        );
        assert_eq!(app.handle_key(ConfigUiKey::Enter), ConfigUiIntent::None);
        assert_eq!(
            app.notice.as_ref().map(|notice| notice.text.as_str()),
            Some("Edit the source file directly.")
        );

        app.model
            .file_actions
            .push(file_action("other", "/tmp/other", true, true));
        assert_eq!(
            app.handle_key(ConfigUiKey::Char('e')),
            open_file_intent("settings", "/tmp/settings", false)
        );
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
            open_file_intent("existing", "/tmp/acme/existing.toml", false)
        );

        app.selected_row = 1;
        assert_eq!(
            app.handle_key(ConfigUiKey::Char(' ')),
            open_file_intent("missing", "/tmp/acme/missing.toml", true)
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
