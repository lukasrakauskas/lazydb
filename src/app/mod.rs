mod job;
mod types;
mod util;

pub use job::{Job, JobResult, spawn_job};
pub use types::*;
pub use util::*;

use std::collections::{HashMap, HashSet};
use std::io::Stdout;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::layout::Rect;
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::autocomplete;
use crate::config::{Config, Features};
use crate::db::{self, CancelSlot, Connection, Database, ExecCtx, StatementResult, ssh::SshTunnel};
use crate::editor::Editor;
use crate::filter::CellMatches;
use crate::shortcuts::{self, Action, View};
use crate::ui;

type Term = Terminal<CrosstermBackend<Stdout>>;

pub struct App {
    pub config: Config,
    pub conn_cursor: usize,
    pub db: Option<Box<dyn Database>>,
    pub db_name: Option<String>,
    pending_db: Option<Box<dyn Database>>,
    pub editor: Editor,
    pub focus: Focus,
    pub output: Output,
    pub running: bool,
    pub running_query: bool,
    pub query_start: Option<Instant>,
    pub form: Option<FormState>,
    rx: Option<Receiver<JobResult>>,
    pub status: String,
    pub result_cursor_row: Option<usize>,
    pub result_scroll_row: usize,
    pub result_cursor_col: usize,
    pub result_scroll_col: usize,
    pub results_body_h: usize,
    pub results_visible_cols: usize,
    pub results_rect: Option<Rect>,
    pub results_click_geom: Option<ResultsClickGeom>,
    pub result_filter: Option<ResultFilter>,
    pub filter_input_open: bool,
    pub show_help: bool,
    pub features_open: bool,
    pub feature_cursor: usize,
    pub confirm_destructive: Option<String>,
    pub confirm_delete: Option<usize>,
    pub debug_keys: bool,
    pub last_key: Option<String>,
    pub autocomplete: Option<Autocomplete>,
    pub schema: HashMap<String, Vec<String>>,
    pub schema_views: Vec<String>,
    pub schema_filter: Option<String>,
    pub schema_filter_input_open: bool,
    pub schema_cursor: usize,
    pub schema_expanded: HashSet<String>,
    pub history: Vec<String>,
    pub history_cursor: Option<usize>,
    pub history_draft: Option<String>,
    pub edit_cell: Option<EditCellState>,
    pub edit_cell_pending: Option<EditCellPending>,
    pub pending_cell_update: Option<(usize, usize, String)>,
    // ponytail: one cancel slot reused per job (Arc<AtomicU32> conn-id, 0 = idle).
    cancel: CancelSlot,
    cancel_requested: bool,
    testing_connection: bool,
    // ponytail: multi-statement result sets; active_result indexes into this.
    all_results: Vec<StatementResult>,
    active_result: usize,
    // ponytail: P2 sort — (col_index, direction); None = unsorted.
    pub sort: SortState,
    // ponytail: P2 cell inspect popup.
    pub cell_inspect: Option<CellInspect>,
    // ponytail: P2 export input modal.
    pub export_input: Option<ExportInput>,
    // ponytail: P2 row insert state.
    pub row_insert: Option<RowInsertState>,
    // ponytail: P2 row delete — shares confirm flow.
    #[allow(dead_code)]
    confirm_row_delete: Option<(usize, String, Vec<String>, Vec<String>)>,
    // ponytail: SSH tunnel subprocess — killed on disconnect/drop.
    ssh_tunnel: Option<SshTunnel>,
    // ponytail: PR3 resizable editor — height in terminal rows.
    pub editor_height: u16,
    // ponytail: PR3 save-editor path input.
    pub editor_save_input: Option<ExportInput>,
    // ponytail: PR14 autocommit state (true = each statement commits immediately).
    pub autocommit: bool,
}

impl App {
    pub fn load() -> Result<Self> {
        let config = Config::load();
        let conn_cursor = config
            .last_connection
            .filter(|i| *i < config.connections.len())
            .unwrap_or(0);
        Ok(Self {
            history: config.history.clone(),
            config,
            conn_cursor,
            db: None,
            db_name: None,
            pending_db: None,
            editor: Editor::new(),
            focus: Focus::Connections,
            output: Output::Empty,
            running: true,
            running_query: false,
            query_start: None,
            form: None,
            rx: None,
            status: "Press 'n' to add a connection, then Enter to connect.".into(),
            result_cursor_row: None,
            result_scroll_row: 0,
            result_cursor_col: 0,
            result_scroll_col: 0,
            results_body_h: 0,
            results_visible_cols: 0,
            results_rect: None,
            results_click_geom: None,
            result_filter: None,
            filter_input_open: false,
            show_help: false,
            features_open: false,
            feature_cursor: 0,
            confirm_destructive: None,
            confirm_delete: None,
            debug_keys: false,
            last_key: None,
            autocomplete: None,
            schema: HashMap::new(),
            schema_views: Vec::new(),
            schema_filter: None,
            schema_filter_input_open: false,
            schema_cursor: 0,
            schema_expanded: HashSet::new(),
            history_cursor: None,
            history_draft: None,
            edit_cell: None,
            edit_cell_pending: None,
            pending_cell_update: None,
            cancel: CancelSlot::new(),
            cancel_requested: false,
            testing_connection: false,
            all_results: Vec::new(),
            active_result: 0,
            sort: None,
            cell_inspect: None,
            export_input: None,
            row_insert: None,
            confirm_row_delete: None,
            ssh_tunnel: None,
            editor_height: 8,
            editor_save_input: None,
            autocommit: true,
        })
    }

    pub fn load_script(&mut self, text: String) {
        self.editor = Editor::from_text(text);
        self.focus = Focus::Editor;
    }

    fn connect_selected(&mut self) {
        if self.config.connections.is_empty() {
            return;
        }
        // Persist which connection we're connecting to.
        self.config.last_connection = Some(self.conn_cursor);
        let _ = self.config.save();
        let mut conn = self.config.connections[self.conn_cursor].clone();
        if conn.ssh_enabled {
            match SshTunnel::start(&conn) {
                Ok((tuned, tunnel)) => {
                    conn = tuned;
                    self.ssh_tunnel = Some(tunnel);
                }
                Err(e) => {
                    self.status = format!("SSH tunnel failed: {e}");
                    return;
                }
            }
        } else {
            self.ssh_tunnel = None;
        }
        let timeout = conn
            .query_timeout_secs
            .filter(|&s| s > 0)
            .map(Duration::from_secs)
            .or_else(|| self.config.query_timeout());
        match db::open(&conn, timeout) {
            Ok(db) => {
                crate::log::info("connect", &[("name", &conn.name)]);
                self.testing_connection = false;
                self.pending_db = Some(db.boxed_clone());
                self.rx = Some(spawn_job(Job::Ping(db, conn.name.clone())));
                self.running_query = true;
                self.status = format!("Connecting to {}…", conn.name);
            }
            Err(e) => self.status = format!("Failed to open: {e}"),
        }
    }

    fn confirm_delete_selected(&mut self) {
        if self.config.connections.is_empty() {
            return;
        }
        self.confirm_delete = Some(self.conn_cursor);
        self.status = "Delete connection? Press Enter to confirm, Esc to cancel.".into();
    }

    fn delete_connection_at(&mut self, idx: usize) {
        if idx >= self.config.connections.len() {
            return;
        }
        self.config.connections.remove(idx);
        if self.conn_cursor >= self.config.connections.len() && self.conn_cursor > 0 {
            self.conn_cursor -= 1;
        }
        let _ = self.config.save();
        self.status = "Connection deleted.".into();
    }

    fn run_query(&mut self) {
        self.autocomplete = None;
        if self.running_query {
            self.status = "A query is already running.".into();
            return;
        }
        let sql = self.editor.text();
        if sql.trim().is_empty() {
            self.status = "Editor is empty.".into();
            return;
        }
        if is_destructive(&sql) {
            self.confirm_destructive = Some(sql);
            self.status = "Destructive query — press 'y' to confirm, 'n' to cancel.".into();
            return;
        }
        self.execute_sql(sql);
    }

    fn execute_sql(&mut self, sql: String) {
        let Some(db) = self.db.as_ref() else {
            self.status = "Not connected — select a connection and press Enter.".into();
            return;
        };
        let db = db.boxed_clone();
        let readable_binary = self.config.features.readable_binary;
        if self.history.last().map(String::as_str) != Some(sql.as_str()) {
            if self.history.len() >= 100 {
                self.history.remove(0);
            }
            self.history.push(sql.clone());
        }
        // Persist history to config file after each query.
        self.config.history = self.history.clone();
        let _ = self.config.save();
        self.history_cursor = None;
        self.history_draft = None;
        let head: String = sql.chars().take(120).collect();
        crate::log::info("query", &[("sql_head", &head)]);
        self.cancel.clear();
        self.rx = Some(spawn_job(Job::Query(
            db,
            sql,
            self.exec_ctx(readable_binary),
        )));
        self.running_query = true;
        self.query_start = Some(Instant::now());
        self.status = "Running query…".into();
        self.result_cursor_row = None;
        self.result_filter = None;
        self.filter_input_open = false;
        self.edit_cell = None;
        self.edit_cell_pending = None;
        self.result_scroll_row = 0;
        self.result_cursor_col = 0;
        self.result_scroll_col = 0;
        self.all_results = Vec::new();
        self.active_result = 0;
    }

