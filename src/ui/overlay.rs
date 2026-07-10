use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};

use crate::app::{App, EditCellState, ExportFormat, FormState};
use crate::config::Features;
use crate::theme;

/// Autocomplete popup anchored at the editor cursor.
pub(crate) fn draw_autocomplete(f: &mut Frame, app: &App, area: Rect, cursor: (u16, u16)) {
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

/// One-line edit input rendered below the results table.
pub(crate) fn draw_edit_bar(f: &mut Frame, edit: &EditCellState, area: Rect) {
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

/// One-line filter input rendered above the results table.
pub(crate) fn draw_filter_bar(f: &mut Frame, query: &str, area: Rect) {
    let line = Line::from(vec![
        Span::styled("filter: ", theme::FILTER_PROMPT),
        Span::styled(query.to_owned(), theme::FILTER_QUERY),
        Span::styled("▏", theme::FILTER_CURSOR),
    ]);
    f.render_widget(Paragraph::new(line), area);
}

pub(crate) fn draw_form(f: &mut Frame, form: &FormState, area: Rect) {
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
        let cx = inner.x
            + 11
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
    let Some(picker) = &form.kind_picker else {
        return;
    };
    let n = picker.filtered.len().min(6);
    let w = 30u16.min(pop.width.saturating_sub(12));
    let h = 3 + n as u16;
    let x = (pop.x + 11).min(pop.right().saturating_sub(w));
    let y = (pop.y + 2).min(pop.bottom().saturating_sub(h));

    let rect = Rect {
        x,
        y,
        width: w,
        height: h,
    };
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
            width = w.saturating_sub(3) as usize
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

    let cx = (inner.x + 1 + picker.query.len() as u16).min(inner.right().saturating_sub(1));
    f.set_cursor_position((cx, inner.y));
}

pub(crate) fn draw_features(f: &mut Frame, app: &App, area: Rect) {
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
            Span::styled(
                *name,
                Style::default().add_modifier(ratatui::style::Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(*desc, theme::FEATURE_DESC),
        ]));
        lines.push(Line::from(""));
    }
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: true }), inner);
}

