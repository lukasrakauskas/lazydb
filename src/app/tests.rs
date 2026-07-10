use super::format_key_event;

fn scroll_down(off: usize, last_row: usize) -> usize {
    off.saturating_add(1).min(last_row)
}
fn scroll_up(off: usize) -> usize {
    off.saturating_sub(1)
}

#[test]
fn row_scroll_clamps_at_bounds() {
    assert_eq!(scroll_down(0, 5), 1);
    assert_eq!(scroll_down(5, 5), 5);
    assert_eq!(scroll_up(3), 2);
}

#[test]
fn col_scroll_clamps_at_bounds() {
    assert_eq!(scroll_down(0, 4), 1);
    assert_eq!(scroll_down(4, 4), 4);
    assert_eq!(scroll_up(0), 0);
}

#[test]
fn key_event_format_reports_modifiers() {
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};
    let shift_enter = KeyEvent::new_with_kind_and_state(
        KeyCode::Enter,
        KeyModifiers::SHIFT,
        KeyEventKind::Press,
        KeyEventState::NONE,
    );
    let s = format_key_event(&shift_enter);
    assert!(s.contains("Enter"), "got: {s}");
    assert!(s.contains("mods=S---"), "SHIFT must be S, got: {s}");

    let plain = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
    assert!(format_key_event(&plain).contains("mods=----"));
}

#[test]
fn history_recall_roundtrip_restores_draft() {
    let mut app = super::App::load().unwrap();
    app.editor = super::Editor::from_text("draft".into());
    app.history = vec!["old".to_string(), "new".to_string()];

    app.recall_history(true);
    assert_eq!(app.editor.text(), "new");
    assert!(app.history_cursor.is_some());

    app.recall_history(true);
    assert_eq!(app.editor.text(), "old");

    app.recall_history(true);
    assert_eq!(app.editor.text(), "old");

    app.recall_history(false);
    assert_eq!(app.editor.text(), "new");
    app.recall_history(false);
    assert_eq!(app.editor.text(), "draft");
    assert!(app.history_cursor.is_none());
}

#[test]
fn schema_rows_expand_to_four_options() {
    let mut app = super::App::load().unwrap();
    let mut schema = std::collections::HashMap::new();
    schema.insert("users".to_string(), vec!["id".to_string()]);
    schema.insert("posts".to_string(), vec!["id".to_string()]);
    app.schema = schema;

    let rows = app.schema_rows();
    assert_eq!(rows.len(), 2);
    assert!(matches!(rows[0], super::SchemaEntry::Table(ref t) if t == "posts"));
    assert!(matches!(rows[1], super::SchemaEntry::Table(ref t) if t == "users"));

    app.schema_expanded.insert("users".to_string());
    let rows = app.schema_rows();
    assert_eq!(rows.len(), 6);
    assert!(matches!(rows[1], super::SchemaEntry::Table(ref t) if t == "users"));
    assert!(
        matches!(rows[2], super::SchemaEntry::Leaf { ref table, opt: super::SchemaOpt::Rows } if table == "users")
    );
    assert!(
        matches!(rows[3], super::SchemaEntry::Leaf { ref table, opt: super::SchemaOpt::Columns } if table == "users")
    );
    assert!(
        matches!(rows[4], super::SchemaEntry::Leaf { ref table, opt: super::SchemaOpt::Constraints } if table == "users")
    );
    assert!(
        matches!(rows[5], super::SchemaEntry::Leaf { ref table, opt: super::SchemaOpt::Indexes } if table == "users")
    );

    app.schema_expanded.remove("users");
    assert_eq!(app.schema_rows().len(), 2);
}