    fn save_form(&mut self) {
        let Some(form) = self.form.as_ref() else {
            return;
        };
        if form.fields[0].trim().is_empty() {
            self.status = "Name is required.".into();
            return;
        }
        let kind = form.kind.clone();
        let port: u16 = form.fields[2]
            .parse()
            .unwrap_or(FormState::default_port(&kind));
        let edit_index = form.edit_index;
        let fields = form.fields.clone();
        let use_keychain = false; // ponytail: no UI toggle yet; user sets in TOML.
        let mut conn = Connection {
            name: fields[0].clone(),
            kind,
            host: fields[1].clone(),
            port,
            username: fields[3].clone(),
            password: fields[4].clone(),
            database: fields[5].clone(),
            ssl: false,
            use_keychain,
            ssh_enabled: false,
            ssh_host: String::new(),
            ssh_port: 22,
            ssh_user: String::new(),
            ssh_keyfile: String::new(),
            query_timeout_secs: None,
        };
        if conn.use_keychain && !conn.password.is_empty() {
            if let Err(e) = crate::db::keychain_store(&conn.name, &conn.password) {
                self.status = format!("Keychain store failed: {e}");
                return;
            }
            conn.password = String::new();
        }
        if let Some(i) = edit_index.filter(|i| *i < self.config.connections.len()) {
            self.config.connections[i] = conn;
            self.conn_cursor = i;
        } else {
            self.config.connections.push(conn);
            self.conn_cursor = self.config.connections.len() - 1;
        }
        match self.config.save() {
            Ok(()) => {
                self.form = None;
                self.status = "Saved. Press Enter to connect.".into();
            }
            Err(e) => self.status = format!("Save failed: {e}"),
        }
    }

    fn apply_job(&mut self, res: JobResult) {
        self.running_query = false;
        match res {
            JobResult::Ping(r) => {
                if self.testing_connection {
                    self.testing_connection = false;
                    self.status = match &r {
                        Ok(name) => format!("Connection OK: {name}"),
                        Err(e) => format!("Connection failed: {e}"),
                    };
                    return;
                }
                match r {
                    Ok(name) => {
                        self.db = self.pending_db.take();
                        self.db_name = Some(name.clone());
                        self.status = format!("Connected to {name}. Loading schema…");
                        self.output = Output::Message(format!("Connected to {name}."));
                        self.schema.clear();
                        if let Some(db) = self.db.as_ref() {
                            self.rx = Some(spawn_job(Job::Schema(db.boxed_clone())));
                        }
                    }
                    Err(e) => {
                        self.pending_db = None;
                        self.status = format!("Connection failed: {e}");
                        self.output = Output::Message(format!("Connection failed: {e}"));
                    }
                }
            }
            JobResult::Query(r) => {
                let cancelled = self.cancel_requested;
                self.cancel_requested = false;
                match r {
                    Ok(er) => {
                        let ms = er.elapsed_ms.to_string();
                        let rows = er.rows.len().to_string();
                        crate::log::info("query_done", &[("ms", &ms), ("rows", &rows)]);
                        self.all_results = er.all_results;
                        self.active_result = self.all_results.len().saturating_sub(1);
                        let active = self.active_result;
                        let r = self
                            .all_results
                            .get(active)
                            .cloned()
                            .unwrap_or(StatementResult {
                                columns: er.columns,
                                rows: er.rows,
                                rows_affected: er.rows_affected,
                                truncated: er.truncated,
                            });
                        self.output = Output::Table {
                            columns: r.columns,
                            rows: r.rows,
                            rows_affected: r.rows_affected,
                            elapsed_ms: er.elapsed_ms,
                            truncated: r.truncated,
                        };
                        self.status = if cancelled {
                            "Query OK (cancel did not take effect).".into()
                        } else if er.truncated {
                            "Query OK (truncated — row cap reached).".into()
                        } else {
                            "Query OK.".into()
                        };
                        self.query_start = None;
                    }
                    Err(e) => {
                        if cancelled {
                            crate::log::info("query_cancelled", &[("err", &e)]);
                            self.output = Output::Message("Query cancelled.".into());
                            self.status = "Query cancelled.".into();
                        } else {
                            crate::log::error("query_err", &[("err", &e)]);
                            self.output = Output::Message(format!("Error: {e}"));
                            self.status = "Query failed.".into();
                        }
                        self.query_start = None;
                    }
                }
            }
            JobResult::Schema(r) => match r {
                Ok(map) => {
                    let n = map.len();
                    self.schema = map;
                    self.schema_cursor = 0;
                    self.schema_expanded.clear();
                    self.schema_filter = None;
                    self.schema_filter_input_open = false;
                    self.status = format!("Schema loaded: {n} tables.");
                    if let Some(db) = self.db.as_ref() {
                        self.rx = Some(spawn_job(Job::Views(db.boxed_clone())));
                    }
                }
                Err(e) => self.status = format!("Schema load failed: {e}"),
            },
            JobResult::Views(r) => match r {
                Ok(views) => {
                    let n = self.schema.len();
                    self.schema_views = views;
                    self.status = format!(
                        "Schema loaded: {n} tables, {} views.",
                        self.schema_views.len()
                    );
                }
                Err(e) => self.status = format!("Views load failed: {e}"),
            },
            JobResult::PrimaryKeys(r) => match r {
                Ok(pk_cols) => {
                    let pending = match self.edit_cell_pending.take() {
                        Some(p) => p,
                        None => return,
                    };
                    match &self.output {
                        Output::Table { columns, rows, .. }
                            if columns.len() == pending.columns_len
                                && rows.len() == pending.rows_len =>
                        {
                            let pk_vals: Vec<String> = pk_cols
                                .iter()
                                .filter_map(|pk| {
                                    let idx = columns.iter().position(|c| c == pk)?;
                                    rows.get(pending.abs_row)?.get(idx).cloned()
                                })
                                .collect();
                            if pk_cols.is_empty() {
                                self.status = "Cannot edit: table has no primary key.".into();
                                return;
                            }
                            if pk_vals.len() != pk_cols.len() {
                                self.status =
                                    "Cannot edit: primary key columns not in result set.".into();
                                return;
                            }
                            self.edit_cell = Some(EditCellState {
                                raw_value: pending.cell_value.clone(),
                                abs_row: pending.abs_row,
                                col: pending.col,
                                col_name: pending.col_name.clone(),
                                table: pending.table.clone(),
                                pk_cols,
                                pk_vals,
                                cursor: pending.cell_value.len(),
                                original_value: pending.cell_value.clone(),
                            });
                            self.status = "Editing cell — Enter to save, Esc to cancel.".into();
                        }
                        _ => {
                            self.status = "Cannot edit: result set has changed.".into();
                        }
                    }
                }
                Err(e) => self.status = format!("Primary key lookup failed: {e}"),
            },
            JobResult::UpdateCell(r) => match r {
                Ok(er) => {
                    if let Some((row, col, new_val)) = self.pending_cell_update.take()
                        && let Output::Table { rows, .. } = &mut self.output
                        && let Some(r) = rows.get_mut(row)
                        && let Some(c) = r.get_mut(col)
                    {
                        *c = new_val;
                    }
                    self.status = format!("Cell updated ({} rows affected).", er.rows_affected);
                }
                Err(e) => self.status = format!("Update failed: {e}"),
            },
        }
    }

    pub fn kind_picker_open(&self) -> bool {
        self.form
            .as_ref()
            .and_then(|f| f.kind_picker.as_ref())
            .is_some()
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        self.last_key = Some(format_key_event(&key));
        let kind_picker = self.kind_picker_open();
        let view = shortcuts::current_view(
            self.focus,
            self.form.is_some(),
            kind_picker,
            self.features_open,
            self.confirm_destructive.is_some(),
            self.confirm_delete.is_some(),
            self.autocomplete.is_some(),
            self.filter_input_open,
            self.edit_cell.is_some(),
            self.export_input.is_some(),
            self.row_insert.is_some(),
            self.cell_inspect.is_some(),
            self.editor_save_input.is_some(),
            self.schema_filter_input_open,
            self.show_help,
        );
        if self.running_query {
            let is_cancel = key.code == KeyCode::Esc
                || (key.modifiers.contains(KeyModifiers::CONTROL)
                    && key.code == KeyCode::Char('c'));
            // ponytail: only intercept in plain focus views — modals keep their own Esc.
            if is_cancel
                && matches!(
                    view,
                    View::Connections | View::Editor | View::Results | View::Schema
                )
            {
                self.cancel_query();
                return;
            }
        }
        if let Some(action) = shortcuts::match_key(view, key) {
            self.apply_action(view, action);
            return;
        }
        self.handle_text_input(view, key);
    }

