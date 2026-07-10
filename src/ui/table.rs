use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{
        HighlightSpacing, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table,
        TableState,
    },
};

use crate::app::{ResultsClickGeom, SortDir, SortState};
use crate::filter::CellMatches;
use crate::theme;

/// Greedily pick visible column indices starting at `col_off` to fit `budget`
/// width, given per-column `widths`, a leading `gutter` (row-num col + 1 gap)
/// and a 1-col gap between data columns.
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

/// A cell's text as a `Line`, with the chars at `hits` (byte offsets within
/// `s`) rendered in Yellow+bold — the FZF-style glow.
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

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw_table(
    f: &mut Frame,
    columns: &[String],
    rows: &[Vec<String>],
    inner: Rect,
    (cursor_row, scroll_row, cursor_col, scroll_col): (Option<usize>, usize, usize, usize),
    disp_abs: &[usize],
    offsets: Option<&std::collections::HashMap<usize, CellMatches>>,
    sort: SortState,
) -> (usize, usize, ResultsClickGeom) {
    let ncol = columns.len();
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
    let body_h = (table_area.height as usize).saturating_sub(1).max(1);

    let mut header_cells: Vec<String> = Vec::with_capacity(vis.len() + 1);
    header_cells.push("#".to_string());
    for &c in &vis {
        let arrow = sort
            .filter(|(sc, _)| *sc == c)
            .map(|(_, d)| if d == SortDir::Asc { " ▲" } else { " ▼" })
            .unwrap_or("");
        header_cells.push(format!("{}{}", columns[c], arrow));
    }
    let header =
        Row::new(header_cells).style(Style::default().add_modifier(ratatui::style::Modifier::BOLD));

    let first = scroll_row.min(rows.len().saturating_sub(1));
    let last_vis = (scroll_row + body_h).min(rows.len());
    let data_rows = rows[first..last_vis].iter().enumerate().map(|(rel, r)| {
        let abs = disp_abs.get(first + rel).copied().unwrap_or(first + rel);
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
        .row_highlight_style(theme::ROW_HIGHLIGHT)
        .column_highlight_style(theme::COLUMN_HIGHLIGHT)
        .cell_highlight_style(theme::CELL_HIGHLIGHT)
        .highlight_spacing(HighlightSpacing::Never);

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

    if rows.is_empty() {
        let body = Rect {
            y: table_area.y + 1,
            height: table_area.height.saturating_sub(1),
            ..table_area
        };
        f.render_widget(Paragraph::new("(no rows)").style(theme::PLACEHOLDER), body);
    }

    if vbar_gutter != Rect::ZERO {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_columns_windows_from_offset() {
        let widths = [5usize, 5, 5];
        assert_eq!(visible_columns(&widths, 0, 12, 2), vec![0]);
        assert_eq!(visible_columns(&widths, 0, 15, 2), vec![0, 1]);
        assert_eq!(visible_columns(&widths, 1, 20, 2), vec![1, 2]);
        assert_eq!(visible_columns(&widths, 0, 1, 2), vec![0]);
        assert_eq!(visible_columns(&widths, 5, 1, 2), vec![2]);
    }

    #[test]
    fn highlighted_line_marks_matched_chars() {
        use ratatui::style::Color;
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
