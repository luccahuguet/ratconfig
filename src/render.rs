// Test lane: default
use super::*;
use crate::model::{
    ConfigUiFieldState, field_counts_for_tab, field_list_value, field_projected_json_value,
    override_intent_label, render_field_edit_value, visible_rows_for_tab_search,
};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Tabs, Wrap};
use serde_json::Value as JsonValue;
use unicode_segmentation::UnicodeSegmentation;

const HORIZONTAL_PADDING: u16 = 1;
const BODY_PANE_GUTTER: u16 = 1;
const FIELD_TAKES_EFFECT_MIN_WIDTH: usize = 13;
const FIELD_TAKES_EFFECT_MAX_WIDTH: usize = 18;
const FIELD_SETTING_MIN_WIDTH: usize = 8;
const FIELD_SETTING_MAX_WIDTH: usize = 44;
const FIELD_VALUE_MIN_WIDTH: usize = 5;
const HEADER_MIN_PATH_WIDTH: usize = 8;
const HEADER_MIN_SOURCE_LABEL_WIDTH: usize = 4;
const HEADER_SOURCE_LABEL_WIDTH: usize = 18;
const STATUS_COLUMN_WIDTH: usize = 10;
const STATUS_ITEM_COLUMN_WIDTH: usize = 24;

#[derive(Clone, Copy)]
struct ConfigUiThemePalette {
    text: Color,
    muted: Color,
    inactive_tab: Color,
    title: Color,
    accent: Color,
    success: Color,
    error: Color,
    metadata_key: Color,
    config_key: Color,
    border: Color,
    selected_bg: Color,
}

fn config_ui_theme_palette(theme: ConfigUiTheme) -> ConfigUiThemePalette {
    match theme {
        ConfigUiTheme::Dark => ConfigUiThemePalette {
            text: Color::White,
            muted: Color::Gray,
            inactive_tab: Color::Gray,
            title: Color::Cyan,
            accent: Color::Yellow,
            success: Color::Green,
            error: Color::Red,
            metadata_key: Color::LightBlue,
            config_key: Color::LightCyan,
            border: Color::Gray,
            selected_bg: Color::DarkGray,
        },
        ConfigUiTheme::Light => ConfigUiThemePalette {
            text: Color::Black,
            muted: Color::Rgb(62, 68, 78),
            inactive_tab: Color::Black,
            title: Color::Rgb(0, 88, 132),
            accent: Color::Rgb(96, 64, 128),
            success: Color::Rgb(0, 100, 56),
            error: Color::Rgb(160, 32, 32),
            metadata_key: Color::Rgb(32, 76, 132),
            config_key: Color::Rgb(0, 92, 120),
            border: Color::Rgb(88, 100, 118),
            selected_bg: Color::Rgb(214, 224, 238),
        },
    }
}

#[derive(Clone, Copy)]
struct FieldColumnWidths {
    takes_effect: usize,
    setting: usize,
    value: usize,
}

const DEFAULT_FIELD_COLUMN_WIDTHS: FieldColumnWidths = FieldColumnWidths {
    takes_effect: FIELD_TAKES_EFFECT_MAX_WIDTH,
    setting: FIELD_SETTING_MAX_WIDTH,
    value: 28,
};

#[derive(Clone, Copy)]
enum ListLayout<'a> {
    Field(FieldColumnWidths),
    Status,
    Table(&'a ConfigUiListTable),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ListEntry<'a> {
    Section(&'a str),
    Row(UiRowRef),
}

struct HeaderMetadata {
    source_label: Option<String>,
    source_path: String,
    owner: String,
    mode: &'static str,
}

impl ConfigUiApp {
    pub fn render_details(&self, row: UiRowRef) -> Vec<Line<'static>> {
        match self.resolve_row(row) {
            Some(ConfigUiRow::Field(field)) => {
                let mut lines = match self
                    .edit
                    .as_ref()
                    .filter(|edit| field.matches_id(&edit.field_id))
                {
                    Some(edit) if edit.mode == ConfigUiEditMode::Choice => {
                        single_choice_detail_lines(field, edit)
                    }
                    Some(edit) if edit.mode == ConfigUiEditMode::MultiChoice => {
                        multi_choice_detail_lines(field, edit)
                    }
                    _ if matches!(
                        field.capability,
                        ConfigUiCapability::Toggle { .. } | ConfigUiCapability::Choice { .. }
                    ) =>
                    {
                        single_choice_field_detail_lines(field)
                    }
                    _ => field_detail_lines(field),
                };
                if self.can_unset_field(field) {
                    lines.extend(reset_detail_lines(field));
                }
                append_field_diagnostics(&mut lines, &self.model, field);
                lines
            }
            Some(ConfigUiRow::Sidecar(sidecar)) => sidecar_detail_lines(sidecar),
            Some(ConfigUiRow::FileAction(action)) => file_action_detail_lines(action),
            Some(ConfigUiRow::Diagnostic(diagnostic)) => diagnostic_detail_lines(diagnostic),
            Some(ConfigUiRow::NativeStatus(status)) => native_status_detail_lines(status),
            None => Vec::new(),
        }
    }
}

pub fn draw_config_ui(frame: &mut Frame<'_>, app: &mut ConfigUiApp) {
    draw_config_ui_with_details(frame, app, ConfigUiApp::render_details);
}

pub fn draw_config_ui_with_details(
    frame: &mut Frame<'_>,
    app: &mut ConfigUiApp,
    detail_lines: impl Fn(&ConfigUiApp, UiRowRef) -> Vec<Line<'static>>,
) {
    let area = frame.area();
    let [header, tabs, separator, body, footer] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(8),
        Constraint::Length(2),
    ])
    .areas(area);

    render_header(frame, app, header);
    render_tabs(frame, app, tabs);
    render_tab_separator(frame, app.active_theme, separator);
    render_body(frame, app, body, &detail_lines);
    render_footer(frame, app, footer);
}

fn render_header(frame: &mut Frame<'_>, app: &ConfigUiApp, area: Rect) {
    let theme = app.active_theme;
    let metadata = header_metadata(app);
    let warning_count = app
        .model
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.blocking)
        .count();
    let diagnostic_text = if warning_count > 0 {
        warning_count.to_string()
    } else {
        "ok".to_string()
    };
    let diagnostic_style = if warning_count > 0 {
        bold_fg_style(Color::Red)
    } else {
        fg_style(Color::Green)
    };

    let title = Line::from(vec![Span::styled("Config", bold_fg_style(Color::Cyan))]);

    frame.render_widget(
        Block::default()
            .borders(Borders::BOTTOM)
            .border_style(border_style(theme)),
        area,
    );
    let content = Rect {
        height: area.height.saturating_sub(1).max(1),
        ..horizontal_inset(area)
    };
    let title_width = 15_u16.min(content.width);
    let gap = u16::from(content.width > title_width);
    let title_area = Rect {
        x: content.x,
        y: content.y,
        width: title_width,
        height: 1,
    };
    let metadata_area = Rect {
        x: content.x + title_width + gap,
        y: content.y,
        width: content.width.saturating_sub(title_width + gap),
        height: 1,
    };

    frame.render_widget(
        Paragraph::new(themed_line(title, theme)).alignment(Alignment::Left),
        title_area,
    );
    if metadata_area.width > 0 {
        let metadata_line = header_metadata_line(
            &metadata,
            &diagnostic_text,
            diagnostic_style,
            metadata_area.width as usize,
        );
        frame.render_widget(
            Paragraph::new(themed_line(metadata_line, theme)).alignment(Alignment::Right),
            metadata_area,
        );
    }
}

fn header_metadata(app: &ConfigUiApp) -> HeaderMetadata {
    let selected = app.visible_rows().get(app.selected_row).copied();
    match selected.and_then(|row| app.resolve_row(row)) {
        Some(ConfigUiRow::Field(field)) => app
            .model
            .sources
            .iter()
            .find(|source| source.id == field.source_id)
            .map(|source| HeaderMetadata {
                source_label: Some(source.label.clone()),
                source_path: config_path_text(&source.path, source.exists),
                owner: source
                    .owner_label
                    .clone()
                    .unwrap_or_else(|| "none".to_string()),
                mode: write_mode(source.read_only),
            })
            .unwrap_or_else(neutral_header_metadata),
        Some(ConfigUiRow::FileAction(action)) => HeaderMetadata {
            source_label: Some(action.label.clone()),
            source_path: config_path_text(&action.path, action.exists),
            owner: "none".to_string(),
            mode: write_mode(action.read_only),
        },
        _ => neutral_header_metadata(),
    }
}

fn neutral_header_metadata() -> HeaderMetadata {
    HeaderMetadata {
        source_label: None,
        source_path: "not file-backed".to_string(),
        owner: "none".to_string(),
        mode: "n/a",
    }
}

fn config_path_text(path: &std::path::Path, exists: bool) -> String {
    if exists {
        path.display().to_string()
    } else {
        format!("{} (missing)", path.display())
    }
}

fn write_mode(read_only: bool) -> &'static str {
    if read_only { "read-only" } else { "writable" }
}

fn header_metadata_line(
    metadata: &HeaderMetadata,
    diagnostic: &str,
    diagnostic_style: Style,
    width: usize,
) -> Line<'static> {
    let base_width = "path: ".len()
        + "  owner: ".len()
        + metadata.owner.len()
        + "  mode: ".len()
        + metadata.mode.len()
        + "  diag: ".len()
        + diagnostic.len();
    let source_prefix_width = "source: ".len() + "  ".len();
    let source_label = metadata.source_label.as_deref().and_then(|label| {
        let available =
            width.saturating_sub(base_width + source_prefix_width + HEADER_MIN_PATH_WIDTH);
        (available >= HEADER_MIN_SOURCE_LABEL_WIDTH)
            .then(|| truncate(label, available.min(HEADER_SOURCE_LABEL_WIDTH)))
    });
    let fixed_width = base_width
        + source_label
            .as_ref()
            .map(|label| source_prefix_width + label.len())
            .unwrap_or_default();
    let path = truncate_start(&metadata.source_path, width.saturating_sub(fixed_width));
    let mut spans = Vec::new();
    if let Some(label) = source_label {
        spans.push(Span::styled("source: ", metadata_key_style()));
        spans.push(Span::styled(label, metadata_value_style()));
        spans.push(Span::raw("  "));
    }
    spans.extend([
        Span::styled("path: ", metadata_key_style()),
        Span::styled(path, metadata_value_style()),
        Span::raw("  "),
        Span::styled("owner: ", metadata_key_style()),
        Span::styled(metadata.owner.clone(), metadata_value_style()),
        Span::raw("  "),
        Span::styled("mode: ", metadata_key_style()),
        Span::styled(metadata.mode, metadata_value_style()),
        Span::raw("  "),
        Span::styled("diag: ", metadata_key_style()),
        Span::styled(diagnostic.to_string(), diagnostic_style),
    ]);
    Line::from(spans)
}

fn render_tabs(frame: &mut Frame<'_>, app: &ConfigUiApp, area: Rect) {
    let palette = config_ui_theme_palette(app.active_theme);
    frame.render_widget(
        Tabs::new(tab_labels(&app.model.tabs))
            .select(app.selected_tab)
            .style(fg_style(palette.inactive_tab))
            .highlight_style(bold_fg_style(palette.accent)),
        area,
    );
}

fn render_tab_separator(frame: &mut Frame<'_>, theme: ConfigUiTheme, area: Rect) {
    let area = horizontal_inset(area);
    frame.render_widget(
        Paragraph::new("─".repeat(usize::from(area.width))).style(border_style(theme)),
        area,
    );
}

fn tab_labels(tabs: &[String]) -> Vec<Line<'static>> {
    tabs.iter()
        .enumerate()
        .map(|(index, tab)| {
            let label = if index < 9 {
                format!("({}) {tab}", index + 1)
            } else {
                tab.clone()
            };
            Line::from(Span::raw(label))
        })
        .collect()
}

fn render_body(
    frame: &mut Frame<'_>,
    app: &mut ConfigUiApp,
    area: Rect,
    detail_lines: &impl Fn(&ConfigUiApp, UiRowRef) -> Vec<Line<'static>>,
) {
    let [settings_area, divider_area, details_area] = Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(BODY_PANE_GUTTER),
        Constraint::Fill(1),
    ])
    .areas(area);
    frame.render_widget(
        Block::default()
            .borders(Borders::LEFT)
            .border_style(border_style(app.active_theme)),
        divider_area,
    );
    let rows = app.visible_rows();
    app.clamp_selection_for_len(rows.len());
    render_list(frame, app, settings_area, &rows);
    render_details(
        frame,
        app,
        details_area,
        rows.get(app.selected_row).copied(),
        detail_lines,
    );
}