    fn apply_action(&mut self, view: View, action: Action) {
        use Action::*;
        match action {
            Quit => self.running = false,
            RunQuery => self.run_query(),
            FocusNext => {
                self.autocomplete = None;
                self.focus = match self.focus {
                    Focus::Connections => Focus::Editor,
                    Focus::Editor => Focus::Results,
                    Focus::Results => Focus::Schema,
                    Focus::Schema => Focus::Connections,
                };
            }
            FocusConnections => {
                self.autocomplete = None;
                self.focus = Focus::Connections;
            }
            FocusEditor => {
                self.autocomplete = None;
                self.focus = Focus::Editor;
            }
            FocusResults => {
                self.autocomplete = None;
                self.focus = Focus::Results;
            }
            FocusSchema => {
                self.autocomplete = None;
                self.focus = Focus::Schema;
            }
            ToggleKeyLog => self.debug_keys = !self.debug_keys,
            ToggleHelp => self.show_help = !self.show_help,
            ToggleFeatures => {
                self.features_open = true;
                self.feature_cursor = 0;
            }

            MoveDown => match view {
                View::Connections => {
                    let n = self.config.connections.len();
                    if n > 0 && self.conn_cursor + 1 < n {
                        self.conn_cursor += 1;
                    }
                }
                View::Results => {
                    let last = match self.result_dims() {
                        (0, _) => return,
                        (n, _) => n.saturating_sub(1),
                    };
                    self.result_cursor_row = Some(match self.result_cursor_row {
                        None => 0,
                        Some(i) => i.saturating_add(1).min(last),
                    });
                    self.scroll_cursor_row_into_view();
                }
                View::Schema => {
                    let rows = self.schema_rows();
                    if !rows.is_empty() {
                        let last = rows.len() - 1;
                        self.schema_cursor = self.schema_cursor.saturating_add(1).min(last);
                    }
                }
                _ => {}
            },
            MoveUp => match view {
                View::Connections => {
                    let n = self.config.connections.len();
                    if n > 0 && self.conn_cursor > 0 {
                        self.conn_cursor -= 1;
                    }
                }
                View::Results => {
                    let last = match self.result_dims() {
                        (0, _) => return,
                        (n, _) => n.saturating_sub(1),
                    };
                    self.result_cursor_row = Some(match self.result_cursor_row {
                        None => last,
                        Some(i) => i.saturating_sub(1),
                    });
                    self.scroll_cursor_row_into_view();
                }
                View::Schema => self.schema_cursor = self.schema_cursor.saturating_sub(1),
                _ => {}
            },
            MoveRight => {
                let last = match &self.output {
                    Output::Table { columns, .. } => columns.len().saturating_sub(1),
                    _ => return,
                };
                self.result_cursor_col = self.result_cursor_col.saturating_add(1).min(last);
                self.scroll_cursor_col_into_view();
            }
            MoveLeft => {
                self.result_cursor_col = self.result_cursor_col.saturating_sub(1);
                self.scroll_cursor_col_into_view();
            }
            PageDown => {
                let (nrows, vh) = self.result_dims();
                let max_scroll = nrows.saturating_sub(vh);
                self.result_scroll_row = self.result_scroll_row.saturating_add(vh).min(max_scroll);
            }
            PageUp => {
                let (_, vh) = self.result_dims();
                self.result_scroll_row = self.result_scroll_row.saturating_sub(vh);
            }
            Home => {
                self.result_cursor_row = Some(0);
                self.result_scroll_row = 0;
            }
            End => {
                let (nrows, vh) = self.result_dims();
                self.result_cursor_row = Some(nrows.saturating_sub(1));
                self.result_scroll_row = nrows.saturating_sub(vh);
            }

            Deselect => {
                // ponytail: Esc in cell-inspect closes it rather than deselecting
                if self.cell_inspect.is_some() {
                    self.cell_inspect = None;
                    self.status = "Inspect closed.".into();
                } else {
                    self.result_cursor_row = None;
                }
            }

            ToggleFilter => match (&self.result_filter, self.filter_input_open) {
                (None, _) => self.set_filter_query(""),
                (Some(_), false) => self.filter_input_open = true,
                (Some(_), true) => self.clear_filter(),
            },
            FilterAccept => {
                let empty = self
                    .result_filter
                    .as_ref()
                    .is_none_or(|f| f.query.is_empty());
                if empty {
                    self.clear_filter();
                } else {
                    self.filter_input_open = false;
                }
            }
            FilterCancel => self.clear_filter(),
            FilterBackspace => {
                if self.filter_input_open
                    && let Some(f) = self.result_filter.as_mut()
                {
                    f.query.pop();
                    let q = f.query.clone();
                    self.set_filter_query(&q);
                }
            }

            SchemaFilterToggle => match (&self.schema_filter, self.schema_filter_input_open) {
                (None, _) => self.set_schema_filter(""),
                (Some(_), false) => self.schema_filter_input_open = true,
                (Some(_), true) => self.clear_schema_filter(),
            },
            SchemaFilterAccept => {
                let empty = self.schema_filter.as_ref().is_none_or(|q| q.is_empty());
                if empty {
                    self.clear_schema_filter();
                } else {
                    self.schema_filter_input_open = false;
                }
            }
            SchemaFilterCancel => self.clear_schema_filter(),
            SchemaFilterBackspace => {
                if self.schema_filter_input_open
                    && let Some(q) = self.schema_filter.as_mut()
                {
                    q.pop();
                    let s = q.clone();
                    self.set_schema_filter(&s);
                }
            }

            ConnectSelected => self.connect_selected(),
            NewConnection => self.form = Some(FormState::new()),
            DeleteConnection => self.confirm_delete_selected(),
            EditConnection => self.edit_selected(),
            EditorNewline => {
                self.exit_history_browse();
                self.editor.newline();
            }
            EditorBackspace => {
                self.exit_history_browse();
                self.editor.backspace();
                self.refresh_autocomplete();
            }
            EditorLeft => {
                self.editor.left();
                self.refresh_autocomplete();
            }
            EditorRight => {
                self.editor.right();
                self.refresh_autocomplete();
            }
            EditorUp => self.editor.up(),
            EditorDown => self.editor.down(),
            EditorHome => self.editor.home(),
            EditorEnd => self.editor.end(),
            RecallHistoryOlder => self.recall_history(true),
            RecallHistoryNewer => self.recall_history(false),
            SaveBuffer => self.start_save_buffer(),
            FormatSql => self.format_sql(),
            ExplainQuery => self.explain_query(),
            AdjustEditorHeightUp => self.adjust_editor_height(1),
            AdjustEditorHeightDown => self.adjust_editor_height(-1),
            BeginTx => self.execute_tx_sql("BEGIN"),
            CommitTx => self.execute_tx_sql("COMMIT"),
            RollbackTx => self.execute_tx_sql("ROLLBACK"),
            ToggleAutocommit => self.toggle_autocommit(),
            SaveBufferAccept => self.confirm_save_buffer(),
            SaveBufferCancel => self.cancel_save_buffer(),

            AcceptCompletion => self.accept_completion(),
            CompletionNext => self.move_completion(1),
            CompletionPrev => self.move_completion(-1),
            DismissCompletion => self.autocomplete = None,

            CopyRowJson => self.copy_row_json(),
            CopyResultCsv => self.copy_result_csv(),

            SchemaExpand => self.schema_expand_or_run(),
            SchemaCollapse => self.schema_collapse_at_cursor(),

            FormSave => {
                let on_type = self
                    .form
                    .as_ref()
                    .is_some_and(|f| f.active == 0 && f.kind_picker.is_none());
                if on_type {
                    if let Some(form) = &mut self.form {
                        form.kind_picker = Some(KindPickerState::new());
                    }
                } else {
                    self.save_form();
                }
            }
            FormCancel => self.form = None,
            FormTestConnection => self.test_connection(),
            FormFieldNext => self.form_field_next(1),
            FormFieldPrev => self.form_field_next(-1),
            FormFieldDown => self.form_field_down(),
            FormFieldUp => self.form_field_up(),
            FormFieldLeft => self.form_field_left(),
            FormFieldRight => self.form_field_right(),
            FormFieldHome => self.form_field_home(),
            FormFieldEnd => self.form_field_end(),
            FormFieldBackspace => self.form_field_backspace(),
            FormKindPickerToggle => self.form_kind_picker_toggle(),
            FormKindPickerSelect => self.form_kind_picker_select(),
            FormKindPickerNext => self.form_kind_picker_next(),
            FormKindPickerPrev => self.form_kind_picker_prev(),
            FormKindPickerBackspace => self.form_kind_picker_backspace(),
            FormKindPickerClose => self.form_kind_picker_close(),

            FeaturesClose => self.features_open = false,
            FeaturesNext => self.features_move(1),
            FeaturesPrev => self.features_move(-1),
            FeaturesToggle => self.features_toggle(),

            ConfirmYes => {
                if let Some(sql) = self.confirm_destructive.take() {
                    self.execute_sql(sql);
                } else if let Some(idx) = self.confirm_delete.take() {
                    self.delete_connection_at(idx);
                }
            }
            ConfirmNo => {
                if self.confirm_destructive.is_some() {
                    self.confirm_destructive = None;
                    self.status = "Query cancelled.".into();
                } else if self.confirm_delete.is_some() {
                    self.confirm_delete = None;
                    self.status = "Deletion cancelled.".into();
                }
            }

            EditCell => self.start_edit_cell(),
            EditCellConfirm => self.confirm_edit_cell(),
            EditCellCancel => self.cancel_edit_cell(),
            EditCellLeft => {
                if let Some(edit) = &mut self.edit_cell {
                    edit.cursor = edit.cursor.saturating_sub(1);
                }
            }
            EditCellRight => {
                if let Some(edit) = &mut self.edit_cell {
                    edit.cursor = (edit.cursor + 1).min(edit.raw_value.len());
                }
            }
            EditCellHome => {
                if let Some(edit) = &mut self.edit_cell {
                    edit.cursor = 0;
                }
            }
            EditCellEnd => {
                if let Some(edit) = &mut self.edit_cell {
                    edit.cursor = edit.raw_value.len();
                }
            }
            EditCellBackspace => {
                if let Some(edit) = &mut self.edit_cell
                    && edit.cursor > 0
                {
                    edit.cursor -= 1;
                    edit.raw_value.remove(edit.cursor);
                }
            }
            CycleResultNext => self.cycle_result(1),
            CycleResultPrev => self.cycle_result(-1),
            SortColumn => self.sort_column(),
            InspectCell => self.inspect_cell(),
            // export
            ExportResult => self.start_export(),
            ExportAccept => self.confirm_export(),
            ExportCancel => self.cancel_export(),
            ExportFormatNext => self.export_cycle_format(1),
            ExportFormatPrev => self.export_cycle_format(-1),
            ExportCursorLeft => self.export_cursor_left(),
            ExportCursorRight => self.export_cursor_right(),
            ExportBackspace => self.export_backspace(),
            // row insert
            RowInsert => self.start_row_insert(),
            RowInsertAccept => self.confirm_row_insert(),
            RowInsertCancel => self.cancel_row_insert(),
            RowInsertFieldLeft => self.row_insert_cursor_left(),
            RowInsertFieldRight => self.row_insert_cursor_right(),
            RowInsertBackspace => self.row_insert_backspace(),
            RowInsertFieldNext => self.row_insert_field_next(1),
            RowInsertFieldPrev => self.row_insert_field_next(-1),
            // row delete
            RowDelete => self.start_row_delete(),
        }
    }