#[test]
fn schema_query_generates_correct_sql() {
    use super::{SchemaOpt, schema_query};
    // mysql arm
    assert_eq!(
        schema_query("users", SchemaOpt::Rows, "mysql"),
        "SELECT * FROM `users` LIMIT 100;"
    );
    assert_eq!(
        schema_query("users", SchemaOpt::Columns, "mysql"),
        "SHOW FULL COLUMNS FROM `users`;"
    );
    assert_eq!(
        schema_query("users", SchemaOpt::Indexes, "mysql"),
        "SHOW INDEX FROM `users`;"
    );
    let c = schema_query("users", SchemaOpt::Constraints, "mysql");
    assert!(c.contains("TABLE_CONSTRAINTS"));
    assert!(c.contains("TABLE_NAME = 'users'"));
    // postgres arm: double-quoted identifier, catalog queries.
    assert_eq!(
        schema_query("users", SchemaOpt::Rows, "postgres"),
        "SELECT * FROM \"users\" LIMIT 100;"
    );
    assert!(
        schema_query("users", SchemaOpt::Columns, "postgres")
            .contains("information_schema.columns")
    );
    assert!(schema_query("users", SchemaOpt::Indexes, "postgres").contains("pg_indexes"));
}

#[test]
fn build_update_sql_quotes_per_backend() {
    use super::build_update_sql;
    let pks = vec!["id".to_string()];
    let vals = vec!["7".to_string()];
    // mysql: backtick identifiers.
    assert_eq!(
        build_update_sql("t", "name", "a'b", &pks, &vals, "mysql"),
        "UPDATE `t` SET `name` = 'a''b' WHERE `id` = '7'"
    );
    // postgres: double-quote identifiers, same value escaping.
    assert_eq!(
        build_update_sql("t", "name", "a'b", &pks, &vals, "postgres"),
        "UPDATE \"t\" SET \"name\" = 'a''b' WHERE \"id\" = '7'"
    );
    // a double-quote in a postgres identifier is doubled.
    assert_eq!(
        build_update_sql("a\"b", "c", "x", &pks, &vals, "postgres"),
        "UPDATE \"a\"\"b\" SET \"c\" = 'x' WHERE \"id\" = '7'"
    );
}

#[test]
fn kind_picker_filters_and_selects() {
    use super::KindPickerState;
    let mut p = KindPickerState::new();
    assert_eq!(p.filtered.len(), 3);
    assert_eq!(p.selected_kind(), Some("mysql"));
    p.set_query("post".into());
    assert_eq!(p.filtered, vec![1usize]);
    assert_eq!(p.selected_kind(), Some("postgres"));
    p.set_query("xyz".into());
    assert!(p.filtered.is_empty());
    assert_eq!(p.selected_kind(), None);
}

#[test]
fn kind_picker_port_auto_update() {
    use super::{FormState, KindPickerState};
    let mut app = super::App::load().unwrap();
    app.form = Some(FormState::new());
    let f = app.form.as_ref().unwrap();
    assert_eq!(f.kind, "mysql");
    assert_eq!(f.fields[2], "3306");
    // Selecting postgres flips the default port 3306→5432.
    let mut p = KindPickerState::new();
    p.set_query("post".into());
    app.form.as_mut().unwrap().kind_picker = Some(p);
    app.form_kind_picker_select();
    let f = app.form.as_ref().unwrap();
    assert_eq!(f.kind, "postgres");
    assert_eq!(f.fields[2], "5432");
    // A user-edited port is preserved.
    app.form.as_mut().unwrap().kind_picker = Some(KindPickerState::new());
    app.form.as_mut().unwrap().fields[2] = "6543".into();
    let mut p = KindPickerState::new();
    p.set_query("post".into());
    app.form.as_mut().unwrap().kind_picker = Some(p);
    app.form_kind_picker_select();
    let f = app.form.as_ref().unwrap();
    assert_eq!(f.kind, "postgres");
    assert_eq!(f.fields[2], "6543");
}

#[test]
fn row_to_json_escapes_and_pairs() {
    let cols = vec!["id".to_string(), "name".to_string()];
    let row = vec!["42".to_string(), "a\"b\\c".to_string()];
    assert_eq!(
        super::row_to_json(&cols, &row),
        "{\"id\":\"42\",\"name\":\"a\\\"b\\\\c\"}"
    );
    let short = vec!["1".to_string()];
    assert_eq!(
        super::row_to_json(&cols, &short),
        "{\"id\":\"1\",\"name\":\"\"}"
    );
}