fn render_list(frame: &mut Frame<'_>, app: &ConfigUiApp, area: Rect, rows: &[UiRowRef]) {
    let (title_area, content_area) = pane_areas(area);
    let title = settings_title(app, usize::from(title_area.width));
    frame.render_widget(
        Paragraph::new(themed_line(Line::from(title), app.active_theme)),
        title_area,
    );
    if content_area.height == 0 {
        return;
    }

    let layout = match list_layout(&app.model, app.selected_tab) {
        ListLayout::Field(_) => ListLayout::Field(field_column_widths(
            &app.model,
            app.selected_tab,
            usize::from(content_area.width),
        )),
        layout => layout,
    };
    let entries = list_entries(&app.model, rows);
    let items = entries
        .iter()
        .map(|entry| {
            let line = match entry {
                ListEntry::Section(label) => section_heading_line(label),
                ListEntry::Row(row) => row_line_for_layout(&app.model, *row, layout),
            };
            ListItem::new(themed_line(line, app.active_theme))
        })
        .collect::<Vec<_>>();
    let mut state = ListState::default();
    state.select(selected_list_entry_index(&entries, app.selected_row));
    let [header, rows_area] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(content_area);
    frame.render_widget(
        Paragraph::new(themed_line(list_header_line(layout), app.active_theme))
            .alignment(Alignment::Left),
        header,
    );
    if rows_area.height == 0 {
        return;
    }

    frame.render_stateful_widget(
        List::new(items).highlight_style(selected_row_style(app.active_theme)),
        rows_area,
        &mut state,
    );
}

fn settings_title(app: &ConfigUiApp, width: usize) -> String {
    let counts = field_counts_for_tab(&app.model, app.selected_tab);
    if counts.overview == counts.total {
        return if app.search.is_empty() {
            "settings".to_string()
        } else {
            title_that_fits(
                width,
                format!("settings filtered by {}", app.search),
                format!("search · {}", app.search),
            )
        };
    }

    if !app.search.is_empty() {
        return title_that_fits(
            width,
            format!(
                "settings · search All · Overview {}/{} · {}",
                counts.overview, counts.total, app.search
            ),
            format!("search All · {}", app.search),
        );
    }
    match app.settings_view {
        ConfigUiSettingsView::Overview => title_that_fits(
            width,
            format!("settings · Overview {}/{}", counts.overview, counts.total),
            format!("Overview {}/{}", counts.overview, counts.total),
        ),
        ConfigUiSettingsView::All => title_that_fits(
            width,
            format!("settings All{}/Overview{}", counts.total, counts.overview),
            format!("All {}/Overview {}", counts.total, counts.overview),
        ),
    }
}

fn title_that_fits(width: usize, full: String, compact: String) -> String {
    if Span::raw(full.as_str()).width() <= width {
        full
    } else {
        compact
    }
}

fn list_entries<'a>(model: &'a ConfigUiModel, rows: &[UiRowRef]) -> Vec<ListEntry<'a>> {
    let mut entries = Vec::with_capacity(rows.len());
    let mut previous_section = None;
    for row in rows {
        let section = match row {
            UiRowRef::Field(index) => {
                let label = model.fields[*index].section_label.trim();
                (!label.is_empty()).then_some(label)
            }
            _ => None,
        };
        if section != previous_section
            && let Some(label) = section
        {
            entries.push(ListEntry::Section(label));
        }
        entries.push(ListEntry::Row(*row));
        previous_section = section;
    }
    entries
}

fn selected_list_entry_index(entries: &[ListEntry<'_>], selected_row: usize) -> Option<usize> {
    entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| matches!(entry, ListEntry::Row(_)))
        .nth(selected_row)
        .map(|(index, _)| index)
}

fn section_heading_line(label: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled("── ", fg_style(Color::Gray)),
        Span::styled(label.to_string(), bold_fg_style(Color::Cyan)),
    ])
}

fn list_header_line(layout: ListLayout<'_>) -> Line<'static> {
    match layout {
        ListLayout::Field(widths) => field_list_header_line(widths),
        ListLayout::Status => status_list_header_line(),
        ListLayout::Table(table) => list_table_header_line(table),
    }
}

fn list_layout(model: &ConfigUiModel, selected_tab: usize) -> ListLayout<'_> {
    let tab = &model.tabs[selected_tab];
    if model.operational_tab.as_ref() == Some(tab) {
        ListLayout::Status
    } else {
        model.tab_list_tables.get(tab).map_or(
            ListLayout::Field(DEFAULT_FIELD_COLUMN_WIDTHS),
            ListLayout::Table,
        )
    }
}

fn field_column_widths(
    model: &ConfigUiModel,
    selected_tab: usize,
    available_width: usize,
) -> FieldColumnWidths {
    let rows = visible_rows_for_tab_search(model, selected_tab, "");
    let labels = rows.iter().filter_map(|row| match row {
        UiRowRef::Field(index) => {
            let field = &model.fields[*index];
            Some((
                field.apply_status.summary.as_str(),
                field_display_label(field),
            ))
        }
        UiRowRef::FileAction(index) => {
            let action = &model.file_actions[*index];
            Some((file_action_status_label(action), action.label.as_str()))
        }
        _ => None,
    });
    resolve_field_column_widths(available_width, labels)
}

fn resolve_field_column_widths<'a>(
    available_width: usize,
    rows: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> FieldColumnWidths {
    let mut desired_takes_effect = FIELD_TAKES_EFFECT_MIN_WIDTH;
    let mut desired_setting = FIELD_SETTING_MIN_WIDTH;
    for (takes_effect, setting) in rows {
        desired_takes_effect = desired_takes_effect.max(terminal_width(takes_effect) + 1);
        desired_setting = desired_setting.max(terminal_width(setting) + 1);
    }
    desired_takes_effect = desired_takes_effect.min(FIELD_TAKES_EFFECT_MAX_WIDTH);
    desired_setting = desired_setting.min(FIELD_SETTING_MAX_WIDTH);

    let value_minimum = FIELD_VALUE_MIN_WIDTH.min(available_width.saturating_sub(2));
    let label_space = available_width - value_minimum;
    let setting_floor = usize::from(label_space > 1);
    let takes_effect = desired_takes_effect.min(label_space - setting_floor);
    let setting = desired_setting.min(label_space - takes_effect);
    FieldColumnWidths {
        takes_effect,
        setting,
        value: available_width - takes_effect - setting,
    }
}

fn field_list_header_line(widths: FieldColumnWidths) -> Line<'static> {
    Line::from(vec![
        column_header(field_column_cell("takes effect", widths.takes_effect)),
        column_header(field_column_cell("setting", widths.setting)),
        column_header(truncate_cells("value", widths.value)),
    ])
}

fn status_list_header_line() -> Line<'static> {
    Line::from(vec![
        column_header(status_column_cell("status", STATUS_COLUMN_WIDTH)),
        column_header(status_column_cell("item", STATUS_ITEM_COLUMN_WIDTH)),
        column_header("detail"),
    ])
}

fn render_details(
    frame: &mut Frame<'_>,
    app: &ConfigUiApp,
    area: Rect,
    row: Option<UiRowRef>,
    detail_lines: &impl Fn(&ConfigUiApp, UiRowRef) -> Vec<Line<'static>>,
) {
    let lines = match row {
        Some(row) => detail_lines(app, row),
        None => vec![Line::from(Span::styled(
            empty_settings_message(app),
            fg_style(Color::Gray),
        ))],
    };
    let (title_area, content_area) = pane_areas(area);
    frame.render_widget(
        Paragraph::new(themed_line(Line::from("details"), app.active_theme)),
        title_area,
    );
    frame.render_widget(
        Paragraph::new(themed_lines(lines, app.active_theme)).wrap(Wrap { trim: false }),
        content_area,
    );
}

fn pane_areas(area: Rect) -> (Rect, Rect) {
    let [title, _, content] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(area);
    let title_padding = HORIZONTAL_PADDING.min(title.width);
    let title = Rect {
        x: title.x + title_padding,
        width: title.width.saturating_sub(title_padding),
        ..title
    };
    (title, horizontal_inset(content))
}

fn horizontal_inset(area: Rect) -> Rect {
    let padding = HORIZONTAL_PADDING.min(area.width / 2);
    Rect {
        x: area.x + padding,
        width: area.width.saturating_sub(padding.saturating_mul(2)),
        ..area
    }
}

fn empty_settings_message(app: &ConfigUiApp) -> &'static str {
    if !app.search.is_empty() {
        "No settings match this search."
    } else if app.settings_view == ConfigUiSettingsView::Overview
        && app.selected_tab_has_non_overview_fields()
    {
        if app.search_active {
            "Overview empty. Search All."
        } else {
            "Overview empty. Press a for All."
        }
    } else {
        "No settings on this tab."
    }
}

#[cfg(test)]
pub(crate) fn row_line_for_model(model: &ConfigUiModel, row: UiRowRef) -> Line<'static> {
    row_line_for_layout(model, row, ListLayout::Field(DEFAULT_FIELD_COLUMN_WIDTHS))
}

fn row_line_for_layout(
    model: &ConfigUiModel,
    row: UiRowRef,
    layout: ListLayout<'_>,
) -> Line<'static> {
    match (row, layout) {
        (UiRowRef::Field(index), ListLayout::Table(table)) => {
            let field = &model.fields[index];
            list_table_row_line(table, field, model.field_state(field))
        }
        (UiRowRef::Field(index), layout) => {
            let field = &model.fields[index];
            let state = model.field_state(field);
            let value = field_list_value(field);
            let widths = match layout {
                ListLayout::Field(widths) => widths,
                _ => DEFAULT_FIELD_COLUMN_WIDTHS,
            };
            field_row_line(
                widths,
                &field.apply_status.summary,
                apply_status_style(&field.apply_status),
                field_display_label(field),
                field_style(state, config_key_style()),
                &value,
                field_style(state, fg_style(Color::Gray)),
            )
        }
        (UiRowRef::Sidecar(index), _) => {
            let sidecar = &model.sidecars[index];
            status_row_line(
                sidecar_status_label(sidecar.present),
                sidecar_status_style(sidecar.present),
                &sidecar.name,
                sidecar.path.display().to_string(),
            )
        }
        (UiRowRef::FileAction(index), ListLayout::Status) => {
            let action = &model.file_actions[index];
            status_row_line(
                file_action_status_label(action),
                file_action_status_style(action),
                &action.label,
                action.path.display().to_string(),
            )
        }
        (UiRowRef::FileAction(index), layout) => {
            let action = &model.file_actions[index];
            let widths = match layout {
                ListLayout::Field(widths) => widths,
                _ => DEFAULT_FIELD_COLUMN_WIDTHS,
            };
            field_row_line(
                widths,
                file_action_status_label(action),
                file_action_status_style(action),
                &action.label,
                config_key_style(),
                &action.path.display().to_string(),
                fg_style(Color::Gray),
            )
        }
        (UiRowRef::Diagnostic(index), _) => {
            let diagnostic = &model.diagnostics[index];
            let style = if diagnostic.blocking {
                fg_style(Color::Red)
            } else {
                fg_style(Color::Yellow)
            };
            status_row_line(
                &diagnostic.status,
                style,
                &diagnostic.path,
                diagnostic.headline.as_str(),
            )
        }
        (UiRowRef::NativeStatus(index), _) => {
            let status = &model.native_config_statuses[index];
            status_row_line(
                &status.status,
                native_status_style(status),
                &status.surface,
                status.label.as_str(),
            )
        }
    }
}

fn list_table_header_line(table: &ConfigUiListTable) -> Line<'static> {
    table
        .columns
        .iter()
        .map(|column| column_header(list_table_cell(&column.title, column.width)))
        .collect()
}

fn list_table_row_line(
    table: &ConfigUiListTable,
    field: &ConfigUiField,
    state: ConfigUiFieldState,
) -> Line<'static> {
    table
        .columns
        .iter()
        .enumerate()
        .map(|(index, column)| {
            let cell = field.list_cells.get(index).map_or("", String::as_str);
            Span::styled(
                list_table_cell(cell, column.width),
                list_table_cell_style(field, state, index),
            )
        })
        .collect()
}

fn list_table_cell_style(
    field: &ConfigUiField,
    state: ConfigUiFieldState,
    column_index: usize,
) -> Style {
    let default = match column_index {
        0 => apply_status_style(&field.apply_status),
        1 => config_key_style(),
        2 => metadata_value_style(),
        _ => fg_style(Color::Gray),
    };
    field_style(state, default)
}

fn list_table_cell(value: &str, width: usize) -> String {
    fixed_label(&truncate(value, width), width)
}

fn column_header(value: impl Into<String>) -> Span<'static> {
    Span::styled(value.into(), column_header_style())
}

fn field_row_line(
    widths: FieldColumnWidths,
    status: &str,
    status_style: Style,
    setting: &str,
    setting_style: Style,
    value: &str,
    value_style: Style,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(field_column_cell(status, widths.takes_effect), status_style),
        Span::styled(field_column_cell(setting, widths.setting), setting_style),
        Span::styled(truncate_cells(value, widths.value), value_style),
    ])
}

