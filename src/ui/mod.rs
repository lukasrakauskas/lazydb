mod bar;
mod overlay;
mod pane;
mod table;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders},
};

use crate::app::App;
use crate::theme;

pub fn draw(f: &mut Frame, app: &mut App) {
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
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
        .constraints([Constraint::Length(8), Constraint::Min(1)])
        .split(cols[1]);

    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(8), Constraint::Min(1)])
        .split(cols[0]);

    pane::draw_connections(f, &*app, left[0]);
    pane::draw_schema(f, &*app, left[1]);
    pane::draw_editor(f, &*app, right[0]);
    pane::draw_results(f, &mut *app, right[1]);
    bar::draw_shortcuts_bar(f, &*app, shortcuts_bar);
    bar::draw_status(f, &*app, status);

    if let Some(form) = &app.form {
        overlay::draw_form(f, form, f.area());
    }

    if app.features_open {
        overlay::draw_features(f, app, f.area());
    }

    if app.confirm_destructive.is_some() {
        overlay::draw_confirm_destructive(f, app, f.area());
    }

    if app.confirm_delete.is_some() {
        overlay::draw_confirm_delete(f, app, f.area());
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
