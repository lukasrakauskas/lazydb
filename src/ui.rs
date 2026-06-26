use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap,
    },
    Frame,
};

use crate::app::{App, Focus, FormState, Output, SchemaEntry, SchemaOpt};
use crate::config::Features;
use crate::highlight;
use crate::shortcuts;

pub fn draw(f: &mut Frame, app: &mut App) {
    let main_chunks =
        Layout::default().direction(Direction::Vertical).constraints([
            Constraint::Min(3),
            Constraint::Length(1), // shortcuts bar — active view's keymap
            Constraint::Length(1), // status / log line
        ]).split(f.area());
    let main = main_chunks[0];
    let shortcuts_bar = main_chunks[1];
    let status = main_chunks[2];

    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(34), Constraint::Min(1)])
        .split(main);
    let right = Layout::default()
        .direction(Direction::Vertical)
        // ponytail: fixed 8-row editor pane = 6 content rows + 2 border; results take the rest.
        .constraints([Constraint::Length(8), Constraint::Min(1)])
        .split(cols[1]);

    // ponytail: left column split — Connections (top, room for ~6 rows) +
    // Schema browser (bottom, the rest). Right column stays editor+results.
    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Min(1)])
        .split(cols[0]);
    draw_connections(f, &*app, left[0]);
    draw_schema(f, &*app, left[1]);
    draw_editor(f, &*app, right[0]);
    draw_results(f, &mut *app, right[1]);
    draw_shortcuts_bar(f, &*app, shortcuts_bar);
    draw_status(f, &*app, status);

    if let Some(form) = &app.form {
        draw_form(f, form, f.area());
    }

    if app.features_open {
        draw_features(f, app, f.area());
    }

    if app.confirm_destructive.is_some() {
        draw_confirm_destructive(f, app, f.area());
    }
}

fn block<'a>(title: &'a str, num: &'a str, focused: bool) -> Block<'a> {
    // lazygit-faithful: focused pane = bright accent (cyan) + bold border;
    // unfocused = terminal default (Reset) — muted next to the active border
    // and adaptive to dark/light themes. Mirrors lazygit's
    // ActiveBorderColor ["green","bold"] / InactiveBorderColor ["default"].
    let (color, bold) = if focused {
        (Color::Cyan, Modifier::BOLD)
    } else {
        (Color::Reset, Modifier::empty())
    };
    // lazygit-style plain bracket badges: [1] [2] [3] [4] — no colored box.
    let badge = Style::default().fg(color).add_modifier(bold);
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color).add_modifier(bold))
        .title(Line::from(vec![
            Span::styled(format!("[{num}]"), badge),
            Span::raw(" "),
            Span::raw(title.to_string()),
        ]))
}

fn draw_connections(f: &mut Frame, app: &App, area: Rect) {
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
                Style::default().fg(Color::Green)
            } else {
                Style::default()
            };
            ListItem::new(line).style(style)
        })
        .collect();

    let list = List::new(items)
        .block(block("Connections", "1", app.focus == Focus::Connections))
        .highlight_style(Style::default().bg(Color::Gray).fg(Color::Black).add_modifier(Modifier::BOLD));

    let mut state = ListState::default();
    if !app.config.connections.is_empty() {
        state.select(Some(app.conn_cursor));
    }
    f.render_stateful_widget(list, area, &mut state);
}