fn field_column_cell(value: &str, width: usize) -> String {
    let value = truncate_cells(value, width.saturating_sub(1));
    let padding = width.saturating_sub(terminal_width(&value));
    value + &" ".repeat(padding)
}

fn truncate_cells(value: &str, limit: usize) -> String {
    if terminal_width(value) <= limit {
        return value.to_string();
    }
    if limit <= 3 {
        return ".".repeat(limit);
    }

    let mut prefix = String::new();
    let mut width = 0;
    let span = Span::raw(value);
    for grapheme in span.styled_graphemes(Style::default()) {
        let grapheme_width = terminal_width(grapheme.symbol);
        if width + grapheme_width > limit - 3 {
            break;
        }
        prefix.push_str(grapheme.symbol);
        width += grapheme_width;
    }
    prefix + "..."
}

fn terminal_width(value: &str) -> usize {
    Span::raw(value).width()
}

fn status_row_line(
    status: &str,
    status_style: Style,
    item: &str,
    detail: impl Into<String>,
) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            status_column_cell(status, STATUS_COLUMN_WIDTH),
            status_style,
        ),
        Span::styled(
            status_column_cell(item, STATUS_ITEM_COLUMN_WIDTH),
            config_key_style(),
        ),
        Span::styled(detail.into(), fg_style(Color::Gray)),
    ])
}

fn status_column_cell(value: &str, width: usize) -> String {
    format!("{:<width$}", truncate(value, width.saturating_sub(1)))
}

fn field_display_label(field: &ConfigUiField) -> &str {
    if field.display_label.is_empty() {
        &field.path
    } else {
        &field.display_label
    }
}

fn styled_line(text: impl Into<String>, style: Style) -> Line<'static> {
    Line::from(Span::styled(text.into(), style))
}

fn field_title_lines(field: &ConfigUiField) -> Vec<Line<'static>> {
    let mut lines = vec![styled_line(
        field_display_label(field).to_string(),
        config_key_style().add_modifier(Modifier::BOLD),
    )];
    if !field.display_label.is_empty() && field.display_label != field.path {
        lines.push(detail_line("path", &field.path));
    }
    lines.push(Line::from(""));
    lines
}

fn field_style(state: ConfigUiFieldState, default: Style) -> Style {
    if state == ConfigUiFieldState::Invalid {
        state_style(state)
    } else {
        default
    }
}

fn append_field_diagnostics(
    lines: &mut Vec<Line<'static>>,
    model: &ConfigUiModel,
    field: &ConfigUiField,
) {
    for diagnostic in model.diagnostics.iter().filter(|diagnostic| {
        matches!(
            &diagnostic.scope,
            ConfigUiDiagnosticScope::Field(identity) if field.matches_id(identity)
        )
    }) {
        lines.push(Line::from(""));
        lines.extend(diagnostic_detail_lines(diagnostic));
    }
}

/// Renders stored sparse field state; use [`ConfigUiApp::render_details`] for reset availability
/// and scoped diagnostics.
pub fn default_field_detail_lines(field: &ConfigUiField) -> Vec<Line<'static>> {
    field_detail_lines(field)
}

fn field_detail_lines(field: &ConfigUiField) -> Vec<Line<'static>> {
    let mut lines = field_title_lines(field);
    lines.push(detail_line(
        "intent",
        override_intent_label(&field.snapshot.intent),
    ));
    let override_value = match &field.snapshot.intent {
        ConfigUiOverride::Absent => None,
        ConfigUiOverride::Explicit(value) => {
            lines.extend(json_detail_lines("override", field, value));
            Some(value)
        }
        ConfigUiOverride::Invalid { input } => {
            let value = field_detail_value(field, input, None);
            lines.extend(field_detail_value_lines("input", &value));
            None
        }
    };
    if let Some(effective) = &field.snapshot.effective {
        lines.extend(resolved_detail_lines(
            "effective",
            "eff. origin",
            field,
            effective,
            (override_value == Some(&effective.value)).then_some("same as override"),
        ));
    }
    if let Some(baseline) = &field.snapshot.baseline {
        let same_as = field
            .snapshot
            .effective
            .as_ref()
            .filter(|effective| effective.value == baseline.value)
            .map(|_| "same as effective")
            .or_else(|| (override_value == Some(&baseline.value)).then_some("same as override"));
        lines.extend(resolved_detail_lines(
            "baseline",
            "base origin",
            field,
            baseline,
            same_as,
        ));
    }
    if let Some(manager) = &field.snapshot.external_manager {
        lines.push(detail_line("managed by", manager));
    }
    if let Some(type_label) = &field.type_label {
        lines.push(detail_line("type", type_label));
    }
    lines.extend([
        detail_line("takes effect", &field.apply_status.label),
        detail_line("after save", &field.apply_status.detail),
    ]);
    if !field.validation.is_empty() {
        lines.push(detail_line("validation", &field.validation));
    }
    if matches!(field.capability, ConfigUiCapability::MultiChoice { .. }) {
        lines.push(detail_line(
            "allowed",
            &capability_choices(field)
                .into_iter()
                .map(ConfigUiChoice::display_label)
                .collect::<Vec<_>>()
                .join(", "),
        ));
    }
    if field.rebuild_required {
        lines.push(detail_line("rebuild", "required"));
    }
    if !field.description.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(field.description.clone()));
    }
    lines
}

fn json_detail_lines(label: &str, field: &ConfigUiField, value: &JsonValue) -> Vec<Line<'static>> {
    let rendered = render_field_edit_value(field, value);
    field_detail_value_lines(label, &field_detail_value(field, &rendered, Some(value)))
}

fn resolved_detail_lines(
    label: &str,
    origin_label: &str,
    field: &ConfigUiField,
    resolved: &ConfigUiResolvedValue,
    same_as: Option<&str>,
) -> Vec<Line<'static>> {
    let mut lines = same_as.map_or_else(
        || json_detail_lines(label, field, &resolved.value),
        |same_as| vec![detail_line(label, same_as)],
    );
    if let Some(origin) = &resolved.origin {
        lines.push(detail_line(origin_label, origin));
    }
    lines
}

fn reset_detail_lines(field: &ConfigUiField) -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        detail_line("reset", "remove override"),
        detail_line(
            "inherits",
            field
                .snapshot
                .baseline
                .as_ref()
                .map_or("unknown", |_| "baseline"),
        ),
    ]
}

#[derive(PartialEq)]
enum FieldDetailValue<'a> {
    Scalar(&'a str),
    Structured(JsonValue),
    StructuredFallback(&'a str),
}

fn structured_json(value: &str) -> Option<JsonValue> {
    serde_json::from_str(value)
        .ok()
        .filter(|value| matches!(value, JsonValue::Array(_) | JsonValue::Object(_)))
}

fn field_detail_value<'a>(
    field: &ConfigUiField,
    fallback: &'a str,
    preferred_json: Option<&JsonValue>,
) -> FieldDetailValue<'a> {
    let structured = preferred_json
        .is_some_and(|value| matches!(value, JsonValue::Array(_) | JsonValue::Object(_)))
        || matches!(
            field.type_label.as_deref(),
            Some("array" | "object" | "string_list" | "string list" | "table")
        );
    if !structured {
        return FieldDetailValue::Scalar(fallback);
    }

    match preferred_json
        .filter(|value| matches!(value, JsonValue::Array(_) | JsonValue::Object(_)))
        .cloned()
        .or_else(|| structured_json(fallback))
    {
        Some(value) => FieldDetailValue::Structured(value),
        None => FieldDetailValue::StructuredFallback(fallback),
    }
}

fn field_detail_value_lines(label: &str, value: &FieldDetailValue<'_>) -> Vec<Line<'static>> {
    let value = match value {
        FieldDetailValue::Scalar(value) => return vec![detail_line(label, value)],
        FieldDetailValue::Structured(value) => {
            serde_json::to_string_pretty(value).expect("serde_json::Value serializes")
        }
        FieldDetailValue::StructuredFallback(value) => value.to_string(),
    };
    let mut lines = vec![detail_line(label, "")];
    lines.extend(
        value
            .lines()
            .map(|line| styled_line(format!("  {line}"), metadata_value_style())),
    );
    lines
}

/// Renders stored choice state; use [`ConfigUiApp::render_details`] for scoped diagnostics.
pub fn single_choice_field_detail_lines(field: &ConfigUiField) -> Vec<Line<'static>> {
    let selected_value = field_projected_json_value(field);
    let mut lines = field_detail_lines(field);
    lines.push(Line::from(""));
    append_single_choice_options(&mut lines, field, selected_value, None);
    lines
}

pub fn single_choice_detail_lines(
    field: &ConfigUiField,
    edit: &ConfigUiEditState,
) -> Vec<Line<'static>> {
    let selected_value = serde_json::from_str::<JsonValue>(&edit.input).ok();
    let mut lines = field_title_lines(field);
    lines.push(detail_line(
        "selected",
        &selected_value
            .as_ref()
            .and_then(|value| choice_label_for_value(field, value))
            .unwrap_or_else(|| "none".to_string()),
    ));
    lines.push(Line::from(""));

    append_single_choice_options(
        &mut lines,
        field,
        selected_value.as_ref(),
        is_choice_picker(field).then_some(edit.choice_index),
    );

    lines
}

fn append_single_choice_options(
    lines: &mut Vec<Line<'static>>,
    field: &ConfigUiField,
    selected_value: Option<&JsonValue>,
    highlighted_index: Option<usize>,
) {
    for (index, choice) in capability_choices(field).into_iter().enumerate() {
        let highlighted = highlighted_index == Some(index);
        let selected = selected_value == Some(&choice.value);
        lines.push(choice_option_line(
            &choice.display_label(),
            highlighted,
            selected,
            ("(x) ", "( ) "),
        ));
    }
}

pub fn multi_choice_detail_lines(
    field: &ConfigUiField,
    edit: &ConfigUiEditState,
) -> Vec<Line<'static>> {
    let enabled_values = serde_json::from_str::<Vec<JsonValue>>(&edit.input).unwrap_or_default();
    let mut lines = field_title_lines(field);
    lines.push(detail_line(
        "enabled",
        &format!(
            "{}/{}",
            enabled_values.len(),
            capability_choices(field).len()
        ),
    ));
    if is_ordered_multi_choice_field(field) {
        lines.push(detail_line(
            "order",
            &multi_choice_order_label(field, &enabled_values),
        ));
    }
    lines.push(Line::from(""));

    let choices = edit_choices(field, &edit.input);
    for (index, choice) in choices.iter().enumerate() {
        let selected = index == edit.choice_index.min(choices.len().saturating_sub(1));
        let enabled = enabled_values.contains(&choice.value);
        lines.push(choice_option_line(
            &choice.display_label(),
            selected,
            enabled,
            ("[x] ", "[ ] "),
        ));
    }

    lines
}

fn choice_option_line(
    value: &str,
    focused: bool,
    enabled: bool,
    markers: (&'static str, &'static str),
) -> Line<'static> {
    let cursor_style = if focused {
        bold_fg_style(Color::Yellow)
    } else {
        fg_style(Color::Gray)
    };
    let marker_style = if enabled {
        bold_fg_style(Color::Green)
    } else {
        fg_style(Color::Gray)
    };
    let value_style = if focused {
        bold_fg_style(Color::White)
    } else if enabled {
        fg_style(Color::White)
    } else {
        fg_style(Color::Gray)
    };
    Line::from(vec![
        Span::styled(if focused { "> " } else { "  " }, cursor_style),
        Span::styled(if enabled { markers.0 } else { markers.1 }, marker_style),
        Span::styled(value.to_string(), value_style),
    ])
}

pub fn sidecar_detail_lines(sidecar: &ConfigUiSidecar) -> Vec<Line<'static>> {
    vec![
        styled_line(sidecar.name.clone(), bold_fg_style(Color::Cyan)),
        Line::from(""),
        detail_line("path", &sidecar.path.display().to_string()),
        detail_line("state", sidecar_status_label(sidecar.present)),
        detail_line("owner", sidecar.owner_label.as_deref().unwrap_or("none")),
        detail_line("write", write_detail_label(sidecar.read_only)),
    ]
}

pub fn file_action_detail_lines(action: &ConfigUiFileAction) -> Vec<Line<'static>> {
    let mut lines = vec![
        styled_line(
            action.label.clone(),
            config_key_style().add_modifier(Modifier::BOLD),
        ),
        Line::from(""),
        detail_line("source", &action.source_id),
        detail_line("action", &action.action_id),
        detail_line("path", &action.path.display().to_string()),
        detail_line("state", file_action_status_label(action)),
        detail_line("write", write_detail_label(action.read_only)),
        detail_line(
            "create",
            if action.create_if_missing {
                "offered when absent"
            } else {
                "not offered"
            },
        ),
    ];
    if let Some(reason) = &action.disabled_reason {
        lines.push(detail_line("disabled", reason));
    }
    if !action.description.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(action.description.clone()));
    }
    lines
}