    fn handle_text_input(&mut self, view: View, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if let KeyCode::Char(c) = key.code {
            if ctrl {
                return;
            }
            match view {
                View::Editor | View::EditorAutocomplete => {
                    self.exit_history_browse();
                    self.editor.insert_char(c);
                    self.refresh_autocomplete();
                }
                View::KindPicker => {
                    if let Some(form) = &mut self.form
                        && let Some(picker) = &mut form.kind_picker
                    {
                        let mut q = picker.query.clone();
                        q.push(c);
                        picker.set_query(q);
                    }
                }
                View::ResultsFilter => {
                    let q = match &self.result_filter {
                        Some(f) => format!("{}{c}", f.query),
                        None => c.to_string(),
                    };
                    self.set_filter_query(&q);
                }
                View::SchemaFilter => {
                    let q = match &self.schema_filter {
                        Some(s) => format!("{s}{c}"),
                        None => c.to_string(),
                    };
                    self.set_schema_filter(&q);
                }
                View::Form => {
                    if let Some(f) = self.form.as_mut() {
                        if f.active == 0 {
                            if f.kind_picker.is_none() {
                                f.kind_picker = Some(KindPickerState::new());
                            }
                            if let Some(p) = &mut f.kind_picker {
                                let mut q = p.query.clone();
                                q.push(c);
                                p.set_query(q);
                            }
                        } else {
                            f.fields[f.active - 1].insert(f.cursor, c);
                            f.cursor += 1;
                        }
                    }
                }
                View::ResultsEdit => {
                    if let Some(edit) = &mut self.edit_cell {
                        edit.raw_value.insert(edit.cursor, c);
                        edit.cursor += 1;
                    }
                }
                View::ResultsExport => {
                    if let Some(export) = &mut self.export_input {
                        export.path.insert(export.cursor, c);
                        export.cursor += 1;
                    }
                }
                View::ResultsRowInsert => {
                    if let Some(ins) = &mut self.row_insert {
                        ins.values[ins.cursor_col].insert(ins.cursor_char, c);
                        ins.cursor_char += 1;
                    }
                }
                View::EditorSave => {
                    if let Some(input) = &mut self.editor_save_input {
                        input.path.insert(input.cursor, c);
                        input.cursor += 1;
                    }
                }
                _ => {}
            }
        }
    }

    fn displayed_count(&self) -> usize {
        match &self.output {
            Output::Table { rows, .. } => match &self.result_filter {
                Some(f) => f.matched.len(),
                None => rows.len(),
            },
            _ => 0,
        }
    }

    fn result_dims(&self) -> (usize, usize) {
        (self.displayed_count(), self.results_body_h.max(1))
    }

    fn scroll_cursor_row_into_view(&mut self) {
        let Some(cur) = self.result_cursor_row else {
            return;
        };
        let vh = self.results_body_h.max(1);
        if cur < self.result_scroll_row {
            self.result_scroll_row = cur;
        } else if cur >= self.result_scroll_row + vh {
            self.result_scroll_row = cur.saturating_sub(vh - 1);
        }
    }

    fn scroll_cursor_col_into_view(&mut self) {
        let vc = self.results_visible_cols.max(1);
        if self.result_cursor_col < self.result_scroll_col {
            self.result_scroll_col = self.result_cursor_col;
        } else if self.result_cursor_col >= self.result_scroll_col + vc {
            self.result_scroll_col = self.result_cursor_col.saturating_sub(vc - 1);
        }
    }

    fn selected_abs_row(&self) -> Option<usize> {
        let cur = self.result_cursor_row?;
        match &self.result_filter {
            Some(f) => f.matched.get(cur).copied(),
            None => Some(cur),
        }
    }

    fn set_filter_query(&mut self, query: &str) {
        let pairs = match &self.output {
            Output::Table { rows, .. } => crate::filter::fuzzy_filter_indices(query, rows),
            _ => Vec::new(),
        };
        let matched: Vec<usize> = pairs.iter().map(|(i, _)| *i).collect();
        let offsets: HashMap<usize, CellMatches> = pairs.into_iter().collect();
        self.result_filter = Some(ResultFilter {
            query: query.to_string(),
            matched,
            offsets,
        });
        self.filter_input_open = true;
        self.result_cursor_row = None;
        self.result_scroll_row = 0;
    }

    fn cycle_result(&mut self, dir: isize) {
        let n = self.all_results.len();
        if n <= 1 {
            return;
        }
        let new = ((self.active_result as isize + dir).rem_euclid(n as isize)) as usize;
        if let Some(r) = self.all_results.get(new) {
            self.active_result = new;
            self.output = Output::Table {
                columns: r.columns.clone(),
                rows: r.rows.clone(),
                rows_affected: r.rows_affected,
                elapsed_ms: 0,
                truncated: r.truncated,
            };
            self.status = format!(
                "Result set {}/{}  ·  {} rows  ·  {} affected",
                new + 1,
                n,
                r.rows.len(),
                r.rows_affected,
            );
            self.result_cursor_row = None;
            self.result_scroll_row = 0;
            self.result_cursor_col = 0;
            self.result_scroll_col = 0;
            self.result_filter = None;
            self.filter_input_open = false;
        }
    }

    fn set_schema_filter(&mut self, query: &str) {
        self.schema_filter = Some(query.to_string());
        self.schema_filter_input_open = true;
    }

    fn clear_schema_filter(&mut self) {
        self.schema_filter = None;
        self.schema_filter_input_open = false;
    }

    fn clear_filter(&mut self) {
        self.result_filter = None;
        self.filter_input_open = false;
        self.result_cursor_row = None;
        self.result_scroll_row = 0;
    }