#[test]
fn csv_escapes_special_fields() {
    let cols = vec!["a".to_string(), "b".to_string()];
    let rows = vec![
        vec!["1".to_string(), "x,y".to_string()],
        vec!["2".to_string(), "he said \"hi\"".to_string()],
    ];
    let csv = super::result_to_csv(&cols, &rows);
    assert_eq!(csv, "a,b\n1,\"x,y\"\n2,\"he said \"\"hi\"\"\"\n");
}

use super::{App, Focus, Output};
use crossterm::event::{
    KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};

fn results_app(nrows: usize, ncols: usize, body_h: usize, vis_cols: usize) -> App {
    let mut app = App::load().unwrap();
    app.focus = Focus::Results;
    app.output = Output::Table {
        columns: (0..ncols).map(|c| format!("c{c}")).collect(),
        rows: (0..nrows)
            .map(|r| (0..ncols).map(|c| format!("{r}.{c}")).collect())
            .collect(),
        rows_affected: nrows as u64,
        elapsed_ms: 0,
        truncated: false,
    };
    app.results_body_h = body_h;
    app.results_visible_cols = vis_cols;
    app
}

fn press(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new_with_kind_and_state(
        code,
        KeyModifiers::NONE,
        KeyEventKind::Press,
        KeyEventState::NONE,
    ));
}

#[test]
fn results_cursor_auto_follows_viewport() {
    let mut app = results_app(30, 5, 5, 5);
    for _ in 0..6 {
        press(&mut app, KeyCode::Char('j'));
    }
    assert_eq!(app.result_cursor_row, Some(5));
    assert_eq!(
        app.result_scroll_row, 1,
        "viewport scrolls to keep cursor at bottom"
    );
    press(&mut app, KeyCode::Char('k'));
    assert_eq!(app.result_cursor_row, Some(4));
    assert_eq!(
        app.result_scroll_row, 1,
        "no scroll up while cursor still visible"
    );
    for _ in 0..5 {
        press(&mut app, KeyCode::Char('k'));
    }
    assert_eq!(app.result_cursor_row, Some(0));
    assert_eq!(
        app.result_scroll_row, 0,
        "viewport follows when cursor hits top"
    );
}

#[test]
fn results_page_scroll_is_independent_of_cursor() {
    let mut app = results_app(30, 5, 5, 5);
    for _ in 0..4 {
        press(&mut app, KeyCode::Char('j'));
    }
    assert_eq!(app.result_cursor_row, Some(3));
    let cursor_before = app.result_cursor_row;
    press(&mut app, KeyCode::PageDown);
    assert_eq!(
        app.result_cursor_row, cursor_before,
        "cursor must not move on PgDn"
    );
    assert_eq!(app.result_scroll_row, 5, "viewport scrolled one page");
    assert!(app.result_cursor_row.unwrap() < app.result_scroll_row);
    for _ in 0..10 {
        press(&mut app, KeyCode::PageDown);
    }
    assert_eq!(app.result_scroll_row, 25);
}

#[test]
fn results_home_end_jump_cursor_and_follow() {
    let mut app = results_app(30, 5, 5, 5);
    press(&mut app, KeyCode::End);
    assert_eq!(app.result_cursor_row, Some(29));
    assert_eq!(
        app.result_scroll_row, 25,
        "End scrolls so the last row is at the viewport bottom"
    );
    press(&mut app, KeyCode::Home);
    assert_eq!(app.result_cursor_row, Some(0));
    assert_eq!(app.result_scroll_row, 0);
}

