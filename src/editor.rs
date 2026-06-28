// Test lane: default

use super::{
    ConfigUiEditBehavior, ConfigUiField, ConfigUiFileAction, ConfigUiModel, UiRowRef,
    visible_rows_for_tab_search,
};
use crate::model::{string_list_values_from_json, validate_string_choice_value};
use serde_json::Value as JsonValue;
use std::collections::BTreeSet;
use std::path::PathBuf;

pub struct ConfigUiApp {
    pub model: ConfigUiModel,
    pub selected_tab: usize,
    pub selected_row: usize,
    pub search: String,
    pub search_active: bool,
    pub edit: Option<ConfigUiEditState>,
    pub notice: Option<ConfigUiNotice>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiNotice {
    pub text: String,
    pub is_error: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigUiEditState {
    pub field_index: usize,
    pub input: String,
    pub mode: ConfigUiEditMode,
    pub choice_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigUiEditMode {
    Text,
    Choice,
    MultiChoice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigUiKey {
    Esc,
    Enter,
    Backspace,
    Tab,
    BackTab,
    Up,
    Down,
    Left,
    Right,
    Char(char),
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

impl ConfigUiApp {
    pub fn new(model: ConfigUiModel) -> Self {
        Self {
            model,
            selected_tab: 0,
            selected_row: 0,
            search: String::new(),
            search_active: false,
            edit: None,
            notice: None,
        }
    }

    pub fn visible_rows(&self) -> Vec<UiRowRef> {
        visible_rows_for_tab_search(&self.model, self.selected_tab, &self.search)
    }

    pub fn next_tab(&mut self) {
        let len = self.model.tabs.len();
        if len > 0 {
            self.selected_tab = (self.selected_tab + 1) % len;
            self.selected_row = 0;
        }
    }

    pub fn previous_tab(&mut self) {
        let len = self.model.tabs.len();
        if len > 0 {
            self.selected_tab = (self.selected_tab + len - 1) % len;
            self.selected_row = 0;
        }
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
        if self.selected_tab >= self.model.tabs.len() {
            self.selected_tab = 0;
        }
        self.clamp_selection_for_len(self.visible_rows().len());
    }

    pub fn clamp_selection_for_len(&mut self, len: usize) {
        self.selected_row = if len == 0 {
            0
        } else {
            self.selected_row.min(len - 1)
        };
    }

    pub fn selected_field_index(&self) -> Option<usize> {
        let row = self.visible_rows().get(self.selected_row).copied()?;
        match row {
            UiRowRef::Field(index) => Some(index),
            _ => None,
        }
    }

    pub fn selected_field(&self) -> Option<&ConfigUiField> {
        self.selected_field_index()
            .and_then(|index| self.model.fields.get(index))
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
        self.edit = Some(ConfigUiEditState {
            field_index,
            choice_index: initial_edit_choice_index(field, &input),
            input,
            mode: edit_mode_for_field(field),
        });
    }

    pub fn finish_successful_write(&mut self) {
        self.edit = None;
    }

    fn cancel_edit(&mut self) -> ConfigUiIntent {
        self.edit = None;
        self.notice_info("Edit canceled.");
        ConfigUiIntent::None
    }

    fn update_edit_input<T>(&mut self, update: impl FnOnce(&mut String) -> T) -> ConfigUiIntent {
        self.notice = None;
        if let Some(edit) = &mut self.edit {
            update(&mut edit.input);
        }
        ConfigUiIntent::None
    }

    fn handle_search_key(&mut self, key: ConfigUiKey) {
        match key {
            ConfigUiKey::Esc | ConfigUiKey::Enter => self.search_active = false,
            ConfigUiKey::Backspace => {
                self.search.pop();
            }
            ConfigUiKey::Ctrl('u' | 'U') => {
                self.search.clear();
            }
            ConfigUiKey::Char(ch) => {
                self.search.push(ch);
                self.selected_row = 0;
            }
            _ => {}
        }
    }

    fn handle_normal_key(&mut self, key: ConfigUiKey) -> ConfigUiIntent {
        match key {
            ConfigUiKey::Char('q') | ConfigUiKey::Esc | ConfigUiKey::Ctrl('c') => {
                ConfigUiIntent::Exit
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
            ConfigUiKey::Enter | ConfigUiKey::Char(' ') => self.activate_selected_row(),
            ConfigUiKey::Char('e') => self.edit_or_activate_selected_row(),
            ConfigUiKey::Char('u') => self.return_selected_field_to_default(),
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
            ConfigUiKey::Backspace => self.update_edit_input(String::pop),
            ConfigUiKey::Ctrl('u' | 'U') => self.update_edit_input(String::clear),
            ConfigUiKey::Char(ch) => self.update_edit_input(|input| input.push(ch)),
            _ => ConfigUiIntent::None,
        }
    }

    fn handle_choice_edit_key(
        &mut self,
        key: ConfigUiKey,
        mode: ConfigUiEditMode,
    ) -> ConfigUiIntent {
        let field = self
            .edit
            .as_ref()
            .and_then(|edit| self.model.fields.get(edit.field_index));
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
        }
    }

    fn move_choice_edit(&mut self, delta: isize) {
        let Some(edit) = &mut self.edit else {
            return;
        };
        let len = self.model.fields[edit.field_index].allowed_values.len();
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
        let Some(edit) = &mut self.edit else {
            return;
        };
        let Some(value) = self.model.fields[edit.field_index]
            .allowed_values
            .get(edit.choice_index)
        else {
            return;
        };
        edit.input = value.clone();
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
        let next = match self.edit.as_ref().map(|edit| {
            let field = &self.model.fields[edit.field_index];
            next_input(field, edit)
        }) {
            None => return,
            Some(Ok(next)) => next,
            Some(Err(message)) => {
                self.notice_error(message);
                return;
            }
        };
        if let Some(edit) = &mut self.edit {
            edit.input = next;
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
        if !field.has_default_value() {
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
        let Some(edit) = self.edit.clone() else {
            return ConfigUiIntent::None;
        };
        let field = self.model.fields[edit.field_index].clone();
        let value = match parse_edit_input(&field, &edit.input) {
            Ok(value) => value,
            Err(message) => {
                self.notice_error(message);
                return ConfigUiIntent::None;
            }
        };
        ConfigUiIntent::SetField {
            field_index: edit.field_index,
            source_id: field.source_id,
            path: field.path,
            value,
        }
    }
}

pub fn edit_input_for_field(field: &ConfigUiField) -> String {
    if field.current_value == "not set" {
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
        return parse_rendered_json_string(&field.current_value)
            .unwrap_or_else(|| field.current_value.clone());
    }
    if field.edit_value.is_empty() {
        field.current_value.clone()
    } else {
        field.edit_value.clone()
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
        && let Some(index) = values.first().and_then(|value| {
            field
                .allowed_values
                .iter()
                .position(|allowed| allowed == value)
        })
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
    let selected = field
        .allowed_values
        .get(edit.choice_index)
        .map(String::as_str)
        .unwrap_or("none");
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
    let target = field
        .allowed_values
        .get(choice_index)
        .ok_or_else(|| format!("{} has no value selected.", field.path))?;
    let mut values = parse_string_list_values(field, input)?;
    if values.iter().any(|value| value == target) {
        values.retain(|value| value != target);
    } else {
        values.push(target.clone());
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
    let target = field
        .allowed_values
        .get(choice_index)
        .ok_or_else(|| format!("{} has no value selected.", field.path))?;
    let mut values = parse_string_list_values(field, input)?;
    let Some(index) = values.iter().position(|value| value == target) else {
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
    serde_json::from_str::<Vec<String>>(&field.edit_value)
        .map(|keys| keys.join(", "))
        .unwrap_or_else(|_| field.edit_value.clone())
}

pub fn field_bool_value(field: &ConfigUiField) -> Option<bool> {
    field.current_value.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "ui")]
    use crate::jsonc::{PatchMutation, set_jsonc_value_text};
    #[cfg(feature = "ui")]
    use crate::row_line_for_model;
    use crate::{
        ConfigUiApplyStatus, ConfigUiPathOwner, ConfigUiValueState, DEFAULT_CONFIG_SOURCE_ID,
    };
    use serde_json::json;
    use std::path::PathBuf;

    fn field(path: &str, kind: &str, value: &str, allowed: &[&str]) -> ConfigUiField {
        field_with_source(DEFAULT_CONFIG_SOURCE_ID, path, kind, value, allowed)
    }

    fn field_with_source(
        source_id: &str,
        path: &str,
        kind: &str,
        value: &str,
        allowed: &[&str],
    ) -> ConfigUiField {
        ConfigUiField {
            source_id: source_id.to_string(),
            path: path.to_string(),
            display_label: String::new(),
            list_cells: Vec::new(),
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

    // Defends: the reusable ratconfig layer can drive a non-Yazelix config fixture with bool, select, multiselect, rendering, and JSONC patching.
    #[cfg(feature = "ui")]
    #[test]
    fn non_yazelix_fixture_uses_generic_model_editor_render_and_jsonc_patch() {
        let model = ConfigUiModel {
            active_config_path: PathBuf::from("/tmp/acme/settings.jsonc"),
            cursor_config_path: PathBuf::from("/tmp/acme/cursors.jsonc"),
            default_cursor_config_path: PathBuf::from("/tmp/acme/default_cursors.jsonc"),
            active_config_exists: true,
            config_owner: ConfigUiPathOwner::User,
            config_read_only: false,
            sources: Vec::new(),
            tabs: vec!["general".to_string()],
            tab_list_tables: std::collections::BTreeMap::new(),
            fields: vec![
                field("server.enabled", "bool", "false", &[]),
                field("ui.theme", "string", "\"light\"", &["light", "dark"]),
                field(
                    "plugins.enabled",
                    "string_list",
                    r#"["git"]"#,
                    &["git", "search"],
                ),
            ],
            file_actions: Vec::new(),
            sidecars: Vec::new(),
            native_config_statuses: Vec::new(),
            diagnostics: Vec::new(),
        };
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

        let raw = r#"{
  // host-owned config
  "ui": { "theme": "light" }
}
"#;
        let patched =
            set_jsonc_value_text(raw, "ui.theme", &json!("dark")).expect("generic jsonc patch");
        assert_eq!(patched.mutation, PatchMutation::Replaced);
        assert!(patched.text.contains("// host-owned config"));
        assert!(patched.text.contains(r#""theme": "dark""#));
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
        app.finish_successful_write();
        assert_eq!(app.handle_key(ConfigUiKey::Esc), ConfigUiIntent::Exit);
    }

    // Defends: edit intents carry source identity and completed writes return to normal routing.
    #[test]
    fn edit_intents_preserve_selected_field_source() {
        let mut model = test_model();
        model.fields = vec![
            field_with_source("server", "server.enabled", "bool", "false", &[]),
            field_with_source("ui", "ui.title", "string", "\"light\"", &[]),
        ];
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
        app.finish_successful_write();

        app.selected_row = 1;
        assert_eq!(
            app.handle_key(ConfigUiKey::Char('e')),
            ConfigUiIntent::BeginEdit {
                field_index: 1,
                source_id: "ui".to_string(),
                path: "ui.title".to_string(),
            }
        );
        app.begin_edit_field(1);
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

        app.finish_successful_write();
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

    // Defends: return-to-default stays on the host-owned unset intent and is unavailable without a default.
    #[test]
    fn return_to_default_requires_default_value() {
        let mut model = test_model();
        model.fields = vec![
            field_with_source("ui", "ui.theme", "string", "\"custom\"", &[]),
            field_with_source("scratch", "scratch.note", "string", "\"custom\"", &[]),
        ];
        model.fields[1].default_value = crate::NO_CONFIG_DEFAULT_VALUE_LABEL.to_string();
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

    // Defends: bools keep direct choice edits while scalar enums use the single-select picker mode.
    #[test]
    fn edit_helpers_use_choice_modes_for_bool_and_enum() {
        let bool_field = field("server.enabled", "bool", "true", &[]);
        assert_eq!(field_bool_value(&bool_field), Some(true));
        assert_eq!(edit_mode_for_field(&bool_field), ConfigUiEditMode::Choice);

        let enum_field = field("ui.theme", "string", "\"light\"", &["light", "dark"]);
        assert_eq!(edit_input_for_field(&enum_field), "light");
        assert_eq!(edit_mode_for_field(&enum_field), ConfigUiEditMode::Choice);
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
            toggled_string_list_input(&field, r#"["status","clock","mode"]"#, 0)
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
        assert_eq!(app.edit.as_ref().expect("ordered edit").choice_index, 1);
        assert_eq!(app.handle_key(ConfigUiKey::Char('J')), ConfigUiIntent::None);
        assert_eq!(
            app.edit.as_ref().expect("ordered edit").input,
            r#"["clock","status"]"#
        );
        assert_eq!(app.handle_key(ConfigUiKey::Char('K')), ConfigUiIntent::None);
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
        for ch in "theme".chars() {
            assert_eq!(app.handle_key(ConfigUiKey::Char(ch)), ConfigUiIntent::None);
        }
        assert_eq!(app.handle_key(ConfigUiKey::Ctrl('U')), ConfigUiIntent::None);
        assert!(app.search.is_empty());
        assert_eq!(app.handle_key(ConfigUiKey::Enter), ConfigUiIntent::None);
        assert!(!app.search_active);
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
        ConfigUiModel {
            active_config_path: PathBuf::from("/tmp/acme/settings.jsonc"),
            cursor_config_path: PathBuf::from("/tmp/acme/cursors.jsonc"),
            default_cursor_config_path: PathBuf::from("/tmp/acme/default_cursors.jsonc"),
            active_config_exists: true,
            config_owner: ConfigUiPathOwner::User,
            config_read_only: false,
            sources: Vec::new(),
            tabs: vec!["general".to_string()],
            tab_list_tables: std::collections::BTreeMap::new(),
            fields: vec![
                field("server.enabled", "bool", "false", &[]),
                field("ui.theme", "string", "\"light\"", &["light", "dark"]),
                field(
                    "plugins.enabled",
                    "string_list",
                    r#"["git"]"#,
                    &["git", "search"],
                ),
            ],
            file_actions: Vec::new(),
            sidecars: Vec::new(),
            native_config_statuses: Vec::new(),
            diagnostics: Vec::new(),
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
            app.handle_key(ConfigUiKey::Char('e')),
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