    fn copy_row_json(&mut self) {
        let abs = match self.selected_abs_row() {
            Some(i) => i,
            None => {
                self.status = "No row selected — press j/k or click to select.".into();
                return;
            }
        };
        let json = match &self.output {
            Output::Table { columns, rows, .. } => match rows.get(abs) {
                Some(row) => row_to_json(columns, row),
                None => return,
            },
            _ => return,
        };
        self.status = match copy_to_clipboard(&json) {
            Ok(()) => format!("Copied row {} as JSON.", abs + 1),
            Err(e) => format!("Copy failed: {e}"),
        };
    }

    fn copy_result_csv(&mut self) {
        let (csv, n) = match &self.output {
            Output::Table { columns, rows, .. } => (result_to_csv(columns, rows), rows.len()),
            _ => return,
        };
        self.status = match copy_to_clipboard(&csv) {
            Ok(()) => format!("Copied {} rows as CSV ({} bytes).", n, csv.len()),
            Err(e) => format!("Copy failed: {e}"),
        };
    }

    fn schema_expand_or_run(&mut self) {
        let kind = self.db.as_ref().map(|d| d.kind()).unwrap_or("mysql");
        match self.schema_entry_at_cursor() {
            Some(SchemaEntry::Table(t)) => {
                self.schema_expanded.insert(t);
            }
            Some(SchemaEntry::View(v)) => {
                let sql = schema_query(&v, SchemaOpt::Rows, kind);
                self.editor = Editor::from_text(sql);
                self.focus = Focus::Results;
                self.run_query();
            }
            Some(SchemaEntry::Leaf { table, opt }) => {
                let sql = schema_query(&table, opt, kind);
                self.editor = Editor::from_text(sql);
                self.focus = Focus::Results;
                self.run_query();
            }
            None => {}
        }
    }

    fn schema_collapse_at_cursor(&mut self) {
        if let Some(t) = self.schema_table_at_cursor() {
            self.schema_expanded.remove(&t);
            self.schema_cursor_to_table(&t);
        }
    }

