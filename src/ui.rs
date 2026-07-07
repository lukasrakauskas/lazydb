use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, BorderType, Borders, Clear, HighlightSpacing, List, ListItem, ListState, Paragraph,
        Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table, TableState, Wrap,
    },
};

use crate::app::{
    App, EditCellState, Focus, FormState, Output, ResultsClickGeom, SchemaEntry, SchemaOpt,
};
use crate::config::Features;
use crate::filter::CellMatches;
use crate::highlight;
use crate::shortcuts;
use crate::theme;

pub fn draw(f: &mut Frame, app: &mut App) {
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(1), // shortcuts bar — active view's keymap
            Constraint::Length(1), // status / log line
        ])
        .split(f.area());
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

    if app.confirm_delete.is_some() {
        draw_confirm_delete(f, app, f.area());
    }
}

fn block<'a>(title: &'a str, num: &'a str, focused: bool) -> Block<'a> {
    let (border, badge) = if focused {
        (theme::FOCUSED_BORDER, theme::FOCUSED_BADGE)
    } else {
        (theme::UNFOCUSED_BORDER, theme::UNFOCUSED_BADGE)
    };
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border)
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

fn draw_schema(f: &mut Frame, app: &App, area: Rect) {
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

fn draw_editor(f: &mut Frame, app: &App, area: Rect) {
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
    let Some(ac) = &app.autocomplete else {
        return;
    };
    if ac.items.is_empty() {
        return;
    }
    let max_h = 6usize;
    let count = ac.items.len().min(max_h);
    let w = ac
        .items
        .iter()
        .take(count)
        .map(|s| s.chars().count())
        .max()
        .unwrap_or(0) as u16
        + 1;
    let h = count as u16;
    let (cx, cy) = cursor;
    let py = if cy + 1 + h <= area.bottom() {
        cy + 1
    } else {
        cy.saturating_sub(h)
    };
    let px = cx.min(area.right().saturating_sub(w));
    let rect = Rect {
        x: px,
        y: py,
        width: w,
        height: h,
    };
    f.render_widget(Clear, rect);
    let lines: Vec<Line> = ac
        .items
        .iter()
        .take(count)
        .enumerate()
        .map(|(i, s)| {
            let style = if i == ac.cursor {
                theme::AUTOCOMPLETE_CURSOR
            } else {
                theme::AUTOCOMPLETE_ITEM
            };
            Line::from(Span::styled(
                format!("{:width$}", s, width = w as usize),
                style,
            ))
        })
        .collect();
    f.render_widget(Paragraph::new(lines), rect);
}

fn draw_results(f: &mut Frame, app: &mut App, area: Rect) {
    app.results_rect = Some(area);
    let title = match &app.output {
        Output::Table {
            columns,
            rows,
            rows_affected,
            truncated,
            ..
        } if !columns.is_empty() => {
            // Title shows displayed count when filtered, plus the filter query.
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
            // Reserve space for the filter bar (top) and/or edit bar (bottom).
            // ponytail: each reserves 1 line only when active, so unfiltered/
            // non-editing result sets keep full height.
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
            // Displayed rows + their absolute indices (filtered subset, or the
            // full set with 0..n). The abs indices drive the row-number column
            // and the matched-char highlight lookup.
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

/// Manual 2D-scrollable grid: content-sized columns, row/col offsets viewport.
/// ponytail: widths measured by char count, not display width, so CJK/emoji will
/// misalign and may overflow a cell. Fine for typical ASCII SQL result sets;
/// swap in unicode-width if you store wide chars in DB columns.
/// Greedily pick visible column indices starting at `col_off` to fit `budget`
// width, given per-column `widths`, a leading `gutter` (row-num col + 1 gap)
// and a 1-col gap between data columns. `TableState` has no horizontal offset
// in 0.28, so `draw_table` windows columns manually and feeds only `vis` to
// the `Table`.
fn visible_columns(widths: &[usize], col_off: usize, budget: usize, gutter: usize) -> Vec<usize> {
    let ncol = widths.len();
    let mut vis: Vec<usize> = Vec::new();
    let mut used = gutter;
    for (c, &w) in widths.iter().enumerate().skip(col_off) {
        if !vis.is_empty() {
            used += 1;
        }
        if used + w > budget {
            break;
        }
        used += w;
        vis.push(c);
    }
    if vis.is_empty() {
        vis.push(col_off.min(ncol.saturating_sub(1)));
    }
    vis
}

/// ratatui `Table` with row + column + cell highlight and vertical &
// horizontal scrollbars. Cursor (the highlighted + copy-target cell) and
// viewport scroll are independent: we slice visible rows from `scroll_row`
// and visible columns from `scroll_col` ourselves, then set `TableState`'s
// `selected`/`selected_column` to viewport-relative indices (or `None` when
// the cursor is outside the viewport) so ratatui's own auto-follow never
// fights our scroll. Returns `(body_h, visible_cols)` so the caller can feed
// them back to the nav handlers for auto-follow + page sizing.
// ponytail: widths measured by char count, not display width, so CJK/emoji
// will misalign. Fine for ASCII SQL results; swap in unicode-width if needed.
fn draw_table(
    f: &mut Frame,
    columns: &[String],
    rows: &[Vec<String>],
    inner: Rect,
    // (cursor_row, scroll_row, cursor_col, scroll_col)
    // cursor_row is Option (None = deselected). Indices are into `rows`, the
    // displayed set (filtered subset or full).
    (cursor_row, scroll_row, cursor_col, scroll_col): (Option<usize>, usize, usize, usize),
    // Absolute row index for each displayed row (same length as `rows`). Used
    // for the row-number column and to look up match offsets.
    disp_abs: &[usize],
    // When a filter is active, maps abs row → matched byte offsets in the
    // row's tab-joined text, for the FZF-style char highlight. None = no
    // filter, render cells plain.
    offsets: Option<&std::collections::HashMap<usize, CellMatches>>,
) -> (usize, usize, ResultsClickGeom) {
    let ncol = columns.len();
    // Content-width per column (char count).
    let widths: Vec<usize> = columns
        .iter()
        .enumerate()
        .map(|(c, col)| {
            let cell_max = rows
                .iter()
                .filter_map(|r| r.get(c))
                .map(|s| s.chars().count())
                .max()
                .unwrap_or(0);
            col.chars().count().max(cell_max)
        })
        .collect();

    // Reserve a 1-col right gutter for the vertical scrollbar and a 1-row
    // bottom gutter for the horizontal one — but only when there's enough
    // content to scroll, so tiny result sets aren't cropped.
    let [table_area, vbar_gutter] = if rows.len() > 1 {
        Layout::horizontal([Constraint::Min(1), Constraint::Length(1)]).areas(inner)
    } else {
        [inner, Rect::ZERO]
    };
    let [table_area, hbar_gutter] = if ncol > 1 {
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(table_area)
    } else {
        [table_area, Rect::ZERO]
    };

    let rownum_w = rows.len().to_string().len().max(1);
    let budget = table_area.width as usize;
    let vis = visible_columns(&widths, scroll_col, budget, rownum_w + 1);
    let visible_cols = vis.len();
    // Body height in data rows (excl. header). Clamped ≥1 so callers' math
    // never divides by zero before the first real render.
    let body_h = (table_area.height as usize).saturating_sub(1).max(1);

    // First column = row number; then the visible data columns.
    let mut header_cells: Vec<String> = Vec::with_capacity(vis.len() + 1);
    header_cells.push("#".to_string());
    for &c in &vis {
        header_cells.push(columns[c].clone());
    }
    let header = Row::new(header_cells).style(Style::default().add_modifier(Modifier::BOLD));

    // Slice the visible rows ourselves (ratatui's TableState.offset would
    // force the selected row into view — we don't want that when the cursor
    // is off-screen). Row numbers are the absolute row index (via `disp_abs`)
    // so the user sees real indices even in a filtered view.
    let first = scroll_row.min(rows.len().saturating_sub(1));
    let last_vis = (scroll_row + body_h).min(rows.len());
    let data_rows = rows[first..last_vis].iter().enumerate().map(|(rel, r)| {
        let abs = disp_abs.get(first + rel).copied().unwrap_or(first + rel);
        // Per-cell matched offsets for this row (from the active filter), so
        // each visible cell can highlight the chars the query matched within it.
        let matched_cells = offsets
            .and_then(|m| m.get(&abs))
            .cloned()
            .unwrap_or_default();
        let mut cells: Vec<Line> = Vec::with_capacity(vis.len() + 1);
        cells.push(Line::from(format!("{:>width$}", abs + 1, width = rownum_w)));
        for &c in &vis {
            let cell_str = r.get(c).cloned().unwrap_or_default();
            let hits = matched_cells
                .iter()
                .find(|(col, _)| *col == c)
                .map(|(_, o)| o.clone())
                .unwrap_or_default();
            cells.push(highlighted_line(&cell_str, &hits));
        }
        Row::new(cells)
    });

    let mut col_widths: Vec<Constraint> = Vec::with_capacity(vis.len() + 1);
    col_widths.push(Constraint::Length(rownum_w as u16));
    for &c in &vis {
        col_widths.push(Constraint::Length(widths[c] as u16));
    }

    let table = Table::new(data_rows, col_widths)
        .header(header)
        // REVERSED for all three highlight levels so selection is always
        // readable regardless of terminal theme (dark or light). The cell
        // content spans (including matched-char Magenta+bold) still show
        // through because ratatui merges highlight styles with cell styles.
        .row_highlight_style(theme::ROW_HIGHLIGHT)
        .column_highlight_style(theme::COLUMN_HIGHLIGHT)
        .cell_highlight_style(theme::CELL_HIGHLIGHT)
        // no symbol gutter — the row-num column is our gutter
        .highlight_spacing(HighlightSpacing::Never);

    // Viewport-relative selection: highlight the cursor only when it's
    // actually on screen. `selected` is relative to our sliced rows; the
    // Table column index for an absolute col is 1 (row-num col) + its
    // position in `vis`.
    let mut state = TableState::new();
    if let Some(cr) = cursor_row
        && last_vis > first
        && cr >= first
        && cr < last_vis
    {
        state.select(Some(cr - first));
    }
    if let Some(pos) = vis.iter().position(|&c| c == cursor_col) {
        state.selected_column_mut().replace(1 + pos);
    }
    f.render_stateful_widget(table, table_area, &mut state);

    // Empty result: overlay "(no rows)" on the body (header still shows).
    if rows.is_empty() {
        let body = Rect {
            y: table_area.y + 1,
            height: table_area.height.saturating_sub(1),
            ..table_area
        };
        f.render_widget(Paragraph::new("(no rows)").style(theme::PLACEHOLDER), body);
    }

    // Scrollbars reflect the manual offsets.
    if vbar_gutter != Rect::ZERO {
        let body_h = (table_area.height as usize).saturating_sub(1); // minus header
        let mut vstate = ScrollbarState::new(rows.len())
            .position(scroll_row.min(rows.len().saturating_sub(1)))
            .viewport_content_length(body_h);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            vbar_gutter,
            &mut vstate,
        );
    }
    if hbar_gutter != Rect::ZERO {
        let mut hstate = ScrollbarState::new(ncol)
            .position(scroll_col.min(ncol.saturating_sub(1)))
            .viewport_content_length(visible_cols.max(1));
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::HorizontalBottom),
            hbar_gutter,
            &mut hstate,
        );
    }
    // Geometry for click-to-select: the body rect (excl. header) and the
    // x-range of each visible data column. ratatui lays columns out left-to-
    // right with 1-col spacing, the row-num column first — so the first data
    // column starts at table_area.x + rownum_w + 1.
    let body = Rect {
        x: table_area.x,
        y: table_area.y + 1,
        width: table_area.width,
        height: table_area.height.saturating_sub(1),
    };
    let mut cols: Vec<(usize, u16, u16)> = Vec::with_capacity(vis.len());
    let mut x = table_area.x.saturating_add(rownum_w as u16 + 1);
    for &c in &vis {
        let w = widths[c] as u16;
        cols.push((c, x, w));
        x = x.saturating_add(w + 1);
    }
    let geom = ResultsClickGeom { body, cols };

    (body_h, visible_cols, geom)
}

/// One-line edit input rendered below the results table while inline editing
// is active. Shows `edit <col>: <value>` with the cursor at the edit position.
fn draw_edit_bar(f: &mut Frame, edit: &EditCellState, area: Rect) {
    let display = format!("{}: {}", edit.col_name, edit.raw_value);
    let line = Line::from(vec![
        Span::styled("edit ", theme::EDIT_PROMPT),
        Span::styled(display, theme::EDIT_VALUE),
        Span::styled("▏", theme::EDIT_CURSOR),
    ]);
    f.render_widget(Paragraph::new(line), area);
    let x = area.x + 5 + edit.col_name.len() as u16 + 2 + edit.cursor as u16;
    let y = area.y;
    if x < area.right() && y < area.bottom() {
        f.set_cursor_position((x, y));
    }
}

/// One-line filter input rendered above the results table while the filter
// mode is active. Shows a `filter:` prompt, the live query, and a block cursor.
// ponytail: no background — a bg(Black) blends into dark terminals and hides
// the text; a distinct bg would clash with the table's row/col Gray guides.
// Cyan-bold prompt + Gray-bold query (dark, readable — White was too bright).
fn draw_filter_bar(f: &mut Frame, query: &str, area: Rect) {
    let line = Line::from(vec![
        Span::styled("filter: ", theme::FILTER_PROMPT),
        Span::styled(query.to_owned(), theme::FILTER_QUERY),
        Span::styled("▏", theme::FILTER_CURSOR),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

/// A cell's text as a `Line`, with the chars at `hits` (byte offsets within
// `s`) rendered in Yellow+bold — the FZF-style glow showing which chars the
// filter query matched. `hits` empty → a plain line. ponytail: walks
// `char_indices` so multi-byte cells highlight by byte offset (the matcher
// works on bytes); merges consecutive same-style runs into one span.
fn highlighted_line(s: &str, hits: &[usize]) -> Line<'static> {
    if hits.is_empty() {
        return Line::from(s.to_owned());
    }
    let hit: std::collections::HashSet<usize> = hits.iter().copied().collect();
    let mut spans: Vec<Span> = Vec::new();
    let mut buf = String::new();
    let mut buf_hit = false;
    for (b, ch) in s.char_indices() {
        let is_hit = hit.contains(&b);
        if is_hit != buf_hit && !buf.is_empty() {
            let text = std::mem::take(&mut buf);
            spans.push(if buf_hit {
                Span::styled(text, theme::MATCHED_CHAR)
            } else {
                Span::raw(text)
            });
        }
        buf.push(ch);
        buf_hit = is_hit;
    }
    if !buf.is_empty() {
        spans.push(if buf_hit {
            Span::styled(buf, theme::MATCHED_CHAR)
        } else {
            Span::raw(buf)
        });
    }
    Line::from(spans)
}
fn draw_shortcuts_bar(f: &mut Frame, app: &App, area: Rect) {
    // The bottom keybar shows only the shortcuts active in the current view
    // (view-specific + common pane chrome + global), so the hint always
    // matches what the keys actually do right now. Modals and the editor
    // autocomplete sub-mode each surface their own set.
    let view = shortcuts::current_view(
        app.focus,
        app.form.is_some(),
        app.form
            .as_ref()
            .and_then(|f| f.kind_picker.as_ref())
            .is_some(),
        app.features_open,
        app.confirm_destructive.is_some(),
        app.confirm_delete.is_some(),
        app.autocomplete.is_some(),
        app.filter_input_open,
        app.edit_cell.is_some(),
    );
    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    for (i, b) in shortcuts::bar_bindings(view).enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(b.keys_display(), theme::SHORTCUT_KEY));
        spans.push(Span::raw(" "));
        spans.push(Span::styled(b.label, theme::SHORTCUT_LABEL));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let conn = app
        .db_name
        .clone()
        .unwrap_or_else(|| "not connected".into());
    let spinner = if app.running_query {
        " ⏳ (Esc/Ctrl+C to cancel)"
    } else {
        ""
    };
    let left = format!(" {conn}{spinner} | {} ", app.status);
    // ponytail: the key-log inspector shares this line (right side). The
    // shortcut hints moved up to the dedicated shortcuts bar.
    let right = if app.debug_keys {
        format!(" {} ", app.last_key.as_deref().unwrap_or("(none)"))
    } else {
        String::new()
    };
    let line = Line::from(vec![Span::raw(left), Span::raw(right)]);
    f.render_widget(Paragraph::new(line), area);
}

fn draw_form(f: &mut Frame, form: &FormState, area: Rect) {
    let w = 64.min(area.width);
    let h = 12.min(area.height);
    let x = area.x + (area.width - w) / 2;
    let y = area.y + (area.height - h) / 2;
    let pop = Rect {
        x,
        y,
        width: w,
        height: h,
    };
    f.render_widget(Clear, pop);

    let b = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(if form.edit_index.is_some() {
            "Edit Connection  (Enter: save, Esc: cancel, Tab: next, Ctrl+K: pick, Ctrl+T: test)"
        } else {
            "New Connection  (Enter: save, Esc: cancel, Tab: next, Ctrl+K: pick, Ctrl+T: test)"
        })
        .border_style(theme::FORM_BORDER);
    let inner = b.inner(pop);
    f.render_widget(b, pop);

    let mut lines: Vec<Line> = Vec::new();
    let type_style = if form.active == 0 {
        theme::FORM_ACTIVE_FIELD
    } else {
        Style::default()
    };
    lines.push(Line::from(vec![
        Span::styled(format!("{:>9}: ", "Type"), theme::FORM_LABEL),
        Span::styled(format!("{}  ▼", form.kind), type_style),
    ]));
    for (i, label) in FormState::LABELS.iter().enumerate() {
        let val = if i == 4 {
            "*".repeat(form.fields[i].len())
        } else {
            form.fields[i].clone()
        };
        let fld_active = i + 1;
        let val_style = if fld_active == form.active {
            theme::FORM_ACTIVE_FIELD
        } else {
            Style::default()
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{label:>9}: "), theme::FORM_LABEL),
            Span::styled(val, val_style),
        ]));
    }
    f.render_widget(Paragraph::new(lines), inner);

    if form.kind_picker.is_none() {
        let cx = inner.x + 11
            + if form.active == 0 {
                form.kind.len() as u16
            } else {
                form.cursor as u16
            };
        let cy = inner.y + form.active as u16;
        if cx < inner.right() && cy < inner.bottom() {
            f.set_cursor_position((cx, cy));
        }
    }

    if form.kind_picker.is_some() {
        draw_form_kind_picker(f, form, pop);
    }
}