fn draw_schema(f: &mut Frame, app: &App, area: Rect) {
    let n = app.schema.len();
    let title = format!("Schema  ·  {n} tables  (Enter: expand / run)");
    let b = block(&title, "4", app.focus == Focus::Schema);
    let inner = b.inner(area);
    f.render_widget(b, area);

    let rows = app.schema_rows();
    if rows.is_empty() {
        f.render_widget(
            Paragraph::new("Connect to load schema.").style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    }
    let focused = app.focus == Focus::Schema;
    let lines: Vec<Line> = rows.iter().enumerate().map(|(i, entry)| {
        let selected = focused && i == app.schema_cursor;
        let style = if selected {
            Style::default().bg(Color::Cyan).fg(Color::Black)
        } else {
            Style::default()
        };
        match entry {
            SchemaEntry::Table(t) => {
                let mark = if app.schema_expanded.contains(t) { "▼" } else { "▶" };
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
    }).collect();
    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_editor(f: &mut Frame, app: &App, area: Rect) {
    // Inline block (not the shared `block()` helper) so we can add a
    // right-aligned query-timing title. ponytail: only the editor needs it.
    let focused = app.focus == Focus::Editor;
    let (color, bold) = if focused {
        (Color::Cyan, Modifier::BOLD)
    } else {
        (Color::Reset, Modifier::empty())
    };
    let badge = Style::default().fg(color).add_modifier(bold);
    let mut b = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color).add_modifier(bold))
        .title_top(Line::from(vec![
            Span::styled("[2]".to_string(), badge),
            Span::raw(" "),
            Span::raw("SQL Editor  (Ctrl+R / F5 to run)".to_string()),
        ]));
    // ponytail: live query timer top-right; ticks via the render loop's
    // ~100ms redraw, holds the final value after the query completes.
    if let Some(t) = app.editor_time_label() {
        b = b.title_top(Line::from(t).alignment(Alignment::Right));
    }
    let inner = b.inner(area);
    f.render_widget(b, area);

    // ponytail: per-line SQL highlighting; block-comment state carries across lines.
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

/// Autocomplete popup anchored at the editor cursor. Up to 6 items; flips
/// above the cursor when there's no room below. ponytail: no border, just a
/// filled rect so it stays a 1-line-per-item overlay.
fn draw_autocomplete(f: &mut Frame, app: &App, area: Rect, cursor: (u16, u16)) {
    let Some(ac) = &app.autocomplete else { return; };
    if ac.items.is_empty() { return; }
    let max_h = 6usize;
    let count = ac.items.len().min(max_h);
    let w = ac.items.iter().take(count).map(|s| s.chars().count()).max().unwrap_or(0) as u16 + 1;
    let h = count as u16;
    let (cx, cy) = cursor;
    let py = if cy + 1 + h <= area.bottom() { cy + 1 } else { cy.saturating_sub(h) };
    let px = cx.min(area.right().saturating_sub(w));
    let rect = Rect { x: px, y: py, width: w, height: h };
    f.render_widget(Clear, rect);
    let lines: Vec<Line> = ac.items.iter().take(count).enumerate().map(|(i, s)| {
        let style = if i == ac.cursor {
            Style::default().bg(Color::Cyan).fg(Color::Black)
        } else {
            Style::default().bg(Color::DarkGray).fg(Color::White)
        };
        Line::from(Span::styled(format!("{:width$}", s, width = w as usize), style))
    }).collect();
    f.render_widget(Paragraph::new(lines), rect);
}

fn draw_results(f: &mut Frame, app: &mut App, area: Rect) {
    app.results_rect = Some(area);
    let title = match &app.output {
        Output::Table { columns, rows, rows_affected, .. } if !columns.is_empty() => {
            format!(
                "Results  ·  {} rows  ·  {} affected  ·  col {}/{}",
                rows.len(),
                rows_affected,
                app.result_col_off + 1,
                columns.len(),
            )
        }
        _ => "Results".to_string(),
    };
    let b = block(&title, "3", app.focus == Focus::Results);
    let inner = b.inner(area);
    f.render_widget(b, area);

    match &app.output {
        Output::Empty => f.render_widget(
            Paragraph::new("No query run yet.").style(Style::default().fg(Color::DarkGray)),
            inner,
        ),
        Output::Message(m) => f.render_widget(
            Paragraph::new(m.as_str()).wrap(Wrap { trim: false }),
            inner,
        ),
        Output::Table { columns, rows, rows_affected, .. } => {
            if columns.is_empty() {
                f.render_widget(
                    Paragraph::new(format!("{rows_affected} rows affected")),
                    inner,
                );
                return;
            }
            draw_table(f, columns, rows, inner, app.result_row_off, app.result_col_off);
        }
    }
}

/// Manual 2D-scrollable grid: content-sized columns, row/col offsets viewport.
/// ponytail: widths measured by char count, not display width, so CJK/emoji will
/// misalign and may overflow a cell. Fine for typical ASCII SQL result sets;
/// swap in unicode-width if you store wide chars in DB columns.
fn draw_table(
    f: &mut Frame,
    columns: &[String],
    rows: &[Vec<String>],
    inner: Rect,
    row_off: usize,
    col_off: usize,
) {
    let ncol = columns.len();
    let widths: Vec<usize> = (0..ncol)
        .map(|c| {
            let mut w = columns[c].chars().count();
            for r in 0..rows.len() {
                if let Some(cell) = rows[r].get(c) {
                    w = w.max(cell.chars().count());
                }
            }
            w
        })
        .collect();

    let rownum_w = rows.len().to_string().len().max(1);
    let gutter = rownum_w + 1;
    let avail = inner.width as usize;
    let body_w = avail.saturating_sub(gutter);
    const SEP: usize = 2; // spaces between columns

    // Greedily pick visible columns starting at col_off.
    let mut vis: Vec<usize> = Vec::new();
    let mut used = 0usize;
    for c in col_off..ncol {
        if !vis.is_empty() && used + widths[c] + SEP > body_w {
            break;
        }
        used += widths[c] + SEP;
        vis.push(c);
        if used >= body_w {
            break;
        }
    }
    if vis.is_empty() {
        vis.push(col_off.min(ncol - 1));
    }

    let header_body: String =
        vis.iter().map(|&c| format!("{}  ", pad(&columns[c], widths[c]))).collect();
    let header_line = Line::from(trunc(&format!("{}{}", " ".repeat(gutter), header_body), avail))
        .style(Style::default().add_modifier(Modifier::BOLD));
    let separator = Line::from("─".repeat(avail)).style(Style::default().fg(Color::DarkGray));

    let mut lines: Vec<Line> = vec![header_line, separator];
    let body_rows = inner.height as usize;
    let last = (row_off + body_rows.saturating_sub(2)).min(rows.len());
    for i in row_off..last {
        let gutter_str = format!("{:>width$} ", i + 1, width = rownum_w);
        let body: String = vis
            .iter()
            .map(|&c| {
                let cell = rows[i].get(c).map(|s| s.as_str()).unwrap_or("");
                format!("{}  ", pad(cell, widths[c]))
            })
            .collect();
        lines.push(Line::from(trunc(&format!("{gutter_str}{body}"), avail)));
    }
    if rows.is_empty() {
        lines.push(Line::from("(no rows)").style(Style::default().fg(Color::DarkGray)));
    }

    f.render_widget(Paragraph::new(lines), inner);
}

fn pad(s: &str, w: usize) -> String {
    let n = s.chars().count();
    if n >= w {
        s.chars().take(w).collect()
    } else {
        let mut out = s.to_string();
        out.push_str(&" ".repeat(w - n));
        out
    }
}

fn trunc(s: &str, w: usize) -> String {
    s.chars().take(w).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pad_trunc_basic() {
        assert_eq!(pad("ab", 5), "ab   ");
        assert_eq!(pad("abcdef", 3), "abc");
        assert_eq!(trunc("abcdef", 3), "abc");
        assert_eq!(trunc("ab", 5), "ab");
    }

    #[test]
    fn row_window_clamps_to_bounds() {
        // mirrors draw_table's windowing math (header+separator = 2 lines)
        let rows: usize = 3;
        let body_rows: usize = 10;
        let row_off: usize = 1;
        let last = (row_off + body_rows.saturating_sub(2)).min(rows);
        assert_eq!((row_off..last).collect::<Vec<_>>(), vec![1, 2]);
    }
}

fn draw_shortcuts_bar(f: &mut Frame, app: &App, area: Rect) {
    // The bottom keybar shows only the shortcuts active in the current view
    // (view-specific + common pane chrome + global), so the hint always
    // matches what the keys actually do right now. Modals and the editor
    // autocomplete sub-mode each surface their own set.
    let view = shortcuts::current_view(
        app.focus,
        app.form.is_some(),
        app.features_open,
        app.confirm_destructive.is_some(),
        app.autocomplete.is_some(),
    );
    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    for (i, b) in shortcuts::bar_bindings(view).enumerate() {
        if i > 0 { spans.push(Span::raw("  ")); }
        spans.push(Span::styled(
            b.keys_display(),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(b.label, Style::default().fg(Color::DarkGray)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let conn = app.db_name.clone().unwrap_or_else(|| "not connected".into());
    let spinner = if app.running_query { " ⏳" } else { "" };
    let left = format!(" {conn}{spinner} | {} ", app.status);
    // ponytail: the key-log inspector shares this line (right side). The
    // shortcut hints moved up to the dedicated shortcuts bar.
    let right = if app.debug_keys {
        format!(" {} ", app.last_key.as_deref().unwrap_or("(none)"))
    } else {
        String::new()
    };
    let line = Line::from(vec![
        Span::raw(left),
        Span::raw(right),
    ]);
    f.render_widget(
        Paragraph::new(line),
        area,
    );
}

fn draw_form(f: &mut Frame, form: &FormState, area: Rect) {
    let w = 64.min(area.width);
    let h = 12.min(area.height);
    let x = area.x + (area.width - w) / 2;
    let y = area.y + (area.height - h) / 2;
    let pop = Rect { x, y, width: w, height: h };
    f.render_widget(Clear, pop);

    let b = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title("New Connection  (Enter: save, Esc: cancel, Tab: next field)")
        .border_style(Style::default().fg(Color::Yellow));
    let inner = b.inner(pop);
    f.render_widget(b, pop);

    let mut lines: Vec<Line> = Vec::new();
    for (i, label) in FormState::LABELS.iter().enumerate() {
        let val = if i == 4 {
            "*".repeat(form.fields[i].len())
        } else {
            form.fields[i].clone()
        };
        let val_style = if i == form.active {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default()
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{label:>9}: "), Style::default().fg(Color::Gray)),
            Span::styled(val, val_style),
        ]));
    }
    f.render_widget(Paragraph::new(lines), inner);

    let cx = inner.x + 11 + form.cursor as u16;
    let cy = inner.y + form.active as u16;
    if cx < inner.right() && cy < inner.bottom() {
        f.set_cursor_position((cx, cy));
    }
}

fn draw_features(f: &mut Frame, app: &App, area: Rect) {
    // ponytail: height = 2 lines per feature (row + gap) + border/title padding.
    let h = (Features::LIST.len() as u16 * 2 + 4).min(area.height);
    let w = 70.min(area.width);
    let x = area.x + (area.width - w) / 2;
    let y = area.y + (area.height - h) / 2;
    let pop = Rect { x, y, width: w, height: h };
    f.render_widget(Clear, pop);

    let b = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title("Features  (Space: toggle  j/k: move  Esc/f/q: close)")
        .border_style(Style::default().fg(Color::Yellow));
    let inner = b.inner(pop);
    f.render_widget(b, pop);

    let mut lines: Vec<Line> = Vec::new();
    for (i, (name, desc)) in Features::LIST.iter().enumerate() {
        let on = app.config.features.get(i);
        let selected = i == app.feature_cursor;
        let mark_style = if selected {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default().fg(if on { Color::Green } else { Color::DarkGray })
        };
        lines.push(Line::from(vec![
            Span::styled(format!(" {} ", if on { "[x]" } else { "[ ]" }), mark_style),
            Span::styled(*name, Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled(*desc, Style::default().fg(Color::DarkGray)),
        ]));
        lines.push(Line::from(""));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

fn draw_confirm_destructive(f: &mut Frame, app: &App, area: Rect) {
    let w = 72.min(area.width);
    let h = 8.min(area.height);
    let x = area.x + (area.width - w) / 2;
    let y = area.y + (area.height - h) / 2;
    let pop = Rect { x, y, width: w, height: h };
    f.render_widget(Clear, pop);

    let sql = app.confirm_destructive.as_deref().unwrap_or("");
    // ponytail: first line only, truncated to fit the modal.
    let display = sql.lines().next().unwrap_or(sql);
    let truncated: String = display.chars().take(58).collect();
    let line = if display.len() > 58 {
        format!(" {truncated}…")
    } else {
        format!(" {display}")
    };

    let b = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Destructive Query ")
        .border_style(Style::default().fg(Color::Red));
    let inner = b.inner(pop);
    f.render_widget(b, pop);

    let msg = vec![
        Line::from(Span::raw(line)),
        Line::from(""),
        Line::from(Span::styled(
            " This will modify or delete data.",
            Style::default().fg(Color::Red),
        )),
        Line::from(" Press  y  to confirm  ·  n / Esc  to cancel"),
    ];
    f.render_widget(Paragraph::new(msg).wrap(Wrap { trim: false }), inner);
}