pub(crate) fn draw_confirm_destructive(f: &mut Frame, app: &App, area: Rect) {
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

/// Editor save path input.
pub(crate) fn draw_save_editor(f: &mut Frame, app: &App, area: Rect) {
    let Some(input) = &app.editor_save_input else {
        return;
    };
    let w = 72.min(area.width);
    let h = 6.min(area.height);
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
        .title(" Save Editor Buffer  (Enter: save, Esc: cancel)")
        .border_style(theme::FEATURES_BORDER);
    let inner = b.inner(pop);
    f.render_widget(b, pop);

    let line = Line::from(vec![
        Span::styled(" Path: ", theme::FORM_LABEL),
        Span::styled(input.path.clone(), Style::default()),
    ]);
    f.render_widget(Paragraph::new(line), inner);
    let cx = inner.x + 7 + input.cursor as u16;
    let cy = inner.y;
    if cx < inner.right() && cy < inner.bottom() {
        f.set_cursor_position((cx, cy));
    }
}

/// Cell inspect popup — shows full cell content.
pub(crate) fn draw_cell_inspect(f: &mut Frame, app: &App, area: Rect) {
    let Some(inspect) = &app.cell_inspect else {
        return;
    };
    let lines: Vec<&str> = inspect.cell_value.lines().collect();
    let view_h = 12.min(area.height.saturating_sub(4) as usize);
    let view_h = view_h.max(3);
    let w = 72.min(area.width.saturating_sub(4));
    let h = (view_h + 4).min(area.height as usize) as u16;
    let x = area.x + (area.width - w) / 2;
    let y = area.y + (area.height - h) / 2;
    let pop = Rect {
        x,
        y,
        width: w,
        height: h,
    };
    f.render_widget(Clear, pop);

    let scroll = inspect.scroll.min(lines.len().saturating_sub(view_h));
    let title = format!(
        " Cell: {}  (row {}, col {})  [Esc: close]",
        inspect.col_name,
        inspect.abs_row + 1,
        inspect.col + 1
    );
    let b = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .title(title)
        .border_style(theme::FOCUSED_BORDER);
    let inner = b.inner(pop);
    f.render_widget(b, pop);
    let visible: Vec<Line> = lines
        .iter()
        .skip(scroll)
        .take(view_h)
        .map(|l| Line::from(Span::raw(*l)))
        .collect();
    f.render_widget(Paragraph::new(visible).wrap(Wrap { trim: false }), inner);
}

/// Export input modal — path + format.
pub(crate) fn draw_export_input(f: &mut Frame, app: &App, area: Rect) {
    let Some(export) = &app.export_input else {
        return;
    };
    let w = 72.min(area.width);
    let h = 10.min(area.height);
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
        .title(" Export Results  (Enter: export, Esc: cancel, Tab: format)")
        .border_style(theme::FEATURES_BORDER);
    let inner = b.inner(pop);
    f.render_widget(b, pop);

    let format_str = ExportFormat::LABELS[export.format as usize];
    let path_style = Style::default();
    let lines = vec![
        Line::from(vec![
            Span::styled(" Path:  ", theme::FORM_LABEL),
            Span::styled(export.path.clone(), path_style),
        ]),
        Line::from(vec![
            Span::styled(" Format: ", theme::FORM_LABEL),
            Span::styled(format_str.to_string(), path_style),
        ]),
    ];
    f.render_widget(Paragraph::new(lines), inner);
    // cursor on path field
    let cx = inner.x + 7 + export.cursor as u16;
    let cy = inner.y;
    if cx < inner.right() && cy < inner.bottom() {
        f.set_cursor_position((cx, cy));
    }
}

/// Row insert inline modal.
pub(crate) fn draw_row_insert(f: &mut Frame, app: &App, area: Rect) {
    let Some(ins) = &app.row_insert else { return };
    let n = ins.columns.len();
    let h = (n + 3).min(16) as u16;
    let w = 80.min(area.width);
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
        .title(" Insert Row  (Enter: confirm, Tab: field, Esc: cancel)")
        .border_style(theme::FEATURES_BORDER);
    let inner = b.inner(pop);
    f.render_widget(b, pop);

    let mut lines: Vec<Line> = Vec::with_capacity(n);
    for (i, col) in ins.columns.iter().enumerate() {
        let val = &ins.values[i];
        let val_style = if i == ins.cursor_col {
            theme::FORM_ACTIVE_FIELD
        } else {
            Style::default()
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{:>16}: ", col), theme::FORM_LABEL),
            Span::styled(val.clone(), val_style),
        ]));
    }
    f.render_widget(Paragraph::new(lines), inner);

    // cursor
    let cx = inner.x + 18 + ins.cursor_char as u16;
    let cy = inner.y + ins.cursor_col as u16;
    if cx < inner.right() && cy < inner.bottom() {
        f.set_cursor_position((cx, cy));
    }
}

pub(crate) fn draw_confirm_delete(f: &mut Frame, app: &App, area: Rect) {
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

pub(crate) fn draw_help(f: &mut Frame, _app: &App, area: Rect) {
    use crate::shortcuts::{View, bar_bindings};
    let w = 80.min(area.width);
    let h = (area.height - 4).min(36);
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
        .title("Help  (?/Esc/q: close)")
        .border_style(theme::FEATURES_BORDER);
    let inner = b.inner(pop);
    f.render_widget(b, pop);

    let views = [
        View::Editor,
        View::Results,
        View::Schema,
        View::Connections,
        View::ResultsFilter,
        View::ResultsEdit,
        View::ResultsRowInsert,
        View::CellInspect,
        View::EditorAutocomplete,
        View::Form,
        View::KindPicker,
        View::Features,
        View::ConfirmDestructive,
        View::ConfirmDelete,
        View::EditorSave,
    ];
    let mut lines: Vec<Line> = Vec::new();
    for v in &views {
        let bs: Vec<_> = bar_bindings(*v).collect();
        if bs.is_empty() {
            continue;
        }
        let mut parts: Vec<Span> = vec![Span::styled(
            format!("{:>24}", format!("{v:?}")),
            theme::SHORTCUT_KEY,
        )];
        for b in &bs {
            parts.push(Span::raw("  "));
            parts.push(Span::styled(b.keys_display(), theme::SHORTCUT_KEY));
            parts.push(Span::raw(" "));
            parts.push(Span::styled(b.label, theme::SHORTCUT_LABEL));
        }
        lines.push(Line::from(parts));
    }
    f.render_widget(Paragraph::new(lines), inner);
}