fn draw_form_kind_picker(f: &mut Frame, form: &FormState, pop: Rect) {
    let Some(picker) = &form.kind_picker else { return };
    let n = picker.filtered.len().min(6);
    let w = 30u16.min(pop.width.saturating_sub(12));
    let h = 3 + n as u16; // top border + input + n items + bottom border
    let x = (pop.x + 11).min(pop.right().saturating_sub(w));
    let y = (pop.y + 2).min(pop.bottom().saturating_sub(h));

    let rect = Rect { x, y, width: w, height: h };
    f.render_widget(Clear, rect);

    let b = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(theme::FORM_BORDER);
    let inner = b.inner(rect);
    f.render_widget(b, rect);

    let input = Line::from(vec![
        Span::styled(" ", theme::AUTOCOMPLETE_ITEM),
        Span::styled(&picker.query, theme::AUTOCOMPLETE_ITEM),
        Span::styled("▏", theme::AUTOCOMPLETE_CURSOR),
    ]);
    f.render_widget(
        Paragraph::new(input),
        Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        },
    );

    for (i, &idx) in picker.filtered.iter().take(6).enumerate() {
        let style = if i == picker.cursor {
            theme::AUTOCOMPLETE_CURSOR
        } else {
            theme::AUTOCOMPLETE_ITEM
        };
        let text = format!(
            " {:width$}",
            FormState::KINDS[idx],
            width = (w - 3) as usize
        );
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(text, style))),
            Rect {
                x: inner.x,
                y: inner.y + 1 + i as u16,
                width: inner.width,
                height: 1,
            },
        );
    }

    // Cursor on the input line
    let cx = (inner.x + 1 + picker.query.len() as u16).min(inner.right().saturating_sub(1));
    f.set_cursor_position((cx, inner.y));
}

