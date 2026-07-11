use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::App;
use crate::shortcuts;
use crate::theme;

pub(crate) fn draw_shortcuts_bar(f: &mut Frame, app: &App, area: Rect) {
    let view = shortcuts::current_view(
        app.focus,
        app.form.is_some(),
        app.kind_picker_open(),
        app.features_open,
        app.confirm_destructive.is_some(),
        app.confirm_delete.is_some(),
        app.autocomplete.is_some(),
        app.filter_input_open,
        app.edit_cell.is_some(),
        app.export_input.is_some(),
        app.row_insert.is_some(),
        app.cell_inspect.is_some(),
        app.editor_save_input.is_some(),
        app.schema_filter_input_open,
        app.show_help,
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

pub(crate) fn draw_status(f: &mut Frame, app: &App, area: Rect) {
    let conn = app.db_name.as_deref().unwrap_or("not connected");
    let spinner = if app.running_query {
        " ⏳ (Esc/Ctrl+C to cancel)"
    } else {
        ""
    };
    let autocommit = if !app.autocommit { " [TX]" } else { "" };
    let left = format!(" {conn}{autocommit}{spinner} | {} ", app.status);
    let right = if app.debug_keys {
        format!(" {} ", app.last_key.as_deref().unwrap_or("(none)"))
    } else {
        String::new()
    };
    let line = Line::from(vec![Span::raw(left), Span::raw(right)]);
    f.render_widget(Paragraph::new(line), area);
}