fn write_detail_label(read_only: bool) -> &'static str {
    if read_only {
        "read-only"
    } else {
        "writable or absent"
    }
}

pub fn diagnostic_detail_lines(diagnostic: &ConfigUiDiagnostic) -> Vec<Line<'static>> {
    let mut lines = vec![
        styled_line(
            diagnostic.headline.clone(),
            bold_fg_style(if diagnostic.blocking {
                Color::Red
            } else {
                Color::Yellow
            }),
        ),
        Line::from(""),
        detail_line("path", &diagnostic.path),
        detail_line("status", &diagnostic.status),
        detail_line("blocking", if diagnostic.blocking { "yes" } else { "no" }),
    ];
    match &diagnostic.scope {
        ConfigUiDiagnosticScope::Global => lines.push(detail_line("scope", "global")),
        ConfigUiDiagnosticScope::Source { source_id } => {
            lines.push(detail_line("scope", "source"));
            lines.push(detail_line("source", source_id));
        }
        ConfigUiDiagnosticScope::Field(identity) => {
            lines.push(detail_line("scope", "field"));
            lines.push(detail_line("source", &identity.source_id));
            lines.push(detail_line("field", &identity.path));
        }
    }
    lines.push(Line::from(""));
    for detail in &diagnostic.detail_lines {
        lines.push(Line::from(detail.clone()));
    }
    lines
}

pub fn native_status_detail_lines(status: &ConfigUiNativeStatus) -> Vec<Line<'static>> {
    let mut lines = vec![
        styled_line(status.label.clone(), bold_fg_style(Color::Cyan)),
        Line::from(""),
        detail_line("surface", &status.surface),
        detail_line("tool", &status.tool),
        detail_line("status", &status.status),
        detail_line("description", &status.description),
        detail_line("allowed action", &status.allowed_action),
    ];
    if let Some(path) = &status.active_path {
        lines.push(detail_line("active path", path));
    }
    if let Some(path) = &status.managed_path {
        lines.push(detail_line("managed path", path));
    }
    if !status.native_paths.is_empty() {
        lines.push(detail_line("native paths", &status.native_paths.join(", ")));
    }
    if let Some(path) = &status.generated_path {
        lines.push(detail_line("generated path", path));
    }
    if let Some(reason) = &status.read_only_reason {
        lines.push(detail_line("read-only", reason));
    }
    lines
}

fn render_footer(frame: &mut Frame<'_>, app: &ConfigUiApp, area: Rect) {
    if let Some(edit) = &app.edit {
        let Some(field) = app.active_edit_field() else {
            return;
        };
        let editing = edit_status_line(field, edit, area.width as usize);
        let status = app.notice.as_ref().map_or_else(
            || edit_control_line(field, edit.mode),
            |notice| notice_line(notice, area.width as usize),
        );
        frame.render_widget(
            Paragraph::new(themed_lines(vec![editing, status], app.active_theme)),
            area,
        );
        return;
    }

    let notice = app.notice.as_ref().map_or_else(
        || normal_control_line(app),
        |notice| notice_line(notice, area.width as usize),
    );
    let controls = footer_control_line(app);
    frame.render_widget(
        Paragraph::new(themed_lines(vec![notice, controls], app.active_theme)),
        area,
    );
}

fn footer_control_line(app: &ConfigUiApp) -> Line<'static> {
    let search = if app.search_active {
        format!("search: {}_", app.search)
    } else if app.search.is_empty() {
        "/ search".to_string()
    } else {
        "Esc clears search".to_string()
    };
    let mut spans = vec![Span::raw("q quit ")];
    if app.can_toggle_settings_view() {
        let target = match app.settings_view {
            ConfigUiSettingsView::Overview => "All",
            ConfigUiSettingsView::All => "Overview",
        };
        spans.push(Span::styled(
            format!("a {target} "),
            fg_style(Color::Yellow),
        ));
    }
    spans.extend([
        Span::raw("Tab tabs "),
        Span::raw("j/k move "),
        Span::styled(search, fg_style(Color::Yellow)),
    ]);
    Line::from(spans)
}

fn notice_line(notice: &ConfigUiNotice, width: usize) -> Line<'static> {
    let style = if notice.is_error {
        bold_fg_style(Color::Red)
    } else {
        fg_style(Color::Green)
    };
    Line::from(Span::styled(truncate(&notice.text, width), style))
}

fn edit_status_line(
    field: &ConfigUiField,
    edit: &ConfigUiEditState,
    width: usize,
) -> Line<'static> {
    const MIN_VALUE_WIDTH: usize = 8;
    const MIN_PATH_WIDTH: usize = 4;
    let full_framing_width = terminal_width("editing: ") + terminal_width(" = ");
    let (label, path, separator) = if width >= full_framing_width + MIN_PATH_WIDTH + MIN_VALUE_WIDTH
    {
        (
            "editing: ",
            truncate_cells(
                &field.path,
                width.saturating_sub(full_framing_width + MIN_VALUE_WIDTH),
            ),
            " = ",
        )
    } else {
        ("", String::new(), "")
    };
    let value_width = width
        .saturating_sub(terminal_width(label) + terminal_width(&path) + terminal_width(separator));
    let value = match edit.mode {
        ConfigUiEditMode::Text => text_edit_window(&edit.input, edit.cursor, value_width),
        ConfigUiEditMode::Choice => single_choice_status_value(field, edit),
        ConfigUiEditMode::MultiChoice => multi_choice_status_value(field, edit),
    };
    let value = truncate_cells(&value, value_width);
    Line::from(vec![
        Span::styled(label, fg_style(Color::Yellow)),
        Span::styled(path, config_key_style()),
        Span::raw(separator),
        Span::styled(value, fg_style(Color::White)),
    ])
}

fn text_edit_window(input: &str, cursor: usize, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let cursor = crate::editor::grapheme_boundary_at_or_before(input, cursor);
    let graphemes = input.graphemes(true).collect::<Vec<_>>();
    let cursor_index = input[..cursor].graphemes(true).count();
    let content_width = width - 1;
    let mut start = cursor_index;
    let mut end = cursor_index;
    let mut used = 0;

    while start > 0 {
        let candidate = terminal_width(graphemes[start - 1]);
        if used + candidate > content_width / 2 {
            break;
        }
        start -= 1;
        used += candidate;
    }
    while end < graphemes.len() {
        let candidate = terminal_width(graphemes[end]);
        if used + candidate > content_width {
            break;
        }
        end += 1;
        used += candidate;
    }
    while start > 0 {
        let candidate = terminal_width(graphemes[start - 1]);
        if used + candidate > content_width {
            break;
        }
        start -= 1;
        used += candidate;
    }

    let before = graphemes[start..cursor_index].concat();
    let after = graphemes[cursor_index..end].concat();
    format!("{before}_{after}")
}

fn normal_control_line(app: &ConfigUiApp) -> Line<'static> {
    if let Some((_, action)) = app.selected_file_action() {
        return Line::from(if action.disabled_reason.is_some() {
            "file action unavailable"
        } else {
            "Enter/e/Space open file"
        });
    }
    let Some(field) = app.selected_field() else {
        return Line::from("Select a setting row to edit");
    };
    let mut primary = match &field.capability {
        ConfigUiCapability::ReadOnly { reason, .. } => {
            match app.selected_capability_file_action() {
                Some((_, action)) if action.disabled_reason.is_none() => {
                    format!("e open {}", action.label)
                }
                Some(_) => "file action unavailable".to_string(),
                None => format!("read-only: {reason}"),
            }
        }
        ConfigUiCapability::Toggle { .. } => "Space stage  e edit".to_string(),
        ConfigUiCapability::Choice { .. } | ConfigUiCapability::MultiChoice { .. } => {
            "Enter/e/Space picker".to_string()
        }
        ConfigUiCapability::FreeText { .. } => "Enter inline  e editor".to_string(),
    };
    if app.can_unset_field(field) {
        primary.push_str("  u inherit");
    }
    Line::from(primary)
}

fn edit_control_line(field: &ConfigUiField, mode: ConfigUiEditMode) -> Line<'static> {
    match mode {
        ConfigUiEditMode::Text => raw_line([
            "Enter save  Esc cancel  ",
            "←/→ move  Home/End  Del/Backspace  ",
            "Ctrl+u clear  ",
            "Ctrl+e editor",
        ]),
        ConfigUiEditMode::Choice if is_choice_picker(field) => raw_line([
            "hjkl/Arrows move  ",
            "Space select  ",
            "Enter save  ",
            "Esc cancel",
        ]),
        ConfigUiEditMode::Choice => raw_line(["Space toggle  ", "Enter save  ", "Esc cancel"]),
        ConfigUiEditMode::MultiChoice if is_ordered_multi_choice_field(field) => raw_line([
            "hjkl/Arrows move  ",
            "Space toggle  ",
            "J/K reorder  ",
            "Enter save  ",
            "Esc cancel",
        ]),
        ConfigUiEditMode::MultiChoice => raw_line([
            "hjkl/Arrows move  ",
            "Space enable/disable  ",
            "Enter save  ",
            "Esc cancel",
        ]),
    }
}

fn raw_line<const N: usize>(parts: [&'static str; N]) -> Line<'static> {
    parts.into_iter().map(Span::raw).collect()
}

fn themed_lines(lines: Vec<Line<'static>>, theme: ConfigUiTheme) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|line| themed_line(line, theme))
        .collect()
}

fn themed_line(mut line: Line<'static>, theme: ConfigUiTheme) -> Line<'static> {
    line.style = themed_style(line.style, theme);
    line.spans = line
        .spans
        .into_iter()
        .map(|mut span| {
            span.style = themed_style(span.style, theme);
            span
        })
        .collect();
    line
}

fn themed_style(mut style: Style, theme: ConfigUiTheme) -> Style {
    if theme == ConfigUiTheme::Dark {
        return style;
    }
    let palette = config_ui_theme_palette(theme);
    style.fg = Some(style.fg.map_or(palette.text, |color| {
        light_foreground_for_dark_role(color, palette)
    }));
    style
}

fn light_foreground_for_dark_role(color: Color, palette: ConfigUiThemePalette) -> Color {
    match color {
        Color::White | Color::Black => palette.text,
        Color::Gray | Color::DarkGray => palette.muted,
        Color::Cyan => palette.title,
        Color::LightBlue => palette.metadata_key,
        Color::LightCyan => palette.config_key,
        Color::Yellow => palette.accent,
        Color::Green => palette.success,
        Color::Red => palette.error,
        _ => color,
    }
}

fn selected_row_style(theme: ConfigUiTheme) -> Style {
    Style::default()
        .bg(config_ui_theme_palette(theme).selected_bg)
        .add_modifier(Modifier::BOLD)
}

fn border_style(theme: ConfigUiTheme) -> Style {
    fg_style(config_ui_theme_palette(theme).border)
}

pub(crate) fn state_style(state: ConfigUiFieldState) -> Style {
    match state {
        ConfigUiFieldState::Explicit => fg_style(Color::Green),
        ConfigUiFieldState::Inherited => fg_style(Color::Cyan),
        ConfigUiFieldState::Absent => fg_style(Color::Yellow),
        ConfigUiFieldState::Invalid => bold_fg_style(Color::Red),
    }
}

pub fn apply_status_style(status: &ConfigUiApplyStatus) -> Style {
    if status.pending {
        fg_style(Color::Yellow)
    } else {
        fg_style(Color::Green)
    }
}

pub fn sidecar_status_style(present: bool) -> Style {
    if present {
        fg_style(Color::Green)
    } else {
        fg_style(Color::Gray)
    }
}

fn sidecar_status_label(present: bool) -> &'static str {
    if present { "present" } else { "absent" }
}

pub fn file_action_status_label(action: &ConfigUiFileAction) -> &'static str {
    if action.read_only {
        "read-only"
    } else if action.disabled_reason.is_some() {
        "error"
    } else if action.exists {
        "existing"
    } else {
        "absent"
    }
}

pub fn file_action_status_style(action: &ConfigUiFileAction) -> Style {
    match file_action_status_label(action) {
        "error" => bold_fg_style(Color::Red),
        "existing" => fg_style(Color::Green),
        "absent" => fg_style(Color::Gray),
        _ => fg_style(Color::Yellow),
    }
}

pub fn native_status_style(status: &ConfigUiNativeStatus) -> Style {
    match status.severity.as_str() {
        "error" => bold_fg_style(Color::Red),
        "warning" => fg_style(Color::Yellow),
        "ok" => fg_style(Color::Green),
        _ => fg_style(Color::Cyan),
    }
}

pub fn column_header_style() -> Style {
    bold_fg_style(Color::Yellow)
}

pub fn metadata_key_style() -> Style {
    fg_style(Color::LightBlue)
}

pub fn metadata_value_style() -> Style {
    fg_style(Color::White)
}

pub fn config_key_style() -> Style {
    fg_style(Color::LightCyan)
}

fn fg_style(color: Color) -> Style {
    Style::default().fg(color)
}

fn bold_fg_style(color: Color) -> Style {
    fg_style(color).add_modifier(Modifier::BOLD)
}

pub fn fixed_label(value: &str, width: usize) -> String {
    let label = format!("{value:<width$}");
    if label.ends_with(' ') {
        label
    } else {
        format!("{label} ")
    }
}

pub fn detail_line(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(fixed_label(label, 11), metadata_key_style()),
        Span::styled(value.to_string(), metadata_value_style()),
    ])
}

