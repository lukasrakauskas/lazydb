use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, ListState, Paragraph, Wrap},
};

use crate::app::{App, Focus, Output, SchemaEntry, SchemaOpt};
use crate::highlight;
use crate::theme;

use super::block;
use super::overlay::{draw_autocomplete, draw_edit_bar, draw_filter_bar};
use super::table::draw_table;

pub(crate) fn draw_connections(f: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = app
        .config
        .connections
        .iter()
        .map(|c| {
            let active = app.db_name.as_deref() == Some(c.name.as_str());
            let prefix = if active { "● " } else { "  " };
            let line = Line::from(format!(
                "{prefix}{}  {}@{}:{}",
                c.name, c.username, c.host, c.port
            ));
            let style = if active {
                theme::ACTIVE_CONNECTION
            } else {
                Style::default()
            };
            ListItem::new(line).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(block("Connections", "1", app.focus == Focus::Connections))
        .highlight_style(theme::CONNECTION_HIGHLIGHT);

    let mut state = ListState::default();
    if !app.config.connections.is_empty() {
        state.select(Some(app.conn_cursor));
    }
    f.render_stateful_widget(list, area, &mut state);
}

pub(crate) fn draw_schema(f: &mut Frame, app: &App, area: Rect) {
    let n = app.schema.len();
    let title = format!("Schema  ·  {n} tables  (Enter: expand / run)");
    let b = block(&title, "4", app.focus == Focus::Schema);
    let inner = b.inner(area);
    f.render_widget(b, area);

    let rows = app.schema_rows();
    if rows.is_empty() {
        f.render_widget(
            Paragraph::new("Connect to load schema.").style(theme::PLACEHOLDER),
            inner,
        );
        return;
    }
    let focused = app.focus == Focus::Schema;
    let lines: Vec<Line> = rows
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let selected = focused && i == app.schema_cursor;
            let style = if selected {
                theme::SCHEMA_CURSOR
            } else {
                Style::default()
            };
            match entry {
                SchemaEntry::Table(t) => {
                    let mark = if app.schema_expanded.contains(t) {
                        "▼"
                    } else {
                        "▶"
                    };
                    Line::from(Span::styled(format!(" {mark} {t}"), style))
                }
                SchemaEntry::Leaf { opt, .. } => {
                    let label = match opt {
                        SchemaOpt::Rows => "rows",
                        SchemaOpt::Columns => "columns",
                        SchemaOpt::Constraints => "constraints",
                        SchemaOpt::Indexes => "indexes",
                    };
                    Line::from(Span::styled(format!("    {label}"), style))
                }
            }
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

pub(crate) fn draw_editor(f: &mut Frame, app: &App, area: Rect) {
    let focused = app.focus == Focus::Editor;
    let (border, badge) = if focused {
        (theme::FOCUSED_BORDER, theme::FOCUSED_BADGE)
    } else {
        (theme::UNFOCUSED_BORDER, theme::UNFOCUSED_BADGE)
    };
    let mut b = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border)
        .title_top(Line::from(vec![
            Span::styled("[2]".to_string(), badge),
            Span::raw(" "),
            Span::raw("SQL Editor  (Ctrl+R / F5 to run)".to_string()),
        ]));
    if let Some(t) = app.editor_time_label() {
        b = b.title_top(Line::from(t).alignment(Alignment::Right));
    }
    let inner = b.inner(area);
    f.render_widget(b, area);

    let mut in_block = false;
    let lines: Vec<Line> = app
        .editor
        .lines
        .iter()
        .map(|l| Line::from(highlight::highlight_line(l, &mut in_block)))
        .collect();
    f.render_widget(Paragraph::new(lines), inner);

    if app.focus == Focus::Editor {
        let x = inner.x + app.editor.col as u16;
        let y = inner.y + app.editor.row as u16;
        if x < inner.right() && y < inner.bottom() {
            f.set_cursor_position((x, y));
        }
        draw_autocomplete(f, app, area, (x, y));
    }
}

pub(crate) fn draw_results(f: &mut Frame, app: &mut App, area: Rect) {
    app.results_rect = Some(area);
    let title = match &app.output {
        Output::Table {
            columns,
            rows,
            rows_affected,
            truncated,
            ..
        } if !columns.is_empty() => {
            let (nrows, suffix) = match &app.result_filter {
                Some(f) => (f.matched.len(), format!("  ·  filter: '{}'", f.query)),
                None => (rows.len(), String::new()),
            };
            let cur_row = app.result_cursor_row.map(|i| i + 1).unwrap_or(0);
            let trunc = if *truncated { "  ·  truncated" } else { "" };
            format!(
                "Results  ·  {} rows  ·  {} affected  ·  row {}/{}  col {}/{}{}{}",
                nrows,
                rows_affected,
                cur_row,
                nrows,
                app.result_cursor_col + 1,
                columns.len(),
                suffix,
                trunc,
            )
        }
        _ => "Results".to_string(),
    };
    let b = block(&title, "3", app.focus == Focus::Results);
    let inner = b.inner(area);
    f.render_widget(b, area);

    match &app.output {
        Output::Empty => f.render_widget(
            Paragraph::new("No query run yet.").style(theme::PLACEHOLDER),
            inner,
        ),
        Output::Message(m) => {
            f.render_widget(Paragraph::new(m.as_str()).wrap(Wrap { trim: false }), inner)
        }
        Output::Table {
            columns,
            rows,
            rows_affected,
            ..
        } => {
            if columns.is_empty() {
                f.render_widget(
                    Paragraph::new(format!("{rows_affected} rows affected")),
                    inner,
                );
                return;
            }
            let has_filter = app.result_filter.is_some();
            let has_edit = app.edit_cell.is_some();
            let n_bars = (has_filter as u16) + (has_edit as u16);
            let [bar_area, rest] = if n_bars > 0 {
                Layout::vertical([Constraint::Length(n_bars), Constraint::Min(1)]).areas(inner)
            } else {
                [Rect::ZERO, inner]
            };
            if let Some(rf) = &app.result_filter {
                let [filter_bar, _] =
                    Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(bar_area);
                draw_filter_bar(f, &rf.query, filter_bar);
            }
            if let Some(edit) = &app.edit_cell {
                let edit_y = bar_area.y + if has_filter { 1 } else { 0 };
                let edit_bar = Rect {
                    x: bar_area.x,
                    y: edit_y,
                    width: bar_area.width,
                    height: 1,
                };
                draw_edit_bar(f, edit, edit_bar);
            }
            let table_area = rest;
            let (disp, disp_abs): (Vec<Vec<String>>, Vec<usize>) = match &app.result_filter {
                Some(f) => (
                    f.matched
                        .iter()
                        .filter_map(|&i| rows.get(i).cloned())
                        .collect(),
                    f.matched.clone(),
                ),
                None => (rows.clone(), (0..rows.len()).collect()),
            };
            let offsets = app.result_filter.as_ref().map(|f| &f.offsets);
            let (cr, sr, cc, sc) = (
                app.result_cursor_row,
                app.result_scroll_row,
                app.result_cursor_col,
                app.result_scroll_col,
            );
            let (body_h, vis_cols, geom) = draw_table(
                f,
                columns,
                &disp,
                table_area,
                (cr, sr, cc, sc),
                &disp_abs,
                offsets,
            );
            app.results_click_geom = Some(geom);
            app.results_body_h = body_h;
            app.results_visible_cols = vis_cols;
        }
    }
}