#[test]
fn results_mouse_wheel_scrolls_viewport_only() {
    let mut app = results_app(30, 5, 5, 5);
    app.results_rect = Some(ratatui::layout::Rect::new(0, 0, 40, 10));
    for _ in 0..2 {
        press(&mut app, KeyCode::Char('j'));
    }
    let cursor_before = app.result_cursor_row;
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 5,
        row: 5,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(
        app.result_cursor_row, cursor_before,
        "wheel never moves the cursor"
    );
    assert_eq!(app.result_scroll_row, 1, "wheel scrolls the viewport");
}

#[test]
fn results_copy_uses_cursor_not_scroll() {
    let mut app = results_app(30, 2, 5, 2);
    press(&mut app, KeyCode::Char('j'));
    assert_eq!(app.result_cursor_row, Some(0));
    press(&mut app, KeyCode::PageDown);
    assert_eq!(app.result_cursor_row, Some(0));
    assert!(app.result_scroll_row > 0);
    press(&mut app, KeyCode::Char('y'));
    assert!(
        app.status.starts_with("Copied row 1 as JSON"),
        "got: {}",
        app.status
    );
}

#[test]
fn click_to_cell_maps_body_and_header() {
    use super::{ResultsClickGeom, click_to_cell};
    use ratatui::layout::Rect;
    let geom = ResultsClickGeom {
        body: Rect::new(0, 2, 20, 5),
        cols: vec![(0, 3, 4), (1, 8, 4), (2, 13, 4)],
    };
    assert_eq!(click_to_cell(&geom, 5, 4, 3), (Some(6), Some(0)));
    assert_eq!(click_to_cell(&geom, 5, 9, 4), (Some(7), Some(1)));
    assert_eq!(click_to_cell(&geom, 5, 15, 6), (Some(9), Some(2)));
    assert_eq!(click_to_cell(&geom, 5, 9, 1), (None, Some(1)));
    assert_eq!(click_to_cell(&geom, 5, 0, 3), (Some(6), None));
    assert_eq!(click_to_cell(&geom, 5, 7, 3), (Some(6), None));
    assert_eq!(click_to_cell(&geom, 5, 4, 10), (None, Some(0)));
}

fn click_geom() -> super::ResultsClickGeom {
    use ratatui::layout::Rect;
    super::ResultsClickGeom {
        body: Rect::new(0, 2, 20, 5),
        cols: vec![(0, 3, 4), (1, 8, 4), (2, 13, 4)],
    }
}

#[test]
fn results_click_selects_cell_and_focuses() {
    let mut app = results_app(30, 5, 5, 5);
    app.results_rect = Some(ratatui::layout::Rect::new(0, 0, 40, 10));
    app.results_click_geom = Some(click_geom());
    app.result_scroll_row = 5;
    app.focus = Focus::Editor;
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 9,
        row: 4,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.result_cursor_row, Some(7));
    assert_eq!(app.result_cursor_col, 1);
    assert!(
        app.focus == Focus::Results,
        "click focuses the results pane"
    );
}