pub fn truncate(value: &str, limit: usize) -> String {
    if value.chars().count() <= limit {
        return value.to_string();
    }
    if limit <= 3 {
        return ".".repeat(limit);
    }
    value.chars().take(limit - 3).collect::<String>() + "..."
}

pub fn truncate_start(value: &str, limit: usize) -> String {
    let len = value.chars().count();
    if len <= limit {
        return value.to_string();
    }
    if limit <= 3 {
        return ".".repeat(limit);
    }
    let tail = value
        .chars()
        .skip(len.saturating_sub(limit - 3))
        .collect::<String>();
    format!("...{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{apply_status, field, field_with_source, model_with_fields};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::{Buffer, Cell};
    use serde_json::json;
    use std::path::PathBuf;

    fn rendered_cells(line: &Line<'_>) -> Vec<String> {
        line.spans
            .iter()
            .map(|span| span.content.trim().to_string())
            .collect()
    }

    fn rendered_text(line: &Line<'_>) -> String {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect()
    }

    fn rendered_lines(lines: &[Line<'_>]) -> String {
        lines
            .iter()
            .map(rendered_text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn render_app(terminal: &mut Terminal<TestBackend>, app: &mut ConfigUiApp) -> String {
        terminal
            .draw(|frame| draw_config_ui(frame, app))
            .expect("render config UI");
        let buffer = terminal.backend().buffer();
        buffer.content().iter().map(Cell::symbol).collect()
    }

    fn span_width(line: &Line<'_>, index: usize) -> usize {
        line.spans[index].width()
    }

    fn rendered_cell<'a>(buffer: &'a Buffer, text: &str) -> &'a Cell {
        for y in buffer.area.y..buffer.area.bottom() {
            for x in buffer.area.x..buffer.area.right() {
                let suffix = (x..buffer.area.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>();
                if suffix.starts_with(text) {
                    return &buffer[(x, y)];
                }
            }
        }
        panic!("{text:?} was not rendered");
    }

    fn contrast_ratio(foreground: Color, background: Color) -> f64 {
        fn luminance(color: Color) -> f64 {
            let (red, green, blue) = match color {
                Color::Black => (0, 0, 0),
                Color::White => (255, 255, 255),
                Color::Rgb(red, green, blue) => (red, green, blue),
                color => panic!("light theme role uses terminal-defined color {color:?}"),
            };
            let channel = |value: u8| {
                let value = f64::from(value) / 255.0;
                if value <= 0.04045 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(red) + 0.7152 * channel(green) + 0.0722 * channel(blue)
        }

        let foreground = luminance(foreground);
        let background = luminance(background);
        (foreground.max(background) + 0.05) / (foreground.min(background) + 0.05)
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn built_field(
        kind: &str,
        current: Option<&serde_json::Value>,
        default: Option<&serde_json::Value>,
    ) -> ConfigUiField {
        ConfigUiFieldSpec::new(
            "settings",
            "ui.test",
            "general",
            "",
            ConfigUiCapability::ReadOnly {
                reason: "Test field is read-only.".to_string(),
                file_action_id: None,
            },
            "",
            apply_status("after save", "Applied after saving."),
        )
        .build(kind, current, default)
    }

    fn detail_text(field: &ConfigUiField) -> String {
        rendered_lines(&default_field_detail_lines(field))
    }

    fn set_list_table(model: &mut ConfigUiModel, tab: &str, columns: &[(&str, usize)]) {
        model.tab_list_tables.insert(
            tab.to_string(),
            ConfigUiListTable {
                columns: columns
                    .iter()
                    .map(|(title, width)| ConfigUiListColumn {
                        title: title.to_string(),
                        width: *width,
                    })
                    .collect(),
            },
        );
    }

    fn test_model(state: ConfigUiFieldState) -> ConfigUiModel {
        let mut field = field("core.debug_mode", "bool", "false", &[]);
        crate::model::set_field_state_for_test(&mut field, state);
        field.apply_status = apply_status("after app restart", "Restart the app after saving");
        model_with_fields(vec![field])
    }

    // Defends: tab numbering is presentation-only and stops after the ninth host-owned label.
    #[test]
    fn tab_labels_show_first_nine_shortcuts() {
        let tabs = (1..=10)
            .map(|index| format!("tab_{index}"))
            .collect::<Vec<_>>();
        let labels = tab_labels(&tabs)
            .iter()
            .map(rendered_text)
            .collect::<Vec<_>>();

        assert_eq!(labels[0], "(1) tab_1");
        assert_eq!(labels[8], "(9) tab_9");
        assert_eq!(labels[9], "tab_10");
    }

    // Defends: the public detail resolver treats caller-fabricated row references as unavailable
    // instead of indexing beyond a validated model.
    #[test]
    fn detail_resolver_is_total_for_unknown_rows() {
        let app = ConfigUiApp::new(test_model(ConfigUiFieldState::Explicit));

        assert!(matches!(
            app.resolve_row(UiRowRef::Field(0)),
            Some(ConfigUiRow::Field(field)) if field.path == "core.debug_mode"
        ));

        for row in [
            UiRowRef::Field(usize::MAX),
            UiRowRef::FileAction(usize::MAX),
            UiRowRef::Sidecar(usize::MAX),
            UiRowRef::NativeStatus(usize::MAX),
            UiRowRef::Diagnostic(usize::MAX),
        ] {
            assert!(app.resolve_row(row).is_none());
            assert!(app.render_details(row).is_empty());
        }
    }

    // Defends: narrow layouts expose the active view, honest counts, and the matching toggle without hiding search scope.
    #[test]
    fn overview_all_view_state_and_controls_remain_clear_in_a_narrow_ui() {
        let mut recommended = field("core.visible", "string", r#""core""#, &[]);
        crate::model::set_field_state_for_test(&mut recommended, ConfigUiFieldState::Inherited);
        let mut hidden = field("hidden.secret", "string", r#""secret""#, &[]);
        crate::model::set_field_state_for_test(&mut hidden, ConfigUiFieldState::Inherited);
        let mut model = model_with_fields(vec![recommended, hidden]);
        model.recommended_fields = Some(vec![ConfigUiFieldId::new(
            DEFAULT_CONFIG_SOURCE_ID,
            "core.visible",
        )]);
        let mut app = ConfigUiApp::new(model);
        let mut terminal = Terminal::new(TestBackend::new(46, 16)).expect("test terminal");

        let text = render_app(&mut terminal, &mut app);
        assert!(text.contains("Overview 1/2"));
        assert!(text.contains("a All"));
        assert!(text.contains("/ search"));

        app.handle_key(ConfigUiKey::Char('a'));
        assert_eq!(app.visible_rows().len(), 2);
        let text = render_app(&mut terminal, &mut app);
        assert!(text.contains("All 2/Overview 1"));
        assert!(text.contains("a Overview"));
        assert!(text.contains("/ search"));

        app.search_active = true;
        let text = render_app(&mut terminal, &mut app);
        assert!(text.contains("search: _"));
        assert!(!text.contains("a Overview"));

        app.search_active = false;
        app.search = "secret".to_string();
        assert_eq!(app.visible_rows(), vec![UiRowRef::Field(1)]);
        let text = render_app(&mut terminal, &mut app);
        assert!(text.contains("search All · secret"));
        assert!(!text.contains("a Overview"));

        app.search.clear();
        app.model.recommended_fields = Some(Vec::new());
        app.settings_view = ConfigUiSettingsView::Overview;
        let text = render_app(&mut terminal, &mut app);
        assert!(text.contains("Overview 0/2"));
        assert!(text.contains("Overview empty"));
        assert!(text.contains("Press a for All"));
        assert!(text.contains("a All"));

        app.search_active = true;
        let text = render_app(&mut terminal, &mut app);
        assert!(text.contains("Search All"));
        assert!(!text.contains("Press a"));

        app.search_active = false;
        for index in 2..12 {
            let mut extra = app.model.fields[0].clone();
            extra.path = format!("extra.setting_{index}");
            app.model.fields.push(extra);
        }
        app.model.recommended_fields = Some(
            app.model
                .fields
                .iter()
                .take(10)
                .map(|field| ConfigUiFieldId::new(field.source_id.clone(), field.path.clone()))
                .collect(),
        );
        app.settings_view = ConfigUiSettingsView::Overview;
        let text = render_app(&mut terminal, &mut app);
        assert!(text.contains("Overview 10/12"));

        app.settings_view = ConfigUiSettingsView::All;
        let text = render_app(&mut terminal, &mut app);
        assert!(text.contains("All 12/Overview 10"));
    }

    // Defends: headings are derived from filtered field rows and selection indices continue to target only real rows.
    #[test]
    fn section_entries_follow_visible_rows_and_translate_selection() {
        let mut runtime_enabled = field("runtime.enabled", "bool", "true", &[]);
        runtime_enabled.section_label = "Runtime".to_string();
        let mut runtime_shell = field("runtime.shell", "string", r#""nu""#, &[]);
        runtime_shell.section_label = "Runtime".to_string();
        let mut theme = field("ui.theme", "string", r#""dark""#, &[]);
        theme.section_label = "Appearance".to_string();
        let model = model_with_fields(vec![runtime_enabled, runtime_shell, theme]);
        let rows = visible_rows_for_tab_search(&model, 0, "");
        let entries = list_entries(&model, &rows);

        assert_eq!(
            entries,
            vec![
                ListEntry::Section("Runtime"),
                ListEntry::Row(UiRowRef::Field(0)),
                ListEntry::Row(UiRowRef::Field(1)),
                ListEntry::Section("Appearance"),
                ListEntry::Row(UiRowRef::Field(2)),
            ]
        );
        assert_eq!(selected_list_entry_index(&entries, 0), Some(1));
        assert_eq!(selected_list_entry_index(&entries, 1), Some(2));
        assert_eq!(selected_list_entry_index(&entries, 2), Some(4));
        assert_eq!(
            rendered_text(&section_heading_line("Runtime")),
            "── Runtime"
        );

        let filtered_rows = visible_rows_for_tab_search(&model, 0, "theme");
        assert_eq!(
            list_entries(&model, &filtered_rows),
            vec![
                ListEntry::Section("Appearance"),
                ListEntry::Row(UiRowRef::Field(2))
            ]
        );
    }

    // Defends: unsectioned tabs retain one list item per row and custom tables share the same heading mechanism.
    #[test]
    fn section_entries_preserve_unsectioned_and_custom_table_layouts() {
        let mut model = model_with_fields(vec![
            field("keys.copy", "string", r#""Ctrl+c""#, &[]),
            field("keys.paste", "string", r#""Ctrl+v""#, &[]),
        ]);
        let rows = visible_rows_for_tab_search(&model, 0, "");
        assert_eq!(
            list_entries(&model, &rows),
            vec![
                ListEntry::Row(UiRowRef::Field(0)),
                ListEntry::Row(UiRowRef::Field(1))
            ]
        );
        assert_eq!(
            selected_list_entry_index(&list_entries(&model, &rows), 1),
            Some(1)
        );

        model.fields[0].section_label = "Managed".to_string();
        model.fields[1].section_label = "Reference".to_string();
        model.fields[0].list_cells = strings(&["copy", "Ctrl+c"]);
        model.fields[1].list_cells = strings(&["paste", "Ctrl+v"]);
        set_list_table(&mut model, "general", &[("action", 12), ("binding", 12)]);
        assert!(matches!(list_layout(&model, 0), ListLayout::Table(_)));
        assert_eq!(
            list_entries(&model, &rows),
            vec![
                ListEntry::Section("Managed"),
                ListEntry::Row(UiRowRef::Field(0)),
                ListEntry::Section("Reference"),
                ListEntry::Row(UiRowRef::Field(1))
            ]
        );
        assert_eq!(
            rendered_cells(&row_line_for_layout(
                &model,
                UiRowRef::Field(1),
                list_layout(&model, 0),
            )),
            vec!["paste", "Ctrl+v"]
        );
    }

    fn source(
        id: &str,
        label: &str,
        exists: bool,
        owner_label: &str,
        read_only: bool,
    ) -> ConfigUiSource {
        ConfigUiSource {
            id: id.to_string(),
            label: label.to_string(),
            path: PathBuf::from(format!("/home/alex/.config/acme/{id}.toml")),
            exists,
            owner_label: Some(owner_label.to_string()),
            read_only,
        }
    }

    // Defends: the selected field drives header source metadata on a multi-source tab.
    #[test]
    fn header_uses_selected_config_source_metadata() {
        let mut model = model_with_fields(vec![
            field_with_source("settings", "ui.theme", "string", "dark", &[]),
            field_with_source("keys", "keys.leader", "string", "space", &[]),
        ]);
        model.sources = vec![
            source("settings", "Settings", true, "user", false),
            source("keys", "Keybindings", false, "home-manager", true),
        ];
        model.fields[1].snapshot.intent = ConfigUiOverride::Absent;
        model.fields[1].snapshot.effective = model.fields[1].snapshot.baseline.clone();
        model.fields[1].capability = ConfigUiCapability::ReadOnly {
            reason: "Managed declaratively.".to_string(),
            file_action_id: None,
        };
        let mut app = ConfigUiApp::new(model);
        app.selected_row = 1;

        let metadata = header_metadata(&app);
        assert_eq!(metadata.source_label.as_deref(), Some("Keybindings"));
        assert!(metadata.source_path.contains("keys.toml"));
        assert!(metadata.source_path.contains("missing"));
        assert_eq!(metadata.owner, "home-manager");
        assert_eq!(metadata.mode, "read-only");

        let line = header_metadata_line(&metadata, "ok", Style::default(), 160);
        let text = rendered_text(&line);
        assert!(text.contains("source: Keybindings"));
        assert!(text.contains("owner: home-manager"));
        assert!(text.contains("mode: read-only"));
    }

    // Defends: a source-less operational selection renders neutral metadata.
    #[test]
    fn header_is_neutral_without_selected_source_row() {
        let mut model = test_model(ConfigUiFieldState::Explicit);
        model.tabs.push("status".to_string());
        model.operational_tab = Some("status".to_string());
        let mut app = ConfigUiApp::new(model);
        app.selected_tab = 1;
        let metadata = header_metadata(&app);
        assert_eq!(metadata.source_label, None);
        assert_eq!(metadata.source_path, "not file-backed");
        assert_eq!(metadata.owner, "none");
        assert_eq!(metadata.mode, "n/a");

        let line = header_metadata_line(&metadata, "ok", Style::default(), 160);
        assert!(!rendered_text(&line).contains("source:"));
    }

    // Defends: source labels are bounded and omitted before they crowd out path context.
    #[test]
    fn header_bounds_source_label_for_narrow_widths() {
        let metadata = HeaderMetadata {
            source_label: Some("Very Long Source Label".to_string()),
            source_path: "/home/alex/.config/acme/settings.toml".to_string(),
            owner: "user".to_string(),
            mode: "writable",
        };

        let wide_text = rendered_text(&header_metadata_line(
            &metadata,
            "ok",
            Style::default(),
            120,
        ));
        assert!(wide_text.contains("source: Very Long Sourc..."));
        assert!(!wide_text.contains("Very Long Source Label"));

        let narrow_text =
            rendered_text(&header_metadata_line(&metadata, "ok", Style::default(), 30));
        assert!(!narrow_text.contains("source:"));
        assert!(narrow_text.contains("path:"));
    }

    // Defends: settings rows expose apply/setting/value without repeating complete-config explicit state.
    #[test]
    fn field_row_omits_explicit_state_column() {
        let model = test_model(ConfigUiFieldState::Explicit);
        let line = row_line_for_model(&model, UiRowRef::Field(0));

        assert_eq!(
            rendered_cells(&line),
            vec!["after app restart", "core.debug_mode", "false"]
        );
        assert!(!rendered_text(&line).contains("explicit"));
    }

    // Defends: the compact list shows the user's correction target first, then resolved truth,
    // then explicit intent, and never presents missing resolution as an unset override.
    #[test]
    fn field_row_value_projection_distinguishes_invalid_effective_intent_and_unresolved() {
        let mut invalid = built_field("integer", Some(&json!(1)), Some(&json!(2)));
        invalid.snapshot.intent = ConfigUiOverride::Invalid {
            input: "broken".to_string(),
        };
        invalid.snapshot.effective = Some(ConfigUiResolvedValue::new(json!(3)));

        let mut resolved = built_field("integer", Some(&json!(1)), Some(&json!(2)));
        resolved.snapshot.effective = Some(ConfigUiResolvedValue::new(json!(3)));

        let mut explicit = built_field("integer", Some(&json!(1)), None);
        explicit.snapshot.effective = None;

        let unresolved = built_field("integer", None, None);
        let model = model_with_fields(vec![invalid, resolved, explicit, unresolved]);

        assert_eq!(
            (0..4)
                .map(
                    |index| rendered_cells(&row_line_for_model(&model, UiRowRef::Field(index)))[2]
                        .clone()
                )
                .collect::<Vec<_>>(),
            ["broken", "3", "1", "unresolved"]
        );
        let invalid_details = detail_text(&model.fields[0]);
        assert!(invalid_details.contains("intent     invalid"));
        assert!(invalid_details.contains("input      broken"));
        assert!(invalid_details.contains("effective  3"));
        assert!(invalid_details.contains("baseline   2"));
    }

    // Defends: removing the state column still leaves visible names for the remaining settings columns.
    #[test]
    fn field_header_names_remaining_columns() {
        assert_eq!(
            rendered_cells(&field_list_header_line(DEFAULT_FIELD_COLUMN_WIDTHS)),
            vec!["takes effect", "setting", "value"]
        );
    }

    // Defends: a normal half-screen list gives unused setting space to structured previews.
    #[test]
    fn field_columns_give_remaining_width_to_values() {
        let widths = resolve_field_column_widths(
            78,
            [
                ("next Yazi", "flavor"),
                ("next Yazi", "plugin.prepend_fetchers"),
                ("absent", "yazi/package.toml"),
            ],
        );

        assert_eq!(widths.takes_effect, FIELD_TAKES_EFFECT_MIN_WIDTH);
        assert!(widths.setting < FIELD_SETTING_MAX_WIDTH);
        assert!(widths.value > 28);
        assert_eq!(widths.takes_effect + widths.setting + widths.value, 78);
    }

    // Defends: headers and rows share terminal-cell boundaries for ASCII, CJK, and joined emoji.
    #[test]
    fn field_header_and_rows_share_resolved_boundaries() {
        let widths = resolve_field_column_widths(40, [("next launch", "設定設定設定")]);
        let header = field_list_header_line(widths);
        let row = field_row_line(
            widths,
            "next launch",
            Style::default(),
            "設定設定設定",
            Style::default(),
            "値が長い設定値値が長い設定値",
            Style::default(),
        );

        assert_eq!(widths.setting, 13);
        assert_eq!(span_width(&header, 0), widths.takes_effect);
        assert_eq!(span_width(&header, 1), widths.setting);
        assert_eq!(span_width(&row, 0), widths.takes_effect);
        assert_eq!(span_width(&row, 1), widths.setting);
        assert!(span_width(&row, 2) <= widths.value);
        assert!(row.width() <= 40);
        assert_eq!(truncate_cells("👩‍💻👩‍💻👩‍💻", 5), "👩‍💻...");
    }

    // Defends: tiny render areas retain bounded columns instead of producing an over-wide line.
    #[test]
    fn field_columns_fit_narrow_and_zero_widths() {
        for available in [0, 1, 2, 3, 12, 25] {
            let widths = resolve_field_column_widths(
                available,
                [("a long takes-effect status", "a long setting label")],
            );
            let row = field_row_line(
                widths,
                "a long takes-effect status",
                Style::default(),
                "a long setting label",
                Style::default(),
                "a long value",
                Style::default(),
            );

            assert_eq!(
                widths.takes_effect + widths.setting + widths.value,
                available
            );
            assert_eq!(row.width(), available);
        }
    }

    // Defends: search does not move column starts because widths use every row in the selected tab.
    #[test]
    fn field_columns_stay_stable_when_search_hides_the_longest_label() {
        let mut short = field("short", "string", r#""value""#, &[]);
        short.display_label = "Short".to_string();
        let mut long = field("long", "string", r#""value""#, &[]);
        long.display_label = "A much longer setting label".to_string();
        let model = model_with_fields(vec![short, long]);

        assert_eq!(visible_rows_for_tab_search(&model, 0, "short").len(), 1);
        let widths = field_column_widths(&model, 0, 60);
        assert_eq!(
            widths.setting,
            terminal_width("A much longer setting label") + 1
        );
    }

    // Defends: host-defined field table profiles render supplied cells without parsing display labels or values.
    #[test]
    fn custom_field_table_renders_host_supplied_cells() {
        let mut model = test_model(ConfigUiFieldState::Explicit);
        model.tabs = vec!["keys".to_string()];
        model.fields[0].tab = "keys".to_string();
        model.fields[0].display_label = "ignored label".to_string();
        model.fields[0].list_cells = strings(&[
            "editor",
            "Ctrl+x",
            "cut selection",
            "user",
            "settings.toml",
            "ignored extra",
        ]);
        set_list_table(
            &mut model,
            "keys",
            &[
                ("group", 10),
                ("keys", 10),
                ("action", 18),
                ("owner", 8),
                ("source", 14),
            ],
        );

        let layout = list_layout(&model, 0);
        assert_eq!(
            rendered_cells(&list_header_line(layout)),
            vec!["group", "keys", "action", "owner", "source"]
        );
        let row = row_line_for_layout(&model, UiRowRef::Field(0), layout);
        assert_eq!(
            rendered_cells(&row),
            vec!["editor", "Ctrl+x", "cut selection", "user", "settings.toml"]
        );
        assert_eq!(
            row.spans[0].style,
            apply_status_style(&model.fields[0].apply_status)
        );
        assert_eq!(row.spans[1].style, config_key_style());
        assert_eq!(row.spans[2].style, metadata_value_style());
        assert_eq!(row.spans[3].style, fg_style(Color::Gray));
    }

    // Defends: missing custom table cells render as blanks instead of panicking.
    #[test]
    fn custom_field_table_allows_missing_cells() {
        let mut model = test_model(ConfigUiFieldState::Explicit);
        set_list_table(&mut model, "general", &[("one", 8), ("two", 8)]);
        model.fields[0].list_cells = strings(&["only"]);

        let layout = list_layout(&model, 0);
        assert_eq!(
            rendered_cells(&row_line_for_layout(&model, UiRowRef::Field(0), layout)),
            vec!["only", ""]
        );
    }

    // Defends: narrow host table widths bound rendered cells instead of leaking a full ellipsis.
    #[test]
    fn custom_field_table_honors_narrow_column_widths() {
        let mut model = test_model(ConfigUiFieldState::Explicit);
        set_list_table(&mut model, "general", &[("abcd", 2), ("empty", 0)]);
        model.fields[0].list_cells = strings(&["wxyz", "hidden"]);

        let layout = list_layout(&model, 0);
        let header = list_header_line(layout);
        let row = row_line_for_layout(&model, UiRowRef::Field(0), layout);

        assert_eq!(rendered_cells(&header), vec!["..", ""]);
        assert_eq!(rendered_cells(&row), vec!["..", ""]);
    }

    // Defends: the host-selected operational tab keeps status columns even for unchecked metadata.
    #[test]
    fn operational_tab_ignores_custom_field_table() {
        let mut model = test_model(ConfigUiFieldState::Explicit);
        model.tabs = vec!["advanced".to_string()];
        model.operational_tab = Some("advanced".to_string());
        set_list_table(&mut model, "advanced", &[("custom", 8)]);

        assert_eq!(
            rendered_cells(&list_header_line(list_layout(&model, 0))),
            vec!["status", "item", "detail"]
        );
    }

    // Defends: display labels improve visible text while details keep the stable field path.
    #[test]
    fn field_display_label_replaces_visible_label_but_keeps_path_detail() {
        let mut model = test_model(ConfigUiFieldState::Explicit);
        model.fields[0].path = "ui.pane_frames.rounded_corners".to_string();
        model.fields[0].display_label = "Rounded corners".to_string();

        assert_eq!(
            rendered_cells(&row_line_for_model(&model, UiRowRef::Field(0))),
            vec!["after app restart", "Rounded corners", "false"]
        );

        let details = default_field_detail_lines(&model.fields[0]);
        assert_eq!(rendered_text(&details[0]), "Rounded corners");
        assert_eq!(
            rendered_cells(&details[1]),
            vec!["path", "ui.pane_frames.rounded_corners"]
        );
    }

    // Defends: details retain complete structures while keeping intent, effective resolution,
    // baseline resolution, and bounded provenance visibly distinct.
    #[test]
    fn field_details_keep_sparse_channels_and_structures_distinct() {
        let long_value = "a deliberately long nested value that the detail paragraph can wrap without hiding its surrounding structure";
        let intent = json!({"nested": [{"message": long_value}, {"enabled": true}]});
        let baseline = json!({"nested": [{"message": "short"}]});
        let mut field = built_field("object", Some(&intent), Some(&baseline));
        field.snapshot.effective = Some(ConfigUiResolvedValue {
            value: json!({"nested": [{"message": "applied"}]}),
            origin: Some("runtime resolution".to_string()),
        });
        field.snapshot.baseline.as_mut().expect("baseline").origin =
            Some("tool defaults".to_string());
        field.snapshot.external_manager = Some("system policy".to_string());

        let model = model_with_fields(vec![field]);
        assert_eq!(
            rendered_cells(&row_line_for_model(&model, UiRowRef::Field(0))),
            vec!["after save", "ui.test", "{1 keys}"]
        );

        let details = detail_text(&model.fields[0]);
        assert!(details.contains(&format!(
            "override   \n  {{\n    \"nested\": [\n      {{\n        \"message\": \"{long_value}\""
        )));
        assert!(details.contains(
            "effective  \n  {\n    \"nested\": [\n      {\n        \"message\": \"applied\""
        ));
        assert!(details.contains(
            "baseline   \n  {\n    \"nested\": [\n      {\n        \"message\": \"short\""
        ));
        assert!(details.contains("intent     explicit"));
        assert!(details.contains("eff. origin runtime resolution"));
        assert!(details.contains("base origin tool defaults"));
        assert!(details.contains("managed by system policy"));
        assert!(!details.contains("state      "));
        assert!(!details.contains("current    "));
        assert!(!details.contains("default    "));
        assert!(!details.contains("..."));

        let values = json!([1, 2, 3, 4, 5]);
        let same = built_field("array", Some(&values), Some(&values));
        let same_details = detail_text(&same);

        assert_eq!(same_details.matches("    1,").count(), 1);
        assert!(same_details.contains("override   "));
        assert!(same_details.contains("effective  same as override"));
        assert!(same_details.contains("baseline   same as effective"));

        let mut choice = built_field("string", Some(&json!("requested")), None);
        choice.capability = ConfigUiCapability::Choice {
            choices: ["requested", "applied"]
                .into_iter()
                .map(|value| ConfigUiChoice::new(json!(value)))
                .collect(),
        };
        choice.snapshot.effective = Some(ConfigUiResolvedValue::new(json!("applied")));
        let choice_details = rendered_lines(&single_choice_field_detail_lines(&choice));
        assert!(choice_details.contains("(x) applied"));
        assert!(!choice_details.contains("(x) requested"));
    }

    // Defends: absent and scalar channels stay compact while TOML-only float values survive the
    // structured fallback verbatim without being mislabeled as current or default state.
    #[test]
    fn detail_values_keep_sparse_labels_scalars_and_special_toml_complete() {
        let scalar = built_field("bool", Some(&json!(true)), Some(&json!(false)));
        let scalar_details = default_field_detail_lines(&scalar)
            .iter()
            .map(rendered_text)
            .collect::<Vec<_>>();
        assert!(scalar_details.iter().any(|line| line == "override   true"));
        assert!(
            scalar_details
                .iter()
                .any(|line| line == "effective  same as override")
        );
        assert!(scalar_details.iter().any(|line| line == "baseline   false"));

        let unset = built_field("array", None, None);
        assert!(detail_text(&unset).contains("intent     absent"));
        assert!(!detail_text(&unset).contains("effective  "));
        assert!(!detail_text(&unset).contains("baseline   "));

        let rows = build_toml_document_fields(ConfigUiTomlDocumentSpec {
            source_id: "native",
            tab: "native",
            section_label: "",
            current_toml: "limits = [inf, -inf, nan]",
            baseline_toml: Some("limits = [nan]"),
            validation: "",
            rebuild_required: false,
            apply_status: apply_status("after save", "Applied after saving."),
        })
        .expect("TOML document rows");
        let field = rows
            .fields
            .iter()
            .find(|field| field.path == "limits")
            .expect("limits field");
        let special_details = detail_text(field);

        assert!(special_details.contains("override   \n  [inf, -inf, nan]"));
        assert!(!special_details.contains("effective  "));
        assert!(special_details.contains("baseline   \n  [nan]"));
    }

    // Defends: invalid field rows remain visibly exceptional even without the normal state column.
    #[test]
    fn invalid_field_row_keeps_error_style_on_setting_and_value() {
        let model = test_model(ConfigUiFieldState::Invalid);
        let line = row_line_for_model(&model, UiRowRef::Field(0));

        assert_eq!(
            line.spans[1].style,
            state_style(ConfigUiFieldState::Invalid)
        );
        assert_eq!(
            line.spans[2].style,
            state_style(ConfigUiFieldState::Invalid)
        );
    }

    // Defends: exact diagnostics remain reachable through field details without an operational
    // tab, while blocking diagnostics style the row without relabeling inherited intent.
    #[test]
    fn scoped_diagnostic_renders_effective_field_state_message_and_scope() {
        let mut model = test_model(ConfigUiFieldState::Inherited);
        model.fields[0].capability = ConfigUiCapability::Choice {
            choices: ["false", "true"]
                .into_iter()
                .map(|value| ConfigUiChoice::new(json!(value)))
                .collect(),
        };
        model.diagnostics.push(ConfigUiDiagnostic {
            path: "core.debug_mode".to_string(),
            status: "invalid".to_string(),
            headline: "Invalid debug mode".to_string(),
            blocking: true,
            scope: ConfigUiDiagnosticScope::Field(ConfigUiFieldId::new(
                DEFAULT_CONFIG_SOURCE_ID,
                "core.debug_mode",
            )),
            detail_lines: Vec::new(),
        });
        model.diagnostics.push(ConfigUiDiagnostic {
            path: "core.debug_mode".to_string(),
            status: "notice".to_string(),
            headline: "A second field notice".to_string(),
            blocking: false,
            scope: ConfigUiDiagnosticScope::Field(ConfigUiFieldId::new(
                DEFAULT_CONFIG_SOURCE_ID,
                "core.debug_mode",
            )),
            detail_lines: Vec::new(),
        });
        let mut app = ConfigUiApp::new(model);

        let row = row_line_for_model(&app.model, UiRowRef::Field(0));
        assert_eq!(row.spans[1].style, state_style(ConfigUiFieldState::Invalid));
        let field_details = rendered_lines(&app.render_details(UiRowRef::Field(0)));
        assert!(field_details.contains("intent     absent"));
        assert!(field_details.contains("effective  false"));
        assert!(field_details.contains("baseline   same as effective"));
        assert!(field_details.contains("Invalid debug mode"));
        assert!(field_details.contains("A second field notice"));

        app.model.diagnostics[0].blocking = false;
        app.model.diagnostics[0].status = "preserved".to_string();
        app.model.diagnostics[0].headline = "Preserved debug metadata".to_string();
        let field_details = rendered_lines(&app.render_details(UiRowRef::Field(0)));
        assert!(
            field_details.contains("intent     absent"),
            "{field_details}"
        );
        assert!(field_details.contains("Preserved debug metadata"));

        let diagnostic_details =
            |app: &ConfigUiApp| rendered_lines(&app.render_details(UiRowRef::Diagnostic(0)));
        assert!(
            diagnostic_details(&app)
                .contains("scope      field\nsource     config\nfield      core.debug_mode")
        );

        app.model.diagnostics[0].scope = ConfigUiDiagnosticScope::Source {
            source_id: DEFAULT_CONFIG_SOURCE_ID.to_string(),
        };
        assert!(diagnostic_details(&app).contains("scope      source\nsource     config"));

        app.model.diagnostics[0].scope = ConfigUiDiagnosticScope::Global;
        assert!(diagnostic_details(&app).contains("scope      global"));
    }

    // Defends: light roles stay readable while the outer-borderless body keeps one internal divider.
    #[test]
    fn light_theme_and_divided_body_have_stable_rendered_contract() {
        let mut invalid = field("ui.invalid", "string", "broken", &[]);
        crate::model::set_field_state_for_test(&mut invalid, ConfigUiFieldState::Invalid);
        let mut ready = field("ui.ready", "string", "ready-value", &[]);
        ready.apply_status = apply_status("ready now", "Already active.");
        ready.apply_status.pending = false;
        let mut app = ConfigUiApp::new(model_with_fields(vec![invalid, ready]));
        app.model.tabs.push("other".to_string());
        app.active_theme = ConfigUiTheme::Light;

        let mut terminal = Terminal::new(TestBackend::new(120, 24)).expect("test terminal");
        terminal
            .draw(|frame| draw_config_ui(frame, &mut app))
            .expect("render config UI");

        let palette = config_ui_theme_palette(ConfigUiTheme::Light);
        let buffer = terminal.backend().buffer();
        assert_eq!(rendered_cell(buffer, "broken").fg, palette.error);
        assert_eq!(rendered_cell(buffer, "broken").bg, palette.selected_bg);
        assert_eq!(rendered_cell(buffer, "ready now").fg, palette.success);
        assert_eq!(rendered_cell(buffer, "ready-value").fg, palette.muted);
        assert_eq!(rendered_cell(buffer, "Config").fg, palette.title);
        assert_eq!(rendered_cell(buffer, "intent").fg, palette.metadata_key);
        assert_eq!(rendered_cell(buffer, "q quit").fg, palette.text);
        assert_eq!(rendered_cell(buffer, "settings").fg, Color::Black);
        assert_eq!(rendered_cell(buffer, "details").fg, Color::Black);
        assert_eq!(rendered_cell(buffer, "(1) general").fg, palette.accent);
        assert_eq!(rendered_cell(buffer, "(2) other").fg, Color::Black);

        assert_eq!(buffer[(buffer.area.left(), 2)].symbol(), " ");
        assert_eq!(buffer[(buffer.area.left() + 1, 2)].symbol(), "(");
        assert_eq!(buffer[(buffer.area.left(), 3)].symbol(), " ");
        assert_eq!(buffer[(buffer.area.right() - 1, 3)].symbol(), " ");
        for x in buffer.area.left() + 1..buffer.area.right() - 1 {
            assert_eq!(buffer[(x, 3)].symbol(), "─");
            assert_eq!(buffer[(x, 3)].fg, palette.border);
        }

        for (position, symbol) in [((1, 4), "s"), ((1, 6), "t"), ((62, 4), "d"), ((62, 6), "u")] {
            assert_eq!(buffer[position].symbol(), symbol);
        }
        assert!(
            (buffer.area.left()..60)
                .chain(61..buffer.area.right())
                .all(|x| buffer[(x, 5)].symbol() == " ")
        );
        let body_area = Rect::new(buffer.area.x, 4, buffer.area.width, 18);
        for y in body_area.y..body_area.bottom() {
            assert_eq!(buffer[(60, y)].symbol(), "│");
            assert_eq!(buffer[(60, y)].fg, palette.border);
        }
        assert!(
            buffer
                .content()
                .iter()
                .all(|cell| !matches!(cell.symbol(), "┬" | "┴"))
        );

        for foreground in [
            palette.text,
            palette.muted,
            palette.title,
            palette.accent,
            palette.success,
            palette.error,
            palette.metadata_key,
            palette.config_key,
        ] {
            assert!(contrast_ratio(foreground, Color::White) >= 4.5);
            assert!(contrast_ratio(foreground, palette.selected_bg) >= 4.5);
        }
        assert!(contrast_ratio(palette.border, Color::White) >= 3.0);
    }

    // Defends: absent operational status rows stay neutral instead of warning-colored.
    #[test]
    fn absent_sidecar_status_is_neutral() {
        let mut model = test_model(ConfigUiFieldState::Explicit);
        model.sidecars = vec![ConfigUiSidecar {
            name: "Native config".to_string(),
            path: PathBuf::from("/home/alex/.config/acme/native.toml"),
            present: false,
            owner_label: Some("user".to_string()),
            read_only: false,
        }];
        let line = row_line_for_model(&model, UiRowRef::Sidecar(0));

        assert_eq!(
            rendered_cells(&line),
            vec![
                "absent",
                "Native config",
                "/home/alex/.config/acme/native.toml"
            ]
        );
        assert_eq!(span_width(&line, 0), STATUS_COLUMN_WIDTH);
        assert_eq!(span_width(&line, 1), STATUS_ITEM_COLUMN_WIDTH);
        assert_eq!(line.spans[0].style, Style::default().fg(Color::Gray));
        assert!(
            sidecar_detail_lines(&model.sidecars[0])
                .iter()
                .map(rendered_text)
                .any(|line| line.contains("state") && line.contains("absent"))
        );
    }

    // Defends: native status rows align with the operational status/item/detail header.
    #[test]
    fn native_status_rows_use_operational_status_columns() {
        let mut model = test_model(ConfigUiFieldState::Explicit);
        model.native_config_statuses = vec![ConfigUiNativeStatus {
            surface: "mars".to_string(),
            tool: "mars".to_string(),
            description: "Mars config".to_string(),
            status: "existing".to_string(),
            label: "Managed config present".to_string(),
            severity: "ok".to_string(),
            active_path: Some("/home/alex/.config/mars/config.toml".to_string()),
            managed_path: Some("/home/alex/.config/yazelix/mars.toml".to_string()),
            native_paths: vec!["/home/alex/.config/mars/config.toml".to_string()],
            generated_path: None,
            allowed_action: "none".to_string(),
            read_only_reason: None,
        }];
        let line = row_line_for_model(&model, UiRowRef::NativeStatus(0));

        assert_eq!(
            rendered_cells(&line),
            vec!["existing", "mars", "Managed config present"]
        );
        assert_eq!(span_width(&line, 0), STATUS_COLUMN_WIDTH);
        assert_eq!(span_width(&line, 1), STATUS_ITEM_COLUMN_WIDTH);
    }

    // Defends: operational file actions use status columns without changing field-tab actions.
    #[test]
    fn operational_file_actions_use_status_columns() {
        let mut model = test_model(ConfigUiFieldState::Explicit);
        model.file_actions = vec![file_action(true, true, false, None)];
        let line = row_line_for_layout(&model, UiRowRef::FileAction(0), ListLayout::Status);

        assert_eq!(
            rendered_cells(&line),
            vec![
                "read-only",
                "Native config",
                "/home/alex/.config/acme/native.toml"
            ]
        );
        assert_eq!(span_width(&line, 0), STATUS_COLUMN_WIDTH);
        assert_eq!(span_width(&line, 1), STATUS_ITEM_COLUMN_WIDTH);
    }

    // Defends: normal controls derive reset visibility from override/source authority instead of
    // baseline knowledge or editor capability.
    #[test]
    fn normal_controls_follow_selected_row_capabilities() {
        let mut model = test_model(ConfigUiFieldState::Explicit);
        model.fields[0].snapshot.baseline = None;
        let mut app = ConfigUiApp::new(model);
        assert!(rendered_text(&normal_control_line(&app)).contains("u inherit"));
        let unknown = rendered_lines(&app.render_details(UiRowRef::Field(0)));
        assert!(unknown.contains("reset      remove override"));
        assert!(unknown.contains("inherits   unknown"));

        app.model.fields[0].capability = ConfigUiCapability::ReadOnly {
            reason: "Edit the source file directly.".to_string(),
            file_action_id: Some("open_native".to_string()),
        };
        let mut action = file_action(true, false, false, None);
        action.source_id = DEFAULT_CONFIG_SOURCE_ID.to_string();
        app.model.file_actions = vec![action];
        assert_eq!(
            rendered_text(&normal_control_line(&app)),
            "e open Native config  u inherit"
        );
        app.model.file_actions[0].disabled_reason = Some("Unavailable.".to_string());
        assert_eq!(
            rendered_text(&normal_control_line(&app)),
            "file action unavailable  u inherit"
        );

        app.model.fields[0].snapshot.baseline = Some(ConfigUiResolvedValue {
            value: json!(true),
            origin: Some("tool defaults".to_string()),
        });
        let known = rendered_lines(&app.render_details(UiRowRef::Field(0)));
        assert!(known.contains("reset      remove override"));
        assert!(known.contains("baseline   true"));
        assert!(known.contains("base origin tool defaults"));
        assert!(known.contains("inherits   baseline"));

        app.model.fields[0].can_unset = false;
        assert!(!rendered_text(&normal_control_line(&app)).contains("inherit"));
        assert!(!rendered_lines(&app.render_details(UiRowRef::Field(0))).contains("reset      "));

        app.model.fields[0].can_unset = true;
        app.model.fields[0].snapshot.intent = ConfigUiOverride::Absent;
        app.model.fields[0].snapshot.effective = None;
        assert!(!rendered_text(&normal_control_line(&app)).contains("inherit"));

        app.model.fields[0].snapshot.intent = ConfigUiOverride::Explicit(json!(false));
        app.model.fields[0].snapshot.effective = Some(ConfigUiResolvedValue::new(json!(false)));
        app.model.sources[0].read_only = true;
        assert!(!rendered_text(&normal_control_line(&app)).contains("inherit"));
    }

    // Defends: boolean controls distinguish normal-mode staging from edit-mode persistence.
    #[test]
    fn boolean_controls_show_space_to_stage_and_enter_to_save() {
        let mut model = test_model(ConfigUiFieldState::Explicit);
        let ConfigUiCapability::Toggle { off, on } = &mut model.fields[0].capability else {
            panic!("boolean toggle capability");
        };
        off.label = Some("Disabled".to_string());
        on.label = Some("Enabled".to_string());
        let mut app = ConfigUiApp::new(model);

        let normal = rendered_text(&normal_control_line(&app));
        assert!(normal.contains("Space stage"));
        assert!(!normal.contains("Enter/Space stage"));

        let details = |app: &ConfigUiApp| rendered_lines(&app.render_details(UiRowRef::Field(0)));
        assert!(details(&app).contains("  (x) Disabled\n  ( ) Enabled"));

        assert_eq!(app.handle_key(ConfigUiKey::Char(' ')), ConfigUiIntent::None);
        assert!(details(&app).contains("  ( ) Disabled\n  (x) Enabled"));

        let field = app.selected_field().expect("boolean field");
        let editing = rendered_text(&edit_control_line(field, ConfigUiEditMode::Choice));
        assert!(editing.contains("Space toggle"));
        assert!(editing.contains("Enter save"));
        assert!(editing.contains("Esc cancel"));
    }

    // Defends: ordered multichoice editing exposes selected order and the generic reorder command.
    #[test]
    fn ordered_multichoice_rendering_shows_order_controls() {
        let mut model = test_model(ConfigUiFieldState::Explicit);
        let field = &mut model.fields[0];
        let value = json!(["status", "clock"]);
        field.snapshot.intent = ConfigUiOverride::Explicit(value.clone());
        field.snapshot.effective = Some(ConfigUiResolvedValue::new(value.clone()));
        field.capability = ConfigUiCapability::MultiChoice {
            choices: ["clock", "status", "mode"]
                .into_iter()
                .map(|value| ConfigUiChoice::new(json!(value)))
                .collect(),
            ordered: true,
        };
        let input = value.to_string();
        let edit = ConfigUiEditState {
            field_id: field.id(),
            input: input.clone(),
            mode: ConfigUiEditMode::MultiChoice,
            choice_index: 1,
            cursor: input.len(),
        };

        assert!(multi_choice_status_value(field, &edit).contains("order status, clock"));
        let detail = multi_choice_detail_lines(field, &edit)
            .iter()
            .map(rendered_text)
            .collect::<Vec<_>>();
        assert!(
            detail
                .iter()
                .any(|line| line.contains("order") && line.contains("status, clock"))
        );
        assert_eq!(
            detail
                .iter()
                .filter(|line| line.contains('['))
                .cloned()
                .collect::<Vec<_>>(),
            vec!["  [x] status", "> [x] clock", "  [ ] mode"]
        );
        assert!(
            rendered_text(&edit_control_line(field, ConfigUiEditMode::MultiChoice))
                .contains("J/K reorder")
        );
    }

    // Defends: text edit controls expose the host-owned external editor path without changing picker controls.
    #[test]
    fn text_edit_controls_show_external_editor_key() {
        let mut model = test_model(ConfigUiFieldState::Explicit);
        model.fields[0].capability = ConfigUiCapability::FreeText {
            encoding: ConfigUiTextEncoding::String,
        };
        let field = &model.fields[0];
        let app = ConfigUiApp::new(model.clone());

        assert!(rendered_text(&normal_control_line(&app)).contains("Enter inline  e editor"));

        let text_controls = rendered_text(&edit_control_line(field, ConfigUiEditMode::Text));
        assert!(text_controls.starts_with("Enter save  Esc cancel"));
        assert!(text_controls.contains("Ctrl+e editor"));
        assert!(text_controls.contains("Ctrl+u clear"));
        assert!(text_controls.contains("Home/End"));
        assert!(text_controls.contains("Del/Backspace"));

        assert!(
            !rendered_text(&edit_control_line(field, ConfigUiEditMode::Choice)).contains("editor")
        );
    }

    // Defends: text status keeps the cursor visible while choices use friendly labels instead of
    // raw encoded values.
    #[test]
    fn edit_status_windows_text_and_labels_exact_choices() {
        assert_eq!(text_edit_window("abcdefghij", 8, 5), "gh_ij");
        assert_eq!(text_edit_window("ab👩‍💻cd", 2, 5), "ab_👩‍💻");
        assert_eq!(text_edit_window("ab👩‍💻cd", 2 + "👩‍💻".len(), 5), "👩‍💻_cd");
        assert!(terminal_width(&text_edit_window("abcdefghij", 5, 4)) <= 4);

        let text_field = field("a.very.long.setting.path", "string", "abcdefghij", &[]);
        let edit = ConfigUiEditState {
            field_id: text_field.id(),
            input: "abcdefghij".to_string(),
            mode: ConfigUiEditMode::Text,
            choice_index: 0,
            cursor: 8,
        };
        let narrow = edit_status_line(&text_field, &edit, 7);
        assert_eq!(rendered_text(&narrow), "efgh_ij");
        for width in [1, 6, 24, 40] {
            let text = rendered_text(&edit_status_line(&text_field, &edit, width));
            assert!(text.contains('_'));
            assert!(terminal_width(&text) <= width);
        }

        let choice_field = field(
            "a.very.long.setting.path",
            "string",
            r#""light""#,
            &["light", "dark"],
        );
        let choice = ConfigUiEditState {
            field_id: choice_field.id(),
            input: r#""light""#.to_string(),
            mode: ConfigUiEditMode::Choice,
            choice_index: 0,
            cursor: 0,
        };
        assert_eq!(
            rendered_text(&edit_status_line(&choice_field, &choice, 80)),
            "editing: a.very.long.setting.path = selected light"
        );
        for width in [1, 6, 24, 40] {
            let text = rendered_text(&edit_status_line(&choice_field, &choice, width));
            assert!(terminal_width(&text) <= width);
        }
    }

    fn file_action(
        exists: bool,
        read_only: bool,
        create_if_missing: bool,
        disabled_reason: Option<&str>,
    ) -> ConfigUiFileAction {
        ConfigUiFileAction {
            source_id: "native".to_string(),
            action_id: "open_native".to_string(),
            tab: "general".to_string(),
            label: "Native config".to_string(),
            description: "Host-owned native config file".to_string(),
            path: PathBuf::from("/home/alex/.config/acme/native.toml"),
            exists,
            read_only,
            create_if_missing,
            disabled_reason: disabled_reason.map(str::to_string),
        }
    }

    // Defends: file action rows expose neutral absent states without weakening existing, read-only, or error states.
    #[test]
    fn file_action_rows_render_host_file_states() {
        let mut model = test_model(ConfigUiFieldState::Explicit);
        model.fields.clear();
        model.file_actions = vec![
            file_action(true, false, true, None),
            file_action(false, false, true, None),
            file_action(false, false, false, None),
            file_action(true, true, false, Some("Managed declaratively")),
            file_action(false, false, true, Some("Path cannot be resolved")),
        ];

        for (index, (status, style)) in [
            ("existing", Style::default().fg(Color::Green)),
            ("absent", Style::default().fg(Color::Gray)),
            ("absent", Style::default().fg(Color::Gray)),
            ("read-only", Style::default().fg(Color::Yellow)),
            (
                "error",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let line = row_line_for_model(&model, UiRowRef::FileAction(index));
            assert_eq!(
                rendered_cells(&line),
                vec![status, "Native config", "/home/alex/.config/acme/n..."]
            );
            assert_eq!(line.spans[0].style, style);
        }
    }

    // Defends: file action details show host routing metadata and creation policy.
    #[test]
    fn file_action_details_show_host_boundary_metadata() {
        let action = file_action(false, false, true, Some("Path cannot be resolved"));
        let details = file_action_detail_lines(&action);
        let text = details.iter().map(rendered_text).collect::<Vec<_>>();

        assert!(text.iter().any(|line| line.contains("Native config")));
        assert!(text.iter().any(|line| line.contains("source")));
        assert!(text.iter().any(|line| line.contains("open_native")));
        assert!(text.iter().any(|line| line.contains("offered when absent")));
        assert!(
            text.iter()
                .any(|line| line.contains("Path cannot be resolved"))
        );
    }
}
