// Test lane: default
use super::*;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Tabs, Wrap};
use std::collections::BTreeSet;

const HEADER_HORIZONTAL_PADDING: u16 = 1;
const FIELD_TAKES_EFFECT_COLUMN_WIDTH: usize = 26;
const FIELD_SETTING_COLUMN_WIDTH: usize = 30;
const FIELD_VALUE_COLUMN_WIDTH: usize = 18;
const HEADER_MIN_PATH_WIDTH: usize = 8;
const HEADER_MIN_SOURCE_LABEL_WIDTH: usize = 4;
const HEADER_SOURCE_LABEL_WIDTH: usize = 18;
const STATUS_COLUMN_WIDTH: usize = 10;
const STATUS_ITEM_COLUMN_WIDTH: usize = 42;

#[derive(Clone, Copy)]
enum ListLayout {
    Field,
    Status,
}

struct HeaderMetadata {
    source_label: Option<String>,
    source_path: String,
    owner: &'static str,
    mode: &'static str,
}

impl ConfigUiApp {
    pub fn render_details(&self, row: UiRowRef) -> Vec<Line<'static>> {
        match row {
            UiRowRef::Field(index) => {
                let field = &self.model.fields[index];
                if let Some(edit) = &self.edit
                    && edit.field_index == index
                {
                    match edit.mode {
                        ConfigUiEditMode::Choice if is_scalar_enum_field(field) => {
                            return single_choice_detail_lines(field, edit);
                        }
                        ConfigUiEditMode::MultiChoice => {
                            return multi_choice_detail_lines(field, edit);
                        }
                        _ => {}
                    }
                }
                if is_scalar_enum_field(field) {
                    return single_choice_field_detail_lines(field);
                }
                default_field_detail_lines(field)
            }
            UiRowRef::Sidecar(index) => sidecar_detail_lines(&self.model.sidecars[index]),
            UiRowRef::FileAction(index) => {
                file_action_detail_lines(&self.model.file_actions[index])
            }
            UiRowRef::Diagnostic(index) => diagnostic_detail_lines(&self.model.diagnostics[index]),
            UiRowRef::NativeStatus(index) => {
                native_status_detail_lines(&self.model.native_config_statuses[index])
            }
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
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Min(8),
            Constraint::Length(2),
        ])
        .split(area);

    render_header(frame, app, root[0]);
    render_tabs(frame, app, root[1]);
    render_body(frame, app, root[2], &detail_lines);
    render_footer(frame, app, root[3]);
}

fn render_header(frame: &mut Frame<'_>, app: &ConfigUiApp, area: Rect) {
    let metadata = header_metadata(&app.model, app.selected_tab);
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

    frame.render_widget(Block::default().borders(Borders::BOTTOM), area);
    let horizontal_padding = HEADER_HORIZONTAL_PADDING.min(area.width / 2);
    let content = Rect {
        x: area.x + horizontal_padding,
        y: area.y,
        width: area
            .width
            .saturating_sub(horizontal_padding.saturating_mul(2)),
        height: area.height.saturating_sub(1).max(1),
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

    frame.render_widget(Paragraph::new(title).alignment(Alignment::Left), title_area);
    if metadata_area.width > 0 {
        frame.render_widget(
            Paragraph::new(header_metadata_line(
                &metadata,
                &diagnostic_text,
                diagnostic_style,
                metadata_area.width as usize,
            ))
            .alignment(Alignment::Right),
            metadata_area,
        );
    }
}

fn header_metadata(model: &ConfigUiModel, selected_tab: usize) -> HeaderMetadata {
    if let Some(source) = selected_config_source(model, selected_tab) {
        return HeaderMetadata {
            source_label: Some(source.label.clone()),
            source_path: config_path_text(&source.path, source.exists),
            owner: owner_label(source.owner),
            mode: write_mode(source.read_only),
        };
    }

    HeaderMetadata {
        source_label: None,
        source_path: config_path_text(&model.active_config_path, model.active_config_exists),
        owner: owner_label(model.config_owner),
        mode: write_mode(model.config_read_only),
    }
}

fn config_path_text(path: &std::path::Path, exists: bool) -> String {
    if exists {
        path.display().to_string()
    } else {
        format!("{} (missing; showing shipped defaults)", path.display())
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
        Span::styled(metadata.owner, metadata_value_style()),
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
    let labels = app
        .model
        .tabs
        .iter()
        .map(|tab| Line::from(Span::raw(tab.clone())))
        .collect::<Vec<_>>();
    frame.render_widget(
        Tabs::new(labels)
            .select(app.selected_tab)
            .style(fg_style(Color::Gray))
            .highlight_style(bold_fg_style(Color::Yellow)),
        area,
    );
}

fn render_body(
    frame: &mut Frame<'_>,
    app: &mut ConfigUiApp,
    area: Rect,
    detail_lines: &impl Fn(&ConfigUiApp, UiRowRef) -> Vec<Line<'static>>,
) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(44), Constraint::Percentage(56)])
        .split(area);
    let rows = app.visible_rows();
    app.clamp_selection_for_len(rows.len());
    render_list(frame, app, chunks[0], &rows);
    render_details(
        frame,
        app,
        chunks[1],
        rows.get(app.selected_row).copied(),
        detail_lines,
    );
}