#[test]
fn results_click_header_selects_column_only() {
    let mut app = results_app(30, 5, 5, 5);
    app.results_rect = Some(ratatui::layout::Rect::new(0, 0, 40, 10));
    app.results_click_geom = Some(click_geom());
    app.result_cursor_row = Some(12);
    app.handle_mouse(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 15,
        row: 1,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(app.result_cursor_col, 2);
    assert_eq!(
        app.result_cursor_row,
        Some(12),
        "header click must not move the row cursor"
    );
}

#[test]
fn results_deselect_clears_cursor() {
    let mut app = results_app(30, 5, 5, 5);
    press(&mut app, KeyCode::Char('j'));
    assert_eq!(app.result_cursor_row, Some(0));
    press(&mut app, KeyCode::Char('d'));
    assert_eq!(app.result_cursor_row, None);
    press(&mut app, KeyCode::Char('y'));
    assert!(
        app.status.contains("No row selected"),
        "got: {}",
        app.status
    );
}

#[test]
fn results_down_from_deselected_selects_first_row() {
    let mut app = results_app(30, 5, 5, 5);
    assert_eq!(app.result_cursor_row, None);
    press(&mut app, KeyCode::Char('j'));
    assert_eq!(app.result_cursor_row, Some(0));
    let mut app2 = results_app(30, 5, 5, 5);
    press(&mut app2, KeyCode::Char('k'));
    assert_eq!(app2.result_cursor_row, Some(29));
}

#[test]
fn results_filter_narrows_and_ranks_by_score() {
    let mut app = results_app(30, 2, 5, 2);
    app.set_filter_query("5");
    let matched = app.result_filter.as_ref().unwrap().matched.clone();
    assert!(
        matched.contains(&5) && matched.contains(&15) && matched.contains(&25),
        "matched: {matched:?}"
    );
    assert_eq!(app.displayed_count(), matched.len());
    assert_eq!(app.result_cursor_row, None);
}

#[test]
fn results_filter_empty_query_keeps_all_rows() {
    let mut app = results_app(30, 2, 5, 2);
    app.set_filter_query("");
    assert_eq!(app.displayed_count(), 30);
}

#[test]
fn results_filter_clear_restores_full_set() {
    let mut app = results_app(30, 2, 5, 2);
    app.set_filter_query("5");
    assert!(app.displayed_count() < 30);
    app.clear_filter();
    assert!(app.result_filter.is_none());
    assert_eq!(app.displayed_count(), 30);
    assert_eq!(app.result_cursor_row, None);
}

#[test]
fn results_filter_live_typing_appends_and_refilters() {
    let mut app = results_app(30, 2, 5, 2);
    press(&mut app, KeyCode::Char('/'));
    assert!(app.result_filter.is_some(), "/ opens filter mode");
    app.handle_key(KeyEvent::new_with_kind_and_state(
        KeyCode::Char('1'),
        KeyModifiers::NONE,
        KeyEventKind::Press,
        KeyEventState::NONE,
    ));
    let after_1 = app.displayed_count();
    app.handle_key(KeyEvent::new_with_kind_and_state(
        KeyCode::Char('5'),
        KeyModifiers::NONE,
        KeyEventKind::Press,
        KeyEventState::NONE,
    ));
    let q = app.result_filter.as_ref().unwrap().query.clone();
    assert_eq!(q, "15");
    assert!(
        after_1 >= app.displayed_count(),
        "narrowing query must not grow the set"
    );
}

#[test]
fn results_filter_backspace_edits_query() {
    let mut app = results_app(30, 2, 5, 2);
    app.set_filter_query("15");
    assert!(app.filter_input_open);
    app.handle_key(KeyEvent::new_with_kind_and_state(
        KeyCode::Backspace,
        KeyModifiers::NONE,
        KeyEventKind::Press,
        KeyEventState::NONE,
    ));
    assert_eq!(app.result_filter.as_ref().unwrap().query, "1");
}

#[test]
fn results_filter_accept_keeps_filter_closes_input() {
    let mut app = results_app(30, 2, 5, 2);
    app.set_filter_query("5");
    assert!(app.filter_input_open);
    assert!(app.result_filter.is_some());
    let matched = app.displayed_count();
    assert_eq!(matched, 3);
    press(&mut app, KeyCode::Enter);
    assert!(!app.filter_input_open, "accept closes the input mode");
    assert!(
        app.result_filter.is_some(),
        "accept keeps the filter applied"
    );
    assert_eq!(
        app.displayed_count(),
        matched,
        "filtered set unchanged after accept"
    );
}

#[test]
fn results_filter_reopen_edits_committed_query() {
    let mut app = results_app(30, 2, 5, 2);
    app.set_filter_query("5");
    press(&mut app, KeyCode::Enter);
    assert!(!app.filter_input_open);
    assert!(app.result_filter.is_some());
    press(&mut app, KeyCode::Char('/'));
    assert!(
        app.filter_input_open,
        "/ re-opens the input on a committed filter"
    );
    assert_eq!(app.result_filter.as_ref().unwrap().query, "5");
    press(&mut app, KeyCode::Char('/'));
    assert!(app.result_filter.is_none());
    assert!(!app.filter_input_open);
}

#[test]
fn results_filter_backspace_no_op_when_input_closed() {
    let mut app = results_app(30, 2, 5, 2);
    app.set_filter_query("5");
    press(&mut app, KeyCode::Enter);
    let q_before = app.result_filter.as_ref().unwrap().query.clone();
    app.handle_key(KeyEvent::new_with_kind_and_state(
        KeyCode::Backspace,
        KeyModifiers::NONE,
        KeyEventKind::Press,
        KeyEventState::NONE,
    ));
    assert_eq!(app.result_filter.as_ref().unwrap().query, q_before);
}

#[test]
fn results_filter_enter_with_empty_query_cancels() {
    let mut app = results_app(30, 2, 5, 2);
    press(&mut app, KeyCode::Char('/'));
    assert!(app.result_filter.is_some());
    press(&mut app, KeyCode::Enter);
    assert!(
        app.result_filter.is_none(),
        "empty accept should cancel the filter"
    );
}

#[test]
fn results_filter_esc_cancels() {
    let mut app = results_app(30, 2, 5, 2);
    app.set_filter_query("5");
    press(&mut app, KeyCode::Esc);
    assert!(app.result_filter.is_none());
    assert_eq!(app.displayed_count(), 30);
}

#[test]
fn form_from_connection_prefills_fields() {
    use crate::db::Connection;
    let c = Connection {
        name: "prod".into(),
        kind: "mysql".into(),
        host: "db.example.com".into(),
        port: 3307,
        username: "u".into(),
        password: "p".into(),
        database: "d".into(),
        ssl: false,
        use_keychain: false,
        ssh_enabled: false,
        ssh_host: String::new(),
        ssh_port: 22,
        ssh_user: String::new(),
        ssh_keyfile: String::new(),
    };
    let f = super::FormState::from_connection(2, &c);
    assert_eq!(f.edit_index, Some(2));
    assert_eq!(f.kind, "mysql");
    assert_eq!(f.fields[0], "prod");
    assert_eq!(f.fields[1], "db.example.com");
    assert_eq!(f.fields[2], "3307");
    assert_eq!(f.fields[3], "u");
    assert_eq!(f.fields[4], "p");
    assert_eq!(f.fields[5], "d");
}

#[test]
fn save_form_edits_in_place() {
    use crate::config::HOME_LOCK;
    use crate::db::Connection;
    // ponytail: serialize with every other HOME-mutating test (config::tests).
    let _lock = HOME_LOCK.lock().unwrap();
    let tmp = std::env::temp_dir().join(format!("lazydb-edit-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    let old = std::env::var_os("HOME");
    unsafe { std::env::set_var("HOME", &tmp) };

    let mut app = super::App::load().unwrap();
    app.config.connections.push(Connection {
        name: "old".into(),
        kind: "mysql".into(),
        host: "h".into(),
        port: 3306,
        username: "u".into(),
        password: "p".into(),
        database: "d".into(),
        ssl: false,
        use_keychain: false,
        ssh_enabled: false,
        ssh_host: String::new(),
        ssh_port: 22,
        ssh_user: String::new(),
        ssh_keyfile: String::new(),
    });
    app.conn_cursor = 0;
    app.edit_selected();
    assert!(app.form.is_some());
    assert_eq!(app.form.as_ref().unwrap().edit_index, Some(0));
    app.form.as_mut().unwrap().fields[0] = "new".into();
    app.save_form();
    assert_eq!(
        app.config.connections.len(),
        1,
        "edit must replace, not append"
    );
    assert_eq!(app.config.connections[0].name, "new");
    assert_eq!(app.conn_cursor, 0);
    assert!(app.form.is_none());

    if let Some(o) = old {
        unsafe { std::env::set_var("HOME", o) }
    } else {
        unsafe { std::env::remove_var("HOME") }
    }
    std::fs::remove_dir_all(&tmp).ok();
}