fn draw_features(f: &mut Frame, app: &App, area: Rect) {
    // ponytail: height = 2 lines per feature (row + gap) + border/title padding.
    let h = (Features::LIST.len() as u16 * 2 + 4).min(area.height);
    let w = 70.min(area.width);
    let x = area.x + (area.width - w) / 2;
    let y = area.y + (area.height - h) / 2;
    let pop = Rect {
        x,
        y,
        width: w,
        height: h,
    };
    f.render_widget(Clear, pop);

    let b = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title("Features  (Space: toggle  j/k: move  Esc/f/q: close)")
        .border_style(theme::FEATURES_BORDER);
    let inner = b.inner(pop);
    f.render_widget(b, pop);

    let mut lines: Vec<Line> = Vec::new();
    for (i, (name, desc)) in Features::LIST.iter().enumerate() {
        let on = app.config.features.get(i);
        let selected = i == app.feature_cursor;
        let mark_style = if selected {
            theme::FEATURE_CURSOR
        } else if on {
            theme::FEATURE_TOGGLE_ON
        } else {
            theme::FEATURE_TOGGLE_OFF
        };
        lines.push(Line::from(vec![
            Span::styled(format!(" {} ", if on { "[x]" } else { "[ ]" }), mark_style),
            Span::styled(*name, Style::default().add_modifier(Modifier::BOLD)),
            Span::raw("  "),
            Span::styled(*desc, theme::FEATURE_DESC),
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
    let pop = Rect {
        x,
        y,
        width: w,
        height: h,
    };
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
        .border_style(theme::DESTRUCTIVE_BORDER);
    let inner = b.inner(pop);
    f.render_widget(b, pop);

    let msg = vec![
        Line::from(Span::raw(line)),
        Line::from(""),
        Line::from(Span::styled(
            " This will modify or delete data.",
            theme::DESTRUCTIVE_TEXT,
        )),
        Line::from(" Press  y  to confirm  ·  n / Esc  to cancel"),
    ];
    f.render_widget(Paragraph::new(msg).wrap(Wrap { trim: false }), inner);
}

fn draw_confirm_delete(f: &mut Frame, app: &App, area: Rect) {
    let w = 72.min(area.width);
    let h = 8.min(area.height);
    let x = area.x + (area.width - w) / 2;
    let y = area.y + (area.height - h) / 2;
    let pop = Rect {
        x,
        y,
        width: w,
        height: h,
    };
    f.render_widget(Clear, pop);

    let line = app
        .confirm_delete
        .and_then(|i| app.config.connections.get(i))
        .map(|c| {
            format!(
                " Delete connection '{}' ({}@{}:{})?",
                c.name, c.username, c.host, c.port
            )
        })
        .unwrap_or_else(|| " Delete selected connection?".into());

    let b = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(" Delete Connection ")
        .border_style(theme::DESTRUCTIVE_BORDER);
    let inner = b.inner(pop);
    f.render_widget(b, pop);

    let msg = vec![
        Line::from(Span::raw(line)),
        Line::from(""),
        Line::from(Span::styled(
            " This action cannot be undone.",
            theme::DESTRUCTIVE_TEXT,
        )),
        Line::from(" Press  Enter  to confirm  ·  Esc  to cancel"),
    ];
    f.render_widget(Paragraph::new(msg).wrap(Wrap { trim: false }), inner);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_columns_windows_from_offset() {
        // widths: 3 cols of width 5. budget fits 2 data cols after a 2-wide gutter.
        // gutter=2; each data col costs width+1 gap (except the first).
        // budget 12 → 2+5=7 fits col 0, +1+5=13 > 12 → stop → [0].
        // budget 15 → 7, 13, +1+5=19 > 15 → stop after col 1 → [0,1].
        let widths = [5usize, 5, 5];
        assert_eq!(visible_columns(&widths, 0, 12, 2), vec![0]);
        assert_eq!(visible_columns(&widths, 0, 15, 2), vec![0, 1]);
        // offset 1 starts the window at col 1; budget 20 fits both remaining.
        assert_eq!(visible_columns(&widths, 1, 20, 2), vec![1, 2]);
        // nothing fits → still returns the offset col (clamped) so one col shows.
        assert_eq!(visible_columns(&widths, 0, 1, 2), vec![0]);
        assert_eq!(visible_columns(&widths, 5, 1, 2), vec![2]); // col_off clamped to last
    }

    #[test]
    fn highlighted_line_marks_matched_chars() {
        use ratatui::style::Color;
        // "jane" with hits at byte offsets 0 and 2 (j, n) → 'j','n' bold
        // magenta, 'a','e' plain. Runs merge same-style chars, so 4 spans:
        // hit(j) | plain(a) | hit(n) | plain(e).
        let line = highlighted_line("jane", &[0, 2]);
        let spans = line.spans;
        let joined: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(joined, "jane");
        assert_eq!(spans.len(), 4, "four runs: j | a | n | e");
        assert!(
            spans[0].style.fg == Some(Color::Magenta),
            "char 'j' should be magenta"
        );
        assert!(
            spans[2].style.fg == Some(Color::Magenta),
            "char 'n' should be magenta"
        );
        assert!(spans[1].style.fg.is_none(), "char 'a' should be plain");
        assert!(spans[3].style.fg.is_none(), "char 'e' should be plain");
    }

    #[test]
    fn highlighted_line_empty_hits_is_plain() {
        let line = highlighted_line("hello", &[]);
        assert_eq!(line.spans.len(), 1);
        assert!(line.spans[0].style.fg.is_none());
    }
}