fn render_list(frame: &mut Frame<'_>, app: &ConfigUiApp, area: Rect, rows: &[UiRowRef]) {
    let layout = list_layout(app);
    let items = rows
        .iter()
        .map(|row| ListItem::new(row_line_for_layout(&app.model, *row, layout)))
        .collect::<Vec<_>>();
    let mut state = ListState::default();
    if !items.is_empty() {
        state.select(Some(app.selected_row));
    }
    let title = if app.search.is_empty() {
        "settings".to_string()
    } else {
        format!("settings filtered by {}", app.search)
    };
    let block = Block::default().title(title).borders(Borders::ALL);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height == 0 {
        return;
    }

    let list_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);
    frame.render_widget(
        Paragraph::new(list_header_line(app)).alignment(Alignment::Left),
        list_chunks[0],
    );
    if list_chunks[1].height == 0 {
        return;
    }

    frame.render_stateful_widget(
        List::new(items).highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
        list_chunks[1],
        &mut state,
    );
}

fn list_header_line(app: &ConfigUiApp) -> Line<'static> {
    match list_layout(app) {
        ListLayout::Field => field_list_header_line(),
        ListLayout::Status => status_list_header_line(),
    }
}

fn list_layout(app: &ConfigUiApp) -> ListLayout {
    match app.model.tabs.get(app.selected_tab).map(String::as_str) {
        Some("advanced") => ListLayout::Status,
        _ => ListLayout::Field,
    }
}

fn field_list_header_line() -> Line<'static> {
    Line::from(vec![
        Span::styled(
            fixed_label("takes effect", FIELD_TAKES_EFFECT_COLUMN_WIDTH),
            column_header_style(),
        ),
        Span::styled(
            fixed_label("setting", FIELD_SETTING_COLUMN_WIDTH),
            column_header_style(),
        ),
        Span::styled("value", column_header_style()),
    ])
}

fn status_list_header_line() -> Line<'static> {
    Line::from(vec![
        Span::styled(
            status_column_cell("status", STATUS_COLUMN_WIDTH),
            column_header_style(),
        ),
        Span::styled(
            status_column_cell("item", STATUS_ITEM_COLUMN_WIDTH),
            column_header_style(),
        ),
        Span::styled("detail", column_header_style()),
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
            "No settings match this tab/search.",
            fg_style(Color::Gray),
        ))],
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(Block::default().title("details").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        area,
    );
}

pub fn row_line_for_model(model: &ConfigUiModel, row: UiRowRef) -> Line<'static> {
    row_line_for_layout(model, row, ListLayout::Field)
}