    fn form_field_next(&mut self, dir: i32) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        // active: 0 = Type row, 1-6 = Name..Database (fields[0..5]).
        let last = form.fields.len();
        form.active = if dir > 0 {
            if form.active >= last {
                0
            } else {
                form.active + 1
            }
        } else if form.active == 0 {
            last
        } else {
            form.active - 1
        };
        form.cursor = if form.active == 0 {
            0
        } else {
            form.fields[form.active - 1].len()
        };
    }

    fn form_field_left(&mut self) {
        if let Some(f) = self.form.as_mut()
            && f.active > 0
        {
            f.cursor = f.cursor.saturating_sub(1);
        }
    }
    fn form_field_right(&mut self) {
        if let Some(f) = self.form.as_mut()
            && f.active > 0
        {
            f.cursor = (f.cursor + 1).min(f.fields[f.active - 1].len());
        }
    }
    fn form_field_home(&mut self) {
        if let Some(f) = self.form.as_mut()
            && f.active > 0
        {
            f.cursor = 0;
        }
    }
    fn form_field_end(&mut self) {
        if let Some(f) = self.form.as_mut()
            && f.active > 0
        {
            f.cursor = f.fields[f.active - 1].len();
        }
    }
    fn form_field_backspace(&mut self) {
        if let Some(f) = self.form.as_mut()
            && f.active > 0
            && f.cursor > 0
        {
            f.cursor -= 1;
            f.fields[f.active - 1].remove(f.cursor);
        }
    }

    fn form_field_down(&mut self) {
        if let Some(form) = &mut self.form
            && form.active == 0
            && form.kind_picker.is_none()
        {
            form.kind_picker = Some(KindPickerState::new());
            return;
        }
        self.form_field_next(1);
    }

    fn form_field_up(&mut self) {
        if let Some(form) = &mut self.form
            && form.active == 0
            && form.kind_picker.is_none()
        {
            form.kind_picker = Some(KindPickerState::new());
            return;
        }
        self.form_field_next(-1);
    }

    fn form_kind_picker_toggle(&mut self) {
        if let Some(form) = &mut self.form {
            if form.kind_picker.is_some() {
                form.kind_picker = None;
            } else {
                form.kind_picker = Some(KindPickerState::new());
            }
        }
    }

    fn form_kind_picker_select(&mut self) {
        let Some(form) = &mut self.form else { return };
        let Some(picker) = form.kind_picker.take() else {
            return;
        };
        let Some(kind) = picker.selected_kind() else {
            return;
        };
        if form.fields[2] == FormState::default_port(&form.kind).to_string() {
            form.fields[2] = FormState::default_port(kind).to_string();
        }
        form.kind = kind.into();
        form.active = 1;
        form.cursor = form.fields[0].len();
    }

    fn form_kind_picker_next(&mut self) {
        if let Some(form) = &mut self.form
            && let Some(picker) = &mut form.kind_picker
        {
            let n = picker.filtered.len();
            if n > 0 {
                picker.cursor = (picker.cursor + 1).min(n - 1);
            }
        }
    }

    fn form_kind_picker_prev(&mut self) {
        if let Some(form) = &mut self.form
            && let Some(picker) = &mut form.kind_picker
        {
            picker.cursor = picker.cursor.saturating_sub(1);
        }
    }

    fn form_kind_picker_backspace(&mut self) {
        if let Some(form) = &mut self.form
            && let Some(picker) = &mut form.kind_picker
        {
            let mut q = picker.query.clone();
            q.pop();
            picker.set_query(q);
        }
    }

    fn form_kind_picker_close(&mut self) {
        if let Some(form) = &mut self.form {
            form.kind_picker = None;
        }
    }

    fn features_move(&mut self, dir: i32) {
        let last = Features::LIST.len().saturating_sub(1);
        self.feature_cursor = if dir > 0 {
            if self.feature_cursor >= last {
                0
            } else {
                self.feature_cursor + 1
            }
        } else if self.feature_cursor == 0 {
            last
        } else {
            self.feature_cursor - 1
        };
    }

    fn features_toggle(&mut self) {
        let i = self.feature_cursor;
        let v = !self.config.features.get(i);
        self.config.features.set(i, v);
        let name = Features::LIST[i].0;
        self.status = match self.config.save() {
            Ok(()) => format!("{name}: {}", if v { "on" } else { "off" }),
            Err(e) => format!("Toggle failed: {e}"),
        };
    }

    fn start_edit_cell(&mut self) {
        let Some(db) = self.db.as_ref() else {
            self.status = "Not connected.".into();
            return;
        };
        let abs_row = match self.selected_abs_row() {
            Some(r) => r,
            None => {
                self.status = "No row selected — press j/k or click to select.".into();
                return;
            }
        };
        let Output::Table { columns, rows, .. } = &self.output else {
            self.status = "No result table to edit.".into();
            return;
        };
        let cell_value = match rows
            .get(abs_row)
            .and_then(|r| r.get(self.result_cursor_col))
        {
            Some(v) => v.clone(),
            None => {
                self.status = "Invalid cell.".into();
                return;
            }
        };
        let col_idx = self.result_cursor_col.min(columns.len().saturating_sub(1));
        let col_name = columns[col_idx].clone();
        let table = match extract_table_name(&self.editor.text()) {
            Some(t) => t,
            None => {
                self.status = "Cannot edit: no table name found in the query.".into();
                return;
            }
        };
        self.edit_cell_pending = Some(EditCellPending {
            abs_row,
            col: self.result_cursor_col,
            cell_value: cell_value.clone(),
            col_name,
            table,
            columns_len: columns.len(),
            rows_len: rows.len(),
        });
        let db = db.boxed_clone();
        let table = self.edit_cell_pending.as_ref().unwrap().table.clone();
        self.rx = Some(spawn_job(Job::PrimaryKeys(db, table)));
        self.running_query = true;
        self.status = "Looking up primary key…".into();
    }

    fn confirm_edit_cell(&mut self) {
        let edit = match self.edit_cell.take() {
            Some(e) => e,
            None => return,
        };
        if edit.raw_value == edit.original_value {
            self.status = "No change.".into();
            return;
        }
        let Some(db) = self.db.as_ref() else {
            self.status = "Not connected.".into();
            return;
        };
        let sql = build_update_sql(
            &edit.table,
            &edit.col_name,
            &edit.raw_value,
            &edit.pk_cols,
            &edit.pk_vals,
            db.kind(),
        );
        let db = db.boxed_clone();
        self.pending_cell_update = Some((edit.abs_row, edit.col, edit.raw_value));
        self.rx = Some(spawn_job(Job::UpdateCell(db, sql, self.exec_ctx(false))));
        self.running_query = true;
        self.status = "Updating cell…".into();
    }

    fn cancel_edit_cell(&mut self) {
        self.edit_cell = None;
        self.status = "Edit cancelled.".into();
    }

    fn edit_selected(&mut self) {
        if let Some(conn) = self.config.connections.get(self.conn_cursor).cloned() {
            self.form = Some(FormState::from_connection(self.conn_cursor, &conn));
            self.status = "Editing connection — Enter to save, Esc to cancel.".into();
        }
    }

    fn test_connection(&mut self) {
        let Some(form) = self.form.as_ref() else {
            return;
        };
        let kind = form.kind.clone();
        let port: u16 = form.fields[2]
            .parse()
            .unwrap_or(FormState::default_port(&kind));
        let fields = form.fields.clone();
        let conn = Connection {
            name: fields[0].clone(),
            kind,
            host: fields[1].clone(),
            port,
            username: fields[3].clone(),
            password: fields[4].clone(),
            database: fields[5].clone(),
            ssl: false,
            use_keychain: false,
            ssh_enabled: false,
            ssh_host: String::new(),
            ssh_port: 22,
            ssh_user: String::new(),
            ssh_keyfile: String::new(),
            query_timeout_secs: None,
        };
        // ponytail: reuse the Ping job — open + SELECT 1. Best-effort: a fresh
        // pool is built just to test; cheap for a one-shot TUI ping.
        match db::open(&conn, self.config.query_timeout()) {
            Ok(db) => {
                self.testing_connection = true;
                self.running_query = true;
                self.rx = Some(spawn_job(Job::Ping(db, conn.name)));
                self.status = "Testing connection…".into();
            }
            Err(e) => self.status = format!("Open failed: {e}"),
        }
    }

    fn cancel_query(&mut self) {
        let id = self.cancel.conn_id();
        if id == 0 {
            self.status = "Nothing running to cancel.".into();
            return;
        }
        let Some(db) = self.db.as_ref() else {
            self.status = "Not connected.".into();
            return;
        };
        let db = db.boxed_clone();
        // ponytail: detached side-conn sends KILL QUERY; best-effort (needs
        // PROCESS/SUPER). The worker's query errors out → apply_job shows
        // "Query cancelled." once the result arrives.
        std::thread::spawn(move || {
            let _ = db.kill_query(id);
        });
        self.cancel_requested = true;
        self.status = "Cancelling query…".into();
    }

    /// ponytail: one helper builds the per-exec context from config so every
    /// execute_script call shares cancel + row-cap wiring.
    fn exec_ctx(&self, readable_binary: bool) -> ExecCtx {
        ExecCtx {
            cancel: self.cancel.clone(),
            readable_binary,
            limit: self.config.select_limit,
        }
    }
    fn recall_history(&mut self, older: bool) {
        if self.history.is_empty() {
            return;
        }
        let n = self.history.len();
        if self.history_cursor.is_none() {
            if older {
                self.history_draft = Some(self.editor.text());
                self.history_cursor = Some(n - 1);
                self.editor = Editor::from_text(self.history[n - 1].clone());
            }
            return;
        }
        let Some(i) = self.history_cursor else {
            return;
        };
        if older {
            if i == 0 {
                return;
            }
            self.history_cursor = Some(i - 1);
        } else if i + 1 < n {
            self.history_cursor = Some(i + 1);
        } else {
            self.history_cursor = None;
            if let Some(draft) = self.history_draft.take() {
                self.editor = Editor::from_text(draft);
            }
            return;
        }
        let text = self.history[self.history_cursor.unwrap()].clone();
        self.editor = Editor::from_text(text);
    }

    fn exit_history_browse(&mut self) {
        self.history_cursor = None;
        self.history_draft = None;
    }

    fn refresh_autocomplete(&mut self) {
        let (word, word_start) = self.current_word();
        let line = &self.editor.lines[self.editor.row];
        let col = self.editor.col.min(line.len());
        let dot = word_start > 0 && line.as_bytes()[word_start - 1] == b'.';
        if !dot && word.len() < 2 {
            self.autocomplete = None;
            return;
        }
        let (tables, columns) = self.completion_pools(dot, word_start, &line[..col]);
        let items = autocomplete::completions(&word, &tables, &columns);
        if items.is_empty() {
            self.autocomplete = None;
            return;
        }
        let trigger_len = word.len();
        match &mut self.autocomplete {
            Some(ac) => {
                ac.items = items;
                if ac.cursor >= ac.items.len() {
                    ac.cursor = 0;
                }
                ac.trigger_len = trigger_len;
            }
            None => {
                self.autocomplete = Some(Autocomplete {
                    items,
                    cursor: 0,
                    trigger_len,
                })
            }
        }
    }

    fn completion_pools(
        &self,
        dot: bool,
        word_start: usize,
        line_up_to_col: &str,
    ) -> (Vec<String>, Vec<String>) {
        if self.schema.is_empty() {
            return (Vec::new(), Vec::new());
        }
        if dot {
            let table = ident_before(line_up_to_col, word_start - 1);
            if let Some(cols) = self.schema.get(&table) {
                return (Vec::new(), cols.clone());
            }
            return (Vec::new(), Vec::new());
        }
        let tables: Vec<String> = self.schema.keys().cloned().collect();
        let referenced = autocomplete::referenced_tables(&self.current_statement());
        let mut columns: Vec<String> = Vec::new();
        let mut seen = HashSet::new();
        for t in referenced {
            if let Some(cols) = self.schema.get(&t) {
                for c in cols {
                    if seen.insert(c.clone()) {
                        columns.push(c.clone());
                    }
                }
            }
        }
        (tables, columns)
    }

    fn current_statement(&self) -> String {
        let mut off = 0usize;
        for (i, l) in self.editor.lines.iter().enumerate() {
            if i == self.editor.row {
                off += self.editor.col.min(l.len());
                break;
            }
            off += l.len() + 1;
        }
        let text = self.editor.text();
        let upto = off.min(text.len());
        let start = text[..upto].rfind(';').map(|p| p + 1).unwrap_or(0);
        text[start..upto].to_string()
    }

    fn current_word(&self) -> (String, usize) {
        let line = &self.editor.lines[self.editor.row];
        let col = self.editor.col.min(line.len());
        let mut start = 0;
        for (i, ch) in line.char_indices() {
            if i >= col {
                break;
            }
            if !(ch.is_alphanumeric() || ch == '_') {
                start = i + ch.len_utf8();
            }
        }
        (line[start..col].to_string(), start)
    }

    fn accept_completion(&mut self) {
        let Some(ac) = self.autocomplete.take() else {
            return;
        };
        if ac.items.is_empty() {
            return;
        }
        let cand = ac.items[ac.cursor % ac.items.len()].clone();
        let line = &mut self.editor.lines[self.editor.row];
        let start = self.editor.col.saturating_sub(ac.trigger_len);
        line.replace_range(start..self.editor.col, &cand);
        self.editor.col = start + cand.len();
        self.autocomplete = None;
    }

    fn move_completion(&mut self, dir: i32) {
        if let Some(ac) = &mut self.autocomplete {
            if ac.items.is_empty() {
                return;
            }
            let n = ac.items.len() as i32;
            ac.cursor = ((ac.cursor as i32 + dir).rem_euclid(n)) as usize;
        }
    }

    pub fn editor_time_label(&self) -> Option<String> {
        if let Some(start) = self.query_start {
            return Some(format!("{:.1}s", start.elapsed().as_secs_f64()));
        }
        match &self.output {
            Output::Table { elapsed_ms, .. } if *elapsed_ms > 0 => {
                if *elapsed_ms < 1000 {
                    Some(format!("{} ms", elapsed_ms))
                } else {
                    Some(format!("{:.1}s", *elapsed_ms as f64 / 1000.0))
                }
            }
            _ => None,
        }
    }

    /// Rendered rows for the schema tree. When `schema_filter` is Some,
    /// only entries whose table/view name contains the query (case-insensitive)
    /// are included.
    pub fn schema_rows(&self) -> Vec<SchemaEntry> {
        let q = self
            .schema_filter
            .as_deref()
            .filter(|q| !q.is_empty())
            .map(|q| q.to_lowercase());
        let matches_filter = |name: &str| {
            q.as_ref()
                .map(|q| name.to_lowercase().contains(q.as_str()))
                .unwrap_or(true)
        };
        let mut rows: Vec<SchemaEntry> = Vec::new();
        let mut tables: Vec<&String> = self.schema.keys().collect();
        tables.sort();
        for t in tables {
            if !matches_filter(t) {
                continue;
            }
            rows.push(SchemaEntry::Table(t.clone()));
            if self.schema_expanded.contains(t) {
                for opt in [
                    SchemaOpt::Rows,
                    SchemaOpt::Columns,
                    SchemaOpt::Constraints,
                    SchemaOpt::Indexes,
                ] {
                    rows.push(SchemaEntry::Leaf {
                        table: t.clone(),
                        opt,
                    });
                }
            }
        }
        for v in &self.schema_views {
            if matches_filter(v) {
                rows.push(SchemaEntry::View(v.clone()));
            }
        }
        rows
    }

    fn schema_entry_at_cursor(&self) -> Option<SchemaEntry> {
        self.schema_rows().get(self.schema_cursor).cloned()
    }

    fn schema_table_at_cursor(&self) -> Option<String> {
        match self.schema_entry_at_cursor() {
            Some(SchemaEntry::Table(t)) => Some(t),
            Some(SchemaEntry::View(v)) => Some(v),
            Some(SchemaEntry::Leaf { table, .. }) => Some(table),
            None => None,
        }
    }

    fn schema_cursor_to_table(&mut self, t: &str) {
        if let Some(i) = self
            .schema_rows()
            .iter()
            .position(|e| matches!(e, SchemaEntry::Table(name) if name == t))
        {
            self.schema_cursor = i;
        }
    }

    pub fn handle_mouse(&mut self, m: MouseEvent) {
        if self.form.is_some() || self.features_open {
            return;
        }
        // ponytail: cell inspect scroll
        if let Some(inspect) = &mut self.cell_inspect {
            let lines: Vec<&str> = inspect.cell_value.lines().collect();
            match m.kind {
                MouseEventKind::ScrollDown => {
                    inspect.scroll = inspect
                        .scroll
                        .saturating_add(1)
                        .min(lines.len().saturating_sub(1));
                }
                MouseEventKind::ScrollUp => {
                    inspect.scroll = inspect.scroll.saturating_sub(1);
                }
                _ => {}
            }
            return;
        }
        let Some(rect) = self.results_rect else {
            return;
        };
        if !(m.column >= rect.x
            && m.column < rect.right()
            && m.row >= rect.y
            && m.row < rect.bottom())
        {
            return;
        }
        let (nrows, ncol) = match &self.output {
            Output::Table { columns, rows, .. } => (rows.len(), columns.len()),
            _ => return,
        };
        let vh = self.results_body_h.max(1);
        let vc = self.results_visible_cols.max(1);
        let max_scroll_row = nrows.saturating_sub(vh);
        let max_scroll_col = ncol.saturating_sub(vc);
        let last_row = nrows.saturating_sub(1);
        let last_col = ncol.saturating_sub(1);
        match m.kind {
            MouseEventKind::ScrollDown => {
                self.result_scroll_row =
                    self.result_scroll_row.saturating_add(1).min(max_scroll_row);
            }
            MouseEventKind::ScrollUp => {
                self.result_scroll_row = self.result_scroll_row.saturating_sub(1);
            }
            MouseEventKind::ScrollRight => {
                self.result_scroll_col =
                    self.result_scroll_col.saturating_add(1).min(max_scroll_col);
            }
            MouseEventKind::ScrollLeft => {
                self.result_scroll_col = self.result_scroll_col.saturating_sub(1);
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if self.result_filter.is_some() {
                    self.clear_filter();
                }
                let (row, col) = match &self.results_click_geom {
                    Some(geom) => click_to_cell(geom, self.result_scroll_row, m.column, m.row),
                    None => (None, None),
                };
                let mut moved = false;
                if let Some(r) = row {
                    self.result_cursor_row = Some(r.min(last_row));
                    self.scroll_cursor_row_into_view();
                    moved = true;
                }
                if let Some(c) = col {
                    self.result_cursor_col = c.min(last_col);
                    self.scroll_cursor_col_into_view();
                    moved = true;
                }
                if moved {
                    self.focus = Focus::Results;
                }
            }
            _ => {}
        }
    }

    // ── Sort ─────────────────────────────────────────────────────────
    fn sort_column(&mut self) {
        let ncol = match &self.output {
            Output::Table { columns, .. } => columns.len(),
            _ => return,
        };
        let col = self.result_cursor_col.min(ncol.saturating_sub(1));
        self.sort = match self.sort {
            Some((c, SortDir::Asc)) if c == col => Some((col, SortDir::Desc)),
            Some((c, _)) if c == col => None,
            _ => Some((col, SortDir::Asc)),
        };
        if let (Output::Table { rows, .. }, Some((c, dir))) = (&mut self.output, self.sort) {
            rows.sort_by(|a, b| {
                let va = a.get(c).map(String::as_str).unwrap_or("");
                let vb = b.get(c).map(String::as_str).unwrap_or("");
                if dir == SortDir::Asc {
                    va.cmp(vb)
                } else {
                    vb.cmp(va)
                }
            });
        }
        self.status = match self.sort {
            Some((c, SortDir::Asc)) => format!("Sorted asc by column {}.", c + 1),
            Some((_, SortDir::Desc)) => format!("Sorted desc by column {}.", col + 1),
            None => "Sort cleared.".into(),
        };
    }

    // ── Cell inspect ─────────────────────────────────────────────────
    fn inspect_cell(&mut self) {
        let abs = match self.selected_abs_row() {
            Some(r) => r,
            None => {
                self.status = "No row selected.".into();
                return;
            }
        };
        let col = self.result_cursor_col;
        let (col_name, cell_value) = match &self.output {
            Output::Table { columns, rows, .. } => {
                let name = columns.get(col).cloned().unwrap_or_default();
                let val = rows
                    .get(abs)
                    .and_then(|r| r.get(col))
                    .cloned()
                    .unwrap_or_default();
                (name, val)
            }
            _ => {
                self.status = "No result table.".into();
                return;
            }
        };
        self.cell_inspect = Some(CellInspect {
            col_name,
            cell_value,
            abs_row: abs,
            col,
            scroll: 0,
        });
        self.status = "Inspecting cell — Esc to close.".into();
    }

    // ── Export ────────────────────────────────────────────────────────
    fn start_export(&mut self) {
        let has_cols = matches!(&self.output, Output::Table { columns, .. } if !columns.is_empty());
        if !has_cols {
            self.status = "No results to export.".into();
            return;
        }
        self.export_input = Some(ExportInput {
            path: "export.csv".into(),
            format: ExportFormat::Csv,
            cursor: "export.csv".len(),
        });
        self.status = "Enter path, Tab for format, Enter to export, Esc to cancel.".into();
    }

    fn confirm_export(&mut self) {
        let Some(export) = self.export_input.take() else {
            return;
        };
        if export.path.trim().is_empty() {
            self.status = "Path is empty.".into();
            return;
        }
        let path = std::path::Path::new(&export.path);
        // Respect active result filter: export only matching rows.
        let (columns, all_rows) = match &self.output {
            Output::Table { columns, rows, .. } => (columns, rows),
            _ => return,
        };
        let filtered_rows: Vec<Vec<String>>;
        let export_rows: &[Vec<String>] = if let Some(f) = &self.result_filter {
            filtered_rows = f
                .matched
                .iter()
                .filter_map(|&i| all_rows.get(i).cloned())
                .collect();
            &filtered_rows
        } else {
            all_rows
        };
        let content = match export.format {
            ExportFormat::Csv => result_to_csv(columns, export_rows),
            ExportFormat::Json => {
                let objs: Vec<String> = export_rows
                    .iter()
                    .map(|r| row_to_json(columns, r))
                    .collect();
                format!("[\n{}\n]", objs.join(",\n"))
            }
            ExportFormat::JsonLines => export_rows
                .iter()
                .map(|r| row_to_json(columns, r))
                .collect::<Vec<_>>()
                .join("\n"),
            ExportFormat::SqlInsert => {
                let table =
                    extract_table_name(&self.editor.text()).unwrap_or_else(|| "results".into());
                result_to_sql_insert(&table, columns, export_rows)
            }
        };
        let n = export_rows.len();
        match std::fs::write(path, &content) {
            Ok(()) => {
                self.status = format!("Exported {n} rows to {}.", export.path);
            }
            Err(e) => {
                self.status = format!("Export failed: {e}");
                // ponytail: restore input on failure so the user can fix the path
                self.export_input = Some(ExportInput {
                    cursor: export.path.len(),
                    ..export
                });
            }
        }
    }

    fn cancel_export(&mut self) {
        self.export_input = None;
        self.status = "Export cancelled.".into();
    }

    fn export_cycle_format(&mut self, dir: isize) {
        let Some(export) = &mut self.export_input else {
            return;
        };
        let n = ExportFormat::LABELS.len();
        let cur = export.format as isize;
        let next = (cur + dir).rem_euclid(n as isize) as usize;
        export.format = match next {
            1 => ExportFormat::Json,
            2 => ExportFormat::JsonLines,
            3 => ExportFormat::SqlInsert,
            _ => ExportFormat::Csv,
        };
        let old_path = std::path::Path::new(&export.path);
        let stem = old_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("export");
        export.path = format!("{}.{}", stem, export.format.extension());
        export.cursor = export.path.len();
    }

    fn export_cursor_left(&mut self) {
        if let Some(export) = &mut self.export_input {
            export.cursor = export.cursor.saturating_sub(1);
        }
    }
    fn export_cursor_right(&mut self) {
        if let Some(export) = &mut self.export_input {
            export.cursor = (export.cursor + 1).min(export.path.len());
        }
    }
    fn export_backspace(&mut self) {
        if let Some(export) = &mut self.export_input
            && export.cursor > 0
        {
            export.cursor -= 1;
            export.path.remove(export.cursor);
        }
    }

    // ── Row insert ────────────────────────────────────────────────────
    fn start_row_insert(&mut self) {
        if self.db.is_none() {
            self.status = "Not connected.".into();
            return;
        };
        let table = match extract_table_name(&self.editor.text()) {
            Some(t) => t,
            None => {
                self.status = "No table name found in the query.".into();
                return;
            }
        };
        let Output::Table { columns, .. } = &self.output else {
            self.status = "No result table.".into();
            return;
        };
        let cols = columns.clone();
        let values = vec![String::new(); cols.len()];
        self.row_insert = Some(RowInsertState {
            table,
            columns: cols,
            values,
            cursor_col: 0,
            cursor_char: 0,
        });
        self.status = "Enter values, Tab to switch columns, Enter to insert, Esc to cancel.".into();
    }

    fn confirm_row_insert(&mut self) {
        let Some(ins) = self.row_insert.take() else {
            return;
        };
        let Some(db) = self.db.as_ref() else {
            self.status = "Not connected.".into();
            return;
        };
        let kind = db.kind();
        let q = ident_quote(kind);
        let cols: String = ins
            .columns
            .iter()
            .map(|c| format!("{q}{c}{q}"))
            .collect::<Vec<_>>()
            .join(", ");
        let vals: String = ins
            .values
            .iter()
            .map(|v| {
                if v == "NULL" {
                    "NULL".into()
                } else {
                    format!("'{}'", sql_escape(v))
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "INSERT INTO {q}{table}{q} ({cols}) VALUES ({vals});",
            q = q,
            table = ins.table,
            cols = cols,
            vals = vals
        );
        let db = db.boxed_clone();
        self.rx = Some(spawn_job(Job::Query(db, sql, self.exec_ctx(false))));
        self.running_query = true;
        self.status = "Inserting row…".into();
    }

    fn cancel_row_insert(&mut self) {
        self.row_insert = None;
        self.status = "Row insert cancelled.".into();
    }

    fn row_insert_cursor_left(&mut self) {
        if let Some(ins) = &mut self.row_insert {
            ins.cursor_char = ins.cursor_char.saturating_sub(1);
        }
    }
    fn row_insert_cursor_right(&mut self) {
        if let Some(ins) = &mut self.row_insert {
            ins.cursor_char = (ins.cursor_char + 1).min(ins.values[ins.cursor_col].len());
        }
    }
    fn row_insert_backspace(&mut self) {
        if let Some(ins) = &mut self.row_insert
            && ins.cursor_char > 0
        {
            ins.cursor_char -= 1;
            ins.values[ins.cursor_col].remove(ins.cursor_char);
        }
    }
    fn row_insert_field_next(&mut self, dir: isize) {
        let Some(ins) = &mut self.row_insert else {
            return;
        };
        let n = ins.columns.len();
        ins.cursor_col = ((ins.cursor_col as isize + dir).rem_euclid(n as isize)) as usize;
        ins.cursor_char = ins.values[ins.cursor_col].len();
    }

    // ── Row delete ────────────────────────────────────────────────────
    // ponytail: naive WHERE = "col1 = 'v1' AND col2 = 'v2'" for ALL columns.
    // Fragile for large / binary cols but covers the 90% case. Upgrade: use
    // primary key lookup (Job::PrimaryKeys) like cell edit.
    fn start_row_delete(&mut self) {
        let Some(db) = self.db.as_ref() else {
            self.status = "Not connected.".into();
            return;
        };
        let abs_row = match self.selected_abs_row() {
            Some(r) => r,
            None => {
                self.status = "No row selected.".into();
                return;
            }
        };
        let Output::Table { columns, rows, .. } = &self.output else {
            self.status = "No result table.".into();
            return;
        };
        let row = match rows.get(abs_row) {
            Some(r) => r,
            None => {
                self.status = "Invalid row.".into();
                return;
            }
        };
        let table = match extract_table_name(&self.editor.text()) {
            Some(t) => t,
            None => {
                self.status = "No table name found in the query.".into();
                return;
            }
        };
        let kind = db.kind();
        let q = ident_quote(kind);
        let where_clause: String = columns
            .iter()
            .zip(row.iter())
            .map(|(c, v)| format!("{q}{c}{q} = '{}'", sql_escape(v)))
            .collect::<Vec<_>>()
            .join(" AND ");
        let sql = format!("DELETE FROM {q}{table}{q} WHERE {where_clause};");
        let db = db.boxed_clone();
        self.rx = Some(spawn_job(Job::Query(db, sql, self.exec_ctx(false))));
        self.running_query = true;
        self.status = "Deleting row…".into();
    }

    // ── Editor save ───────────────────────────────────────────────────
    fn start_save_buffer(&mut self) {
        self.editor_save_input = Some(ExportInput {
            path: "query.sql".into(),
            format: ExportFormat::Csv, // unused for save
            cursor: "query.sql".len(),
        });
        self.status = "Save editor to file — Enter to save, Esc to cancel.".into();
    }

    fn confirm_save_buffer(&mut self) {
        let Some(input) = self.editor_save_input.take() else {
            return;
        };
        if input.path.trim().is_empty() {
            self.status = "Path is empty.".into();
            return;
        }
        match std::fs::write(&input.path, self.editor.text()) {
            Ok(()) => self.status = format!("Saved to {}.", input.path),
            Err(e) => {
                self.status = format!("Save failed: {e}");
                self.editor_save_input = Some(ExportInput {
                    cursor: input.path.len(),
                    ..input
                });
            }
        }
    }

    fn cancel_save_buffer(&mut self) {
        self.editor_save_input = None;
        self.status = "Save cancelled.".into();
    }

    // ── Format SQL ─────────────────────────────────────────────────────
    fn format_sql(&mut self) {
        use sqlformat::{FormatOptions, QueryParams, format as sql_format};
        let text = self.editor.text();
        if text.trim().is_empty() {
            self.status = "Editor is empty.".into();
            return;
        }
        let formatted = sql_format(&text, &QueryParams::None, &FormatOptions::default());
        if formatted != text {
            self.editor = Editor::from_text(formatted);
            self.status = "SQL formatted.".into();
        } else {
            self.status = "SQL already formatted.".into();
        }
    }

    // ── EXPLAIN shortcut ──────────────────────────────────────────────
    fn explain_query(&mut self) {
        let sql = self.editor.text();
        if sql.trim().is_empty() {
            self.status = "Editor is empty.".into();
            return;
        }
        // ponytail: prepend EXPLAIN to the first statement; won't catch multi-stmt.
        // For multi-statement scripts, explain just the first one.
        let first = sql.split(';').next().unwrap_or(&sql).trim();
        let explain_sql = format!("EXPLAIN {first};");
        let head: String = explain_sql.chars().take(120).collect();
        crate::log::info("query", &[("explain_head", &head)]);
        self.execute_sql(explain_sql);
    }

    // ── Resizable editor height ────────────────────────────────────────
    fn adjust_editor_height(&mut self, dir: i32) {
        // ponytail: clamp between 3 and 40 rows; the right-col height is
        // dynamic so we can't enforce a max based on terminal size here.
        let new = (self.editor_height as i32 + dir * 2).clamp(3, 40);
        self.editor_height = new as u16;
        self.status = format!("Editor height: {}", self.editor_height);
    }

    // ── Transaction control ─────────────────────────────────────────
    fn execute_tx_sql(&mut self, sql: &str) {
        let Some(db) = &self.db else {
            self.status = "Not connected.".into();
            return;
        };
        let db = db.boxed_clone();
        let sql = sql.to_string();
        self.status = format!("{sql}…");
        self.rx = Some(spawn_job(Job::Query(db, sql, self.exec_ctx(false))));
        self.running_query = true;
    }

    fn toggle_autocommit(&mut self) {
        self.autocommit = !self.autocommit;
        let state = if self.autocommit { "ON" } else { "OFF" };
        if !self.autocommit {
            // Execute BEGIN to start a manual transaction
            self.execute_tx_sql("BEGIN");
        }
        self.status = format!("Autocommit: {state}");
    }
}

pub fn run(terminal: &mut Term, mut app: App) -> Result<()> {
    while app.running {
        terminal.draw(|f| ui::draw(f, &mut app))?;
        if event::poll(Duration::from_millis(100))? {
            match event::read()? {
                Event::Key(key) => app.handle_key(key),
                Event::Mouse(m) => app.handle_mouse(m),
                _ => {}
            }
        }

        let mut got: Option<JobResult> = None;
        let mut dead = false;
        if let Some(rx) = app.rx.as_ref() {
            match rx.try_recv() {
                Ok(r) => got = Some(r),
                Err(TryRecvError::Disconnected) => dead = true,
                Err(TryRecvError::Empty) => {}
            }
        }
        if dead {
            app.rx = None;
        }
        if let Some(r) = got {
            app.apply_job(r);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