fn row_line_for_layout(model: &ConfigUiModel, row: UiRowRef, layout: ListLayout) -> Line<'static> {
    match row {
        UiRowRef::Field(index) => {
            let field = &model.fields[index];
            Line::from(vec![
                Span::styled(
                    fixed_label(&field.apply_status.summary, FIELD_TAKES_EFFECT_COLUMN_WIDTH),
                    apply_status_style(&field.apply_status),
                ),
                Span::styled(
                    fixed_label(
                        &truncate(field_display_label(field), FIELD_SETTING_COLUMN_WIDTH),
                        FIELD_SETTING_COLUMN_WIDTH,
                    ),
                    field_style(field, config_key_style()),
                ),
                Span::styled(
                    truncate(&field.current_value, FIELD_VALUE_COLUMN_WIDTH),
                    field_style(field, fg_style(Color::Gray)),
                ),
            ])
        }
        UiRowRef::Sidecar(index) => {
            let sidecar = &model.sidecars[index];
            status_row_line(
                sidecar_status_label(sidecar.present),
                sidecar_status_style(sidecar.present),
                &sidecar.name,
                sidecar.path.display().to_string(),
            )
        }
        UiRowRef::FileAction(index) => {
            let action = &model.file_actions[index];
            match layout {
                ListLayout::Field => Line::from(vec![
                    Span::styled(
                        fixed_label(
                            file_action_status_label(action),
                            FIELD_TAKES_EFFECT_COLUMN_WIDTH,
                        ),
                        file_action_status_style(action),
                    ),
                    Span::styled(
                        fixed_label(
                            &truncate(&action.label, FIELD_SETTING_COLUMN_WIDTH),
                            FIELD_SETTING_COLUMN_WIDTH,
                        ),
                        config_key_style(),
                    ),
                    Span::styled(
                        truncate(&action.path.display().to_string(), FIELD_VALUE_COLUMN_WIDTH),
                        fg_style(Color::Gray),
                    ),
                ]),
                ListLayout::Status => status_row_line(
                    file_action_status_label(action),
                    file_action_status_style(action),
                    &action.label,
                    action.path.display().to_string(),
                ),
            }
        }
        UiRowRef::Diagnostic(index) => {
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
        UiRowRef::NativeStatus(index) => {
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

fn field_title_lines(field: &ConfigUiField) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(Span::styled(
        field_display_label(field).to_string(),
        config_key_style().add_modifier(Modifier::BOLD),
    ))];
    if !field.display_label.is_empty() && field.display_label != field.path {
        lines.push(detail_line("path", &field.path));
    }
    lines.push(Line::from(""));
    lines
}

fn field_style(field: &ConfigUiField, default: Style) -> Style {
    if field.state == ConfigUiValueState::Invalid {
        state_style(field.state)
    } else {
        default
    }
}

pub fn default_field_detail_lines(field: &ConfigUiField) -> Vec<Line<'static>> {
    let mut lines = field_title_lines(field);
    lines.extend([
        detail_line("state", state_label(field.state)),
        detail_line("current", &field.current_value),
        detail_line("default", &field.default_value),
        detail_line("type", &field.kind),
        detail_line("takes effect", &field.apply_status.label),
        detail_line("after save", &field.apply_status.detail),
    ]);
    if !field.validation.is_empty() {
        lines.push(detail_line("validation", &field.validation));
    }
    if !field.allowed_values.is_empty() && !is_scalar_enum_field(field) {
        lines.push(detail_line("allowed", &field.allowed_values.join(", ")));
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

pub fn single_choice_field_detail_lines(field: &ConfigUiField) -> Vec<Line<'static>> {
    let selected_value = parse_rendered_json_string(&field.current_value)
        .unwrap_or_else(|| field.current_value.clone());
    let mut lines = default_field_detail_lines(field);
    lines.push(Line::from(""));
    append_single_choice_options(&mut lines, field, &selected_value, None);
    lines
}

pub fn single_choice_detail_lines(
    field: &ConfigUiField,
    edit: &ConfigUiEditState,
) -> Vec<Line<'static>> {
    let selected_value = edit.input.as_str();
    let mut lines = field_title_lines(field);
    lines.push(detail_line("selected", selected_value));
    lines.push(Line::from(""));

    append_single_choice_options(
        &mut lines,
        field,
        selected_value,
        Some(
            edit.choice_index
                .min(field.allowed_values.len().saturating_sub(1)),
        ),
    );

    lines
}

fn append_single_choice_options(
    lines: &mut Vec<Line<'static>>,
    field: &ConfigUiField,
    selected_value: &str,
    highlighted_index: Option<usize>,
) {
    for (index, value) in field.allowed_values.iter().enumerate() {
        let highlighted = highlighted_index == Some(index);
        let selected = value == selected_value;
        lines.push(choice_option_line(
            value,
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
    let enabled_values = parse_string_list_values(field, &edit.input).unwrap_or_default();
    let enabled_set = enabled_values.iter().cloned().collect::<BTreeSet<_>>();
    let mut lines = field_title_lines(field);
    lines.push(detail_line(
        "enabled",
        &format!("{}/{}", enabled_set.len(), field.allowed_values.len()),
    ));
    if is_ordered_string_list_field(field) {
        let order = if enabled_values.is_empty() {
            "none".to_string()
        } else {
            enabled_values.join(", ")
        };
        lines.push(detail_line("order", &order));
    }
    lines.push(Line::from(""));

    for (index, value) in field.allowed_values.iter().enumerate() {
        let selected = index
            == edit
                .choice_index
                .min(field.allowed_values.len().saturating_sub(1));
        let enabled = enabled_set.contains(value);
        lines.push(choice_option_line(
            value,
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
        Line::from(Span::styled(
            sidecar.name.clone(),
            bold_fg_style(Color::Cyan),
        )),
        Line::from(""),
        detail_line("path", &sidecar.path.display().to_string()),
        detail_line("state", sidecar_status_label(sidecar.present)),
        detail_line("owner", owner_label(sidecar.owner)),
        detail_line("write", write_detail_label(sidecar.read_only)),
    ]
}

pub fn file_action_detail_lines(action: &ConfigUiFileAction) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(Span::styled(
            action.label.clone(),
            config_key_style().add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        detail_line("source", &action.source_id),
        detail_line("action", &action.action_id),
        detail_line("path", &action.path.display().to_string()),
        detail_line("state", file_action_status_label(action)),
        detail_line("write", write_detail_label(action.read_only)),
        detail_line(
            "create",
            if action.create_if_missing {
                "offered when missing"
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
        Line::from(Span::styled(
            diagnostic.headline.clone(),
            bold_fg_style(if diagnostic.blocking {
                Color::Red
            } else {
                Color::Yellow
            }),
        )),
        Line::from(""),
        detail_line("path", &diagnostic.path),
        detail_line("status", &diagnostic.status),
        detail_line("blocking", if diagnostic.blocking { "yes" } else { "no" }),
    ];
    lines.push(Line::from(""));
    for detail in &diagnostic.detail_lines {
        lines.push(Line::from(detail.clone()));
    }
    lines
}

pub fn native_status_detail_lines(status: &ConfigUiNativeStatus) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(Span::styled(
            status.label.clone(),
            bold_fg_style(Color::Cyan),
        )),
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
        let field = &app.model.fields[edit.field_index];
        let editing = edit_status_line(field, edit);
        let status = app.notice.as_ref().map_or_else(
            || edit_control_line(field, edit.mode),
            |notice| notice_line(notice, area.width as usize),
        );
        frame.render_widget(Paragraph::new(vec![editing, status]), area);
        return;
    }

    let notice = app.notice.as_ref().map_or_else(
        || normal_control_line(app),
        |notice| notice_line(notice, area.width as usize),
    );
    let search = if app.search_active {
        format!("search: {}_", app.search)
    } else if app.search.is_empty() {
        "/ search".to_string()
    } else {
        "Esc clears search".to_string()
    };
    let mut controls = raw_line(["q quit  ", "Tab tabs  ", "j/k move  "]);
    controls
        .spans
        .push(Span::styled(search, fg_style(Color::Yellow)));
    frame.render_widget(Paragraph::new(vec![notice, controls]), area);
}

fn notice_line(notice: &ConfigUiNotice, width: usize) -> Line<'static> {
    let style = if notice.is_error {
        bold_fg_style(Color::Red)
    } else {
        fg_style(Color::Green)
    };
    Line::from(Span::styled(truncate(&notice.text, width), style))
}

fn edit_status_line(field: &ConfigUiField, edit: &ConfigUiEditState) -> Line<'static> {
    let value = match edit.mode {
        ConfigUiEditMode::Text => format!("{}_", edit.input),
        ConfigUiEditMode::Choice if is_scalar_enum_field(field) => {
            single_choice_status_value(field, edit)
        }
        ConfigUiEditMode::Choice => edit.input.clone(),
        ConfigUiEditMode::MultiChoice => multi_choice_status_value(field, edit),
    };
    Line::from(vec![
        Span::styled("editing: ", fg_style(Color::Yellow)),
        Span::styled(field.path.clone(), config_key_style()),
        Span::raw(" = "),
        Span::styled(value, fg_style(Color::White)),
    ])
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
    let primary = if is_bool_field(field) {
        "Enter/Space stage  e edit"
    } else if is_scalar_enum_field(field) {
        "Enter/e/Space picker"
    } else if is_enum_string_list_field(field) {
        "Enter/e picker"
    } else if structured_only_edit_notice(field).is_some() {
        "structured view only"
    } else {
        "Enter/e edit"
    };
    setting_control_line(primary, field)
}

fn setting_control_line(primary: &'static str, field: &ConfigUiField) -> Line<'static> {
    if field.has_default_value() {
        raw_line([primary, "  u reset default"])
    } else {
        Line::from(primary)
    }
}

fn edit_control_line(field: &ConfigUiField, mode: ConfigUiEditMode) -> Line<'static> {
    match mode {
        ConfigUiEditMode::Text => raw_line(["Enter save  ", "Esc cancel  ", "Ctrl+u clear"]),
        ConfigUiEditMode::Choice if is_scalar_enum_field(field) => raw_line([
            "hjkl/Arrows move  ",
            "Space select  ",
            "Enter save  ",
            "Esc cancel",
        ]),
        ConfigUiEditMode::Choice => raw_line(["Space toggle  ", "Enter save  ", "Esc cancel"]),
        ConfigUiEditMode::MultiChoice if is_ordered_string_list_field(field) => raw_line([
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

pub fn state_label(state: ConfigUiValueState) -> &'static str {
    match state {
        ConfigUiValueState::Explicit => "explicit",
        ConfigUiValueState::Defaulted => "default",
        ConfigUiValueState::Unset => "unset",
        ConfigUiValueState::Invalid => "invalid",
    }
}

pub fn state_style(state: ConfigUiValueState) -> Style {
    match state {
        ConfigUiValueState::Explicit => fg_style(Color::Green),
        ConfigUiValueState::Defaulted => fg_style(Color::Cyan),
        ConfigUiValueState::Unset => fg_style(Color::Yellow),
        ConfigUiValueState::Invalid => bold_fg_style(Color::Red),
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
    if action.disabled_reason.is_some() {
        "error"
    } else if action.read_only {
        "read-only"
    } else if action.exists {
        "existing"
    } else if action.create_if_missing {
        "missing"
    } else {
        "absent"
    }
}

pub fn file_action_status_style(action: &ConfigUiFileAction) -> Style {
    match file_action_status_label(action) {
        "error" => bold_fg_style(Color::Red),
        "existing" => fg_style(Color::Green),
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
    value
        .chars()
        .take(limit.saturating_sub(3))
        .collect::<String>()
        + "..."
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

    fn span_width(line: &Line<'_>, index: usize) -> usize {
        line.spans[index].content.chars().count()
    }

    fn test_model(state: ConfigUiValueState) -> ConfigUiModel {
        ConfigUiModel {
            active_config_path: PathBuf::from("/home/alex/.config/acme/settings.jsonc"),
            cursor_config_path: PathBuf::from("/home/alex/.config/acme/cursors.jsonc"),
            default_cursor_config_path: PathBuf::from("/runtime/acme/default_cursors.jsonc"),
            active_config_exists: true,
            config_owner: ConfigUiPathOwner::User,
            config_read_only: false,
            sources: Vec::new(),
            tabs: vec!["general".to_string()],
            fields: vec![ConfigUiField {
                source_id: DEFAULT_CONFIG_SOURCE_ID.to_string(),
                path: "core.debug_mode".to_string(),
                display_label: String::new(),
                tab: "general".to_string(),
                kind: "bool".to_string(),
                current_value: "false".to_string(),
                edit_value: "false".to_string(),
                default_value: "false".to_string(),
                state,
                description: String::new(),
                allowed_values: Vec::new(),
                validation: String::new(),
                rebuild_required: false,
                apply_status: ConfigUiApplyStatus {
                    summary: "after app restart".to_string(),
                    label: "after app restart".to_string(),
                    detail: "Restart the app after saving".to_string(),
                    pending: true,
                },
                edit_behavior: ConfigUiEditBehavior::Default,
            }],
            file_actions: Vec::new(),
            sidecars: Vec::new(),
            native_config_statuses: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn source(
        tab: &str,
        label: &str,
        exists: bool,
        owner: ConfigUiPathOwner,
        read_only: bool,
    ) -> ConfigUiSource {
        ConfigUiSource {
            id: tab.to_string(),
            tab: tab.to_string(),
            label: label.to_string(),
            path: PathBuf::from(format!("/home/alex/.config/acme/{tab}.toml")),
            exists,
            owner,
            read_only,
        }
    }

    // Defends: selected file-backed tabs drive header source, path, owner, and write-mode metadata.
    #[test]
    fn header_uses_selected_config_source_metadata() {
        let mut model = test_model(ConfigUiValueState::Explicit);
        let owner = ConfigUiPathOwner::HomeManager;
        model.tabs = vec!["settings".to_string(), "keys".to_string()];
        model.sources = vec![
            source("settings", "Settings", true, ConfigUiPathOwner::User, false),
            source("keys", "Keybindings", false, owner, true),
        ];

        let metadata = header_metadata(&model, 1);
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

    // Defends: callers without sources and the reserved advanced tab keep global fallback metadata.
    #[test]
    fn header_falls_back_to_model_config_metadata() {
        let metadata = header_metadata(&test_model(ConfigUiValueState::Explicit), 0);
        assert_eq!(metadata.source_label, None);
        assert!(metadata.source_path.contains("settings.jsonc"));
        assert_eq!(metadata.owner, "user");
        assert_eq!(metadata.mode, "writable");

        let line = header_metadata_line(&metadata, "ok", Style::default(), 160);
        assert!(!rendered_text(&line).contains("source:"));

        let mut model = test_model(ConfigUiValueState::Explicit);
        let owner = ConfigUiPathOwner::HomeManager;
        model.tabs = vec!["settings".to_string(), "advanced".to_string()];
        model.sources = vec![source("advanced", "Advanced Source", true, owner, true)];

        let metadata = header_metadata(&model, 1);
        assert_eq!(metadata.source_label, None);
        assert!(metadata.source_path.contains("settings.jsonc"));
        assert_eq!(metadata.owner, "user");
        assert_eq!(metadata.mode, "writable");
    }

    // Defends: source labels are bounded and omitted before they crowd out path context.
    #[test]
    fn header_bounds_source_label_for_narrow_widths() {
        let metadata = HeaderMetadata {
            source_label: Some("Very Long Source Label".to_string()),
            source_path: "/home/alex/.config/acme/settings.toml".to_string(),
            owner: "user",
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
        let model = test_model(ConfigUiValueState::Explicit);
        let line = row_line_for_model(&model, UiRowRef::Field(0));

        assert_eq!(
            rendered_cells(&line),
            vec!["after app restart", "core.debug_mode", "false"]
        );
        assert!(!rendered_text(&line).contains("explicit"));
    }

    // Defends: removing the state column still leaves visible names for the remaining settings columns.
    #[test]
    fn field_header_names_remaining_columns() {
        assert_eq!(
            rendered_cells(&field_list_header_line()),
            vec!["takes effect", "setting", "value"]
        );
    }

    // Defends: display labels improve visible text while details keep the stable field path.
    #[test]
    fn field_display_label_replaces_visible_label_but_keeps_path_detail() {
        let mut model = test_model(ConfigUiValueState::Explicit);
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

    // Defends: invalid field rows remain visibly exceptional even without the normal state column.
    #[test]
    fn invalid_field_row_keeps_error_style_on_setting_and_value() {
        let model = test_model(ConfigUiValueState::Invalid);
        let line = row_line_for_model(&model, UiRowRef::Field(0));

        assert_eq!(
            line.spans[1].style,
            state_style(ConfigUiValueState::Invalid)
        );
        assert_eq!(
            line.spans[2].style,
            state_style(ConfigUiValueState::Invalid)
        );
    }

    // Defends: absent advanced status rows stay neutral instead of warning-colored.
    #[test]
    fn absent_sidecar_status_is_neutral() {
        let mut model = test_model(ConfigUiValueState::Explicit);
        model.sidecars = vec![ConfigUiSidecar {
            name: "Native config".to_string(),
            path: PathBuf::from("/home/alex/.config/acme/native.toml"),
            present: false,
            owner: ConfigUiPathOwner::User,
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

    // Defends: native status rows align with the advanced status/item/detail header.
    #[test]
    fn native_status_rows_use_advanced_status_columns() {
        let mut model = test_model(ConfigUiValueState::Explicit);
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

    // Defends: advanced file actions use status columns without changing normal tab file action rows.
    #[test]
    fn advanced_file_actions_use_status_columns() {
        let mut model = test_model(ConfigUiValueState::Explicit);
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

    // Defends: default reset is visible only when the selected field has a default.
    #[test]
    fn normal_controls_hide_default_reset_without_default() {
        let mut app = ConfigUiApp::new(test_model(ConfigUiValueState::Explicit));
        assert!(rendered_text(&normal_control_line(&app)).contains("u reset default"));

        app.model.fields[0].default_value = NO_CONFIG_DEFAULT_VALUE_LABEL.to_string();
        assert!(!rendered_text(&normal_control_line(&app)).contains("reset default"));
    }

    // Defends: ordered string-list editing exposes selected order and the generic reorder command.
    #[test]
    fn ordered_multichoice_rendering_shows_order_controls() {
        let mut model = test_model(ConfigUiValueState::Explicit);
        let field = &mut model.fields[0];
        field.kind = "string_list".to_string();
        field.current_value = r#"["status","clock"]"#.to_string();
        field.edit_value = field.current_value.clone();
        field.allowed_values = vec![
            "clock".to_string(),
            "status".to_string(),
            "mode".to_string(),
        ];
        field.edit_behavior = ConfigUiEditBehavior::OrderedStringList;
        let edit = ConfigUiEditState {
            field_index: 0,
            input: field.edit_value.clone(),
            mode: ConfigUiEditMode::MultiChoice,
            choice_index: 1,
        };

        assert!(multi_choice_status_value(field, &edit).contains("order status, clock"));
        assert!(
            multi_choice_detail_lines(field, &edit)
                .iter()
                .map(rendered_text)
                .any(|line| line.contains("order") && line.contains("status, clock"))
        );
        assert!(
            rendered_text(&edit_control_line(field, ConfigUiEditMode::MultiChoice))
                .contains("J/K reorder")
        );
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

    // Defends: file action rows expose missing, existing, read-only, and error states without scalar edit columns.
    #[test]
    fn file_action_rows_render_host_file_states() {
        let mut model = test_model(ConfigUiValueState::Explicit);
        model.fields.clear();
        model.file_actions = vec![
            file_action(true, false, true, None),
            file_action(false, false, true, None),
            file_action(true, true, false, None),
            file_action(false, false, true, Some("Path cannot be resolved")),
        ];

        for (index, status) in ["existing", "missing", "read-only", "error"]
            .into_iter()
            .enumerate()
        {
            assert_eq!(
                rendered_cells(&row_line_for_model(&model, UiRowRef::FileAction(index))),
                vec![status, "Native config", "/home/alex/.con..."]
            );
        }
        let error_line = row_line_for_model(&model, UiRowRef::FileAction(3));
        assert_eq!(
            error_line.spans[0].style,
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        );
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
        assert!(
            text.iter()
                .any(|line| line.contains("offered when missing"))
        );
        assert!(
            text.iter()
                .any(|line| line.contains("Path cannot be resolved"))
        );
    }
}
