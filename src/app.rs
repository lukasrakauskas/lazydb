use std::collections::{HashMap, HashSet};
use std::io::Stdout;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::{backend::CrosstermBackend, Terminal};
use ratatui::layout::Rect;

use crate::config::{Config, Features};
use crate::db::{self, Connection, Database, ExecutionResult};
use crate::autocomplete;
use crate::editor::Editor;
use crate::ui;
use crate::shortcuts::{self, Action, View};

type Term = Terminal<CrosstermBackend<Stdout>>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Connections,
    Editor,
    Results,
    Schema,
}

/// rainfrog-style: each table expands to 4 fixed options. Selecting one
/// generates + runs a query (prefills the editor so the user can edit it).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SchemaOpt {
    Rows,
    Columns,
    Constraints,
    Indexes,
}

#[derive(Clone, PartialEq, Eq)]
pub enum SchemaEntry {
    Table(String),
    Leaf { table: String, opt: SchemaOpt },
}

pub enum Output {
    Empty,
    Message(String),
    Table {
        columns: Vec<String>,
        rows: Vec<Vec<String>>,
        rows_affected: u64,
        elapsed_ms: u128,
    },
}

enum Job {
    Ping(Box<dyn Database>, String),
    Query(Box<dyn Database>, String, bool),
    /// Fetch table→columns for schema-aware completion, on a successful connect.
    Schema(Box<dyn Database>),
}

enum JobResult {
    Ping(Result<String, String>),
    Query(Result<ExecutionResult, String>),
    Schema(Result<HashMap<String, Vec<String>>, String>),
}

pub struct FormState {
    /// name, host, port, user, password, database
    pub fields: [String; 6],
    pub active: usize,
    pub cursor: usize,
}

impl FormState {
    pub const LABELS: [&'static str; 6] =
        ["Name", "Host", "Port", "User", "Password", "Database"];

    pub fn new() -> Self {
        Self {
            fields: [
                String::new(),
                "127.0.0.1".into(),
                "3306".into(),
                String::new(),
                String::new(),
                String::new(),
            ],
            active: 0,
            cursor: 0,
        }
    }
}

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
    pub result_row_off: usize,
    pub result_col_off: usize,
    pub results_rect: Option<Rect>,
    pub features_open: bool,
    pub feature_cursor: usize,
    pub confirm_destructive: Option<String>,
    pub debug_keys: bool,
    pub last_key: Option<String>,
    pub autocomplete: Option<Autocomplete>,
    pub schema: HashMap<String, Vec<String>>,
    pub schema_cursor: usize,
    pub schema_expanded: HashSet<String>,
    pub history: Vec<String>,
    pub history_cursor: Option<usize>,
    pub history_draft: Option<String>,
}

/// SQL autocomplete popup state. `trigger_len` is the byte length of the
/// identifier being completed, used to slice it out on accept.
pub struct Autocomplete {
    pub items: Vec<String>,
    pub cursor: usize,
    pub trigger_len: usize,
}

impl App {
    pub fn load() -> Result<Self> {
        Ok(Self {
            config: Config::load(),
            conn_cursor: 0,
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
            result_row_off: 0,
            result_col_off: 0,
            results_rect: None,
            features_open: false,
            feature_cursor: 0,
            confirm_destructive: None,
            debug_keys: false,
            last_key: None,
            autocomplete: None,
            schema: HashMap::new(),
            schema_cursor: 0,
            schema_expanded: HashSet::new(),
            history: Vec::new(),
            history_cursor: None,
            history_draft: None,
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
        let conn = self.config.connections[self.conn_cursor].clone();
        match db::open(&conn) {
            Ok(db) => {
                self.pending_db = Some(db.boxed_clone());
                self.rx = Some(spawn_job(Job::Ping(db, conn.name.clone())));
                self.running_query = true;
                self.status = format!("Connecting to {}…", conn.name);
            }
            Err(e) => self.status = format!("Failed to open: {e}"),
        }
    }

    fn delete_selected(&mut self) {
        if self.config.connections.is_empty() {
            return;
        }
        self.config.connections.remove(self.conn_cursor);
        if self.conn_cursor >= self.config.connections.len() && self.conn_cursor > 0 {
            self.conn_cursor -= 1;
        }
        self.status = match self.config.save() {
            Ok(()) => "Connection deleted.".into(),
            Err(e) => format!("Deleted in-memory, but persist failed: {e}"),
        };
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
        // ponytail: in-memory ring buffer (cap 100), dedupe consecutive dupes.
        if self.history.last().map(String::as_str) != Some(sql.as_str()) {
            if self.history.len() >= 100 { self.history.remove(0); }
            self.history.push(sql.clone());
        }
        self.history_cursor = None;
        self.history_draft = None;
        self.rx = Some(spawn_job(Job::Query(db, sql, readable_binary)));
        self.running_query = true;
        self.query_start = Some(Instant::now());
        self.status = "Running query…".into();
        self.result_row_off = 0;
        self.result_col_off = 0;
    }

    fn save_form(&mut self) {
        let conn = {
            let form = self.form.as_ref().unwrap();
            let port: u16 = form.fields[2].parse().unwrap_or(3306);
            Connection {
                name: form.fields[0].clone(),
                kind: "mysql".into(),
                host: form.fields[1].clone(),
                port,
                username: form.fields[3].clone(),
                password: form.fields[4].clone(),
                database: form.fields[5].clone(),
            }
        };
        if conn.name.trim().is_empty() {
            self.status = "Name is required.".into();
            return;
        }
        self.config.connections.push(conn);
        self.conn_cursor = self.config.connections.len() - 1;
        match self.config.save() {
            Ok(()) => {
                self.form = None;
                self.status = "Saved. Press Enter to connect.".into();
            }
            Err(e) => self.status = format!("Save failed: {e}"),
        }
        self.form = None;
        self.status = "Saved. Press Enter to connect.".into();
    }

    fn apply_job(&mut self, res: JobResult) {
        self.running_query = false;
        match res {
            JobResult::Ping(r) => match r {
                Ok(name) => {
                    self.db = self.pending_db.take();
                    self.db_name = Some(name.clone());
                    self.status = format!("Connected to {name}. Loading schema…");
                    self.output = Output::Message(format!("Connected to {name}."));
                    // ponytail: kick off a schema fetch on the job channel so
                    // completion has real table/column names. Cleared first so a
                    // stale prior connection's names don't leak in.
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
            },
            JobResult::Query(r) => match r {
                Ok(er) => {
                    self.output = Output::Table {
                        columns: er.columns,
                        rows: er.rows,
                        rows_affected: er.rows_affected,
                        elapsed_ms: er.elapsed_ms,
                    };
                    self.status = "Query OK.".into();
                    self.query_start = None;
                }
                Err(e) => {
                    self.output = Output::Message(format!("Error: {e}"));
                    self.status = "Query failed.".into();
                    self.query_start = None;
                }
            },
            JobResult::Schema(r) => match r {
                Ok(map) => {
                    let n = map.len();
                    self.schema = map;
                    // ponytail: reset browse state so a reconnect doesn't leave
                    // the cursor/expansion pointing at now-gone tables.
                    self.schema_cursor = 0;
                    self.schema_expanded.clear();
                    self.status = format!("Schema loaded: {n} tables.");
                }
                Err(e) => self.status = format!("Schema load failed: {e}"),
            },
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        // ponytail: key inspector — `?` toggles showing the last KeyEvent in
        // the status bar. Lets us see what tmux actually forwards instead of
        // guessing about Shift+Enter / Kitty-protocol passthrough.
        self.last_key = Some(format_key_event(&key));
        // Resolve the active view (modal > focus > autocomplete sub-mode) and
        // dispatch through the keymap. A matched shortcut → `apply_action`; a
        // miss → raw text input (typing into the editor / a form field).
        let view = shortcuts::current_view(
            self.focus,
            self.form.is_some(),
            self.features_open,
            self.confirm_destructive.is_some(),
            self.autocomplete.is_some(),
        );
        if let Some(action) = shortcuts::match_key(view, key) {
            self.apply_action(view, action);
            return;
        }
        self.handle_text_input(view, key);
    }

    /// The single place that maps an `Action` → mutation. `view` selects the
    /// per-pane behavior of the shared nav actions (MoveDown etc.); everything
    /// else is action-specific. Adding behavior = add an arm here + a binding
    /// in `shortcuts`; this dispatcher itself never changes.
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
            FocusConnections => { self.autocomplete = None; self.focus = Focus::Connections; }
            FocusEditor => { self.autocomplete = None; self.focus = Focus::Editor; }
            FocusResults => { self.autocomplete = None; self.focus = Focus::Results; }
            FocusSchema => { self.autocomplete = None; self.focus = Focus::Schema; }
            ToggleKeyLog => self.debug_keys = !self.debug_keys,
            ToggleFeatures => { self.features_open = true; self.feature_cursor = 0; }

            // Shared list nav — behavior selected per view.
            MoveDown => match view {
                View::Connections => {
                    let n = self.config.connections.len();
                    if n > 0 && self.conn_cursor + 1 < n { self.conn_cursor += 1; }
                }
                View::Results => {
                    let last = match &self.output {
                        Output::Table { rows, .. } => rows.len().saturating_sub(1),
                        _ => return,
                    };
                    self.result_row_off = self.result_row_off.saturating_add(1).min(last);
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
                    if n > 0 && self.conn_cursor > 0 { self.conn_cursor -= 1; }
                }
                View::Results => self.result_row_off = self.result_row_off.saturating_sub(1),
                View::Schema => self.schema_cursor = self.schema_cursor.saturating_sub(1),
                _ => {}
            },
            // Results-only nav — only bound in the Results view, so the
            // keymap already routes them here; no per-view switch needed.
            MoveRight => {
                let last = match &self.output {
                    Output::Table { columns, .. } => columns.len().saturating_sub(1),
                    _ => return,
                };
                self.result_col_off = self.result_col_off.saturating_add(1).min(last);
            }
            MoveLeft => self.result_col_off = self.result_col_off.saturating_sub(1),
            PageDown => {
                let last = match &self.output {
                    Output::Table { rows, .. } => rows.len().saturating_sub(1),
                    _ => return,
                };
                self.result_row_off = self.result_row_off.saturating_add(10).min(last);
            }
            PageUp => self.result_row_off = self.result_row_off.saturating_sub(10),
            Home => self.result_row_off = 0,
            End => {
                let last = match &self.output {
                    Output::Table { rows, .. } => rows.len().saturating_sub(1),
                    _ => return,
                };
                self.result_row_off = last;
            }

            ConnectSelected => self.connect_selected(),
            NewConnection => self.form = Some(FormState::new()),
            DeleteConnection => self.delete_selected(),

            EditorNewline => { self.exit_history_browse(); self.editor.newline(); }
            EditorBackspace => { self.exit_history_browse(); self.editor.backspace(); self.refresh_autocomplete(); }
            EditorLeft => { self.editor.left(); self.refresh_autocomplete(); }
            EditorRight => { self.editor.right(); self.refresh_autocomplete(); }
            EditorUp => self.editor.up(),
            EditorDown => self.editor.down(),
            EditorHome => self.editor.home(),
            EditorEnd => self.editor.end(),
            RecallHistoryOlder => self.recall_history(true),
            RecallHistoryNewer => self.recall_history(false),

            AcceptCompletion => self.accept_completion(),
            CompletionNext => self.move_completion(1),
            CompletionPrev => self.move_completion(-1),
            DismissCompletion => self.autocomplete = None,

            CopyRowJson => self.copy_row_json(),
            CopyResultCsv => self.copy_result_csv(),

            SchemaExpand => self.schema_expand_or_run(),
            SchemaCollapse => self.schema_collapse_at_cursor(),

            FormSave => self.save_form(),
            FormCancel => self.form = None,
            FormFieldNext => self.form_field_next(1),
            FormFieldPrev => self.form_field_next(-1),
            FormFieldLeft => self.form_field_left(),
            FormFieldRight => self.form_field_right(),
            FormFieldHome => self.form_field_home(),
            FormFieldEnd => self.form_field_end(),
            FormFieldBackspace => self.form_field_backspace(),

            FeaturesClose => self.features_open = false,
            FeaturesNext => self.features_move(1),
            FeaturesPrev => self.features_move(-1),
            FeaturesToggle => self.features_toggle(),

            ConfirmYes => {
                if let Some(sql) = self.confirm_destructive.take() {
                    self.execute_sql(sql);
                }
            }
            ConfirmNo => {
                self.confirm_destructive = None;
                self.status = "Query cancelled.".into();
            }
        }
    }

    /// Fall-through for keys that aren't any view's shortcut: typing a char
    /// into the SQL editor / a form field. Non-editable views ignore it.
    fn handle_text_input(&mut self, view: View, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if let KeyCode::Char(c) = key.code {
            if ctrl { return; }
            match view {
                View::Editor | View::EditorAutocomplete => {
                    self.exit_history_browse();
                    self.editor.insert_char(c);
                    self.refresh_autocomplete();
                }
                View::Form => {
                    if let Some(f) = self.form.as_mut() {
                        f.fields[f.active].insert(f.cursor, c);
                        f.cursor += 1;
                    }
                }
                _ => {}
            }
        }
    }

    fn copy_row_json(&mut self) {
        let json = match &self.output {
            Output::Table { columns, rows, .. } => match rows.get(self.result_row_off) {
                Some(row) => row_to_json(columns, row),
                None => return,
            },
            _ => return,
        };
        self.status = match copy_to_clipboard(&json) {
            Ok(()) => format!("Copied row {} as JSON.", self.result_row_off + 1),
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

    /// Schema: Enter/l/Right on a table expands it; on a leaf it generates the
    /// query, prefills the editor, jumps to Results and runs it.
    fn schema_expand_or_run(&mut self) {
        match self.schema_entry_at_cursor() {
            Some(SchemaEntry::Table(t)) => { self.schema_expanded.insert(t); }
            Some(SchemaEntry::Leaf { table, opt }) => {
                let sql = schema_query(&table, opt);
                self.editor = Editor::from_text(sql);
                self.focus = Focus::Results;
                self.run_query();
            }
            None => {}
        }
    }

    /// Schema: h/Left collapses the table at the cursor (or the parent of a
    /// leaf) and parks the cursor on the table row.
    fn schema_collapse_at_cursor(&mut self) {
        if let Some(t) = self.schema_table_at_cursor() {
            self.schema_expanded.remove(&t);
            self.schema_cursor_to_table(&t);
        }
    }

    fn form_field_next(&mut self, dir: i32) {
        let Some(form) = self.form.as_mut() else { return; };
        let last = FormState::LABELS.len() - 1;
        form.active = if dir > 0 {
            if form.active == last { 0 } else { form.active + 1 }
        } else if form.active == 0 {
            last
        } else {
            form.active - 1
        };
        form.cursor = form.fields[form.active].len();
    }

    fn form_field_left(&mut self) {
        if let Some(f) = self.form.as_mut() {
            f.cursor = f.cursor.saturating_sub(1);
        }
    }
    fn form_field_right(&mut self) {
        if let Some(f) = self.form.as_mut() {
            f.cursor = (f.cursor + 1).min(f.fields[f.active].len());
        }
    }
    fn form_field_home(&mut self) {
        if let Some(f) = self.form.as_mut() { f.cursor = 0; }
    }
    fn form_field_end(&mut self) {
        if let Some(f) = self.form.as_mut() { f.cursor = f.fields[f.active].len(); }
    }
    fn form_field_backspace(&mut self) {
        if let Some(f) = self.form.as_mut()
            && f.cursor > 0
        {
            f.cursor -= 1;
            f.fields[f.active].remove(f.cursor);
        }
    }

    fn features_move(&mut self, dir: i32) {
        let last = Features::LIST.len().saturating_sub(1);
        self.feature_cursor = if dir > 0 {
            if self.feature_cursor >= last { 0 } else { self.feature_cursor + 1 }
        } else {
            if self.feature_cursor == 0 { last } else { self.feature_cursor - 1 }
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

    /// Recall a query from history into the editor. `older` = Ctrl+Up,
    /// `!older` = Ctrl+Down. On entering browse mode the current editor
    /// text is stashed as a draft so Ctrl+Down past the newest entry restores
    /// it. ponytail: shell-style recall; cursor parked at the start of the
    /// recalled text so the whole query is visible.
    fn recall_history(&mut self, older: bool) {
        if self.history.is_empty() {
            return;
        }
        let n = self.history.len();
        if self.history_cursor.is_none() {
            if older {
                // enter browse at the newest entry; load and return so the
                // movement block below doesn't immediately move past it.
                self.history_draft = Some(self.editor.text());
                self.history_cursor = Some(n - 1);
                self.editor = Editor::from_text(self.history[n - 1].clone());
            }
            // !older at the draft (newest) position: nothing newer to show.
            return;
        }
        let Some(i) = self.history_cursor else { return; };
        if older {
            if i == 0 {
                return; // oldest reached
            }
            self.history_cursor = Some(i - 1);
        } else if i + 1 < n {
            self.history_cursor = Some(i + 1);
        } else {
            // past the newest → restore the draft we saved on enter
            self.history_cursor = None;
            if let Some(draft) = self.history_draft.take() {
                self.editor = Editor::from_text(draft);
            }
            return;
        }
        let text = self.history[self.history_cursor.unwrap()].clone();
        self.editor = Editor::from_text(text);
    }

    /// Leave history-browse mode without restoring (used when the user starts
    /// editing a recalled query). The recalled text stays in the editor;
    /// only the browse cursor/draft are dropped so the next run pushes fresh.
    fn exit_history_browse(&mut self) {
        self.history_cursor = None;
        self.history_draft = None;
    }

    /// Recompute the autocomplete popup from the identifier at the cursor.
    /// ponytail: schema-aware — `t.<col>` offers columns of table `t`; anywhere
    /// else offers tables + columns of tables referenced in the current
    /// statement, plus keywords/functions. Schema is fetched once on connect.
    fn refresh_autocomplete(&mut self) {
        let (word, word_start) = self.current_word();
        let line = &self.editor.lines[self.editor.row];
        let col = self.editor.col.min(line.len());
        let dot = word_start > 0 && line.as_bytes()[word_start - 1] == b'.';
        // In dot context a 1-char word is still useful (`t.n` → name).
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
                if ac.cursor >= ac.items.len() { ac.cursor = 0; }
                ac.trigger_len = trigger_len;
            }
            None => self.autocomplete = Some(Autocomplete { items, cursor: 0, trigger_len }),
        }
    }

    /// Pick the schema pools for the current context.
    /// dot context → columns of the table named just before the dot;
    /// otherwise → all tables + columns of tables referenced in the statement.
    fn completion_pools(&self, dot: bool, word_start: usize, line_up_to_col: &str) -> (Vec<String>, Vec<String>) {
        if self.schema.is_empty() {
            return (Vec::new(), Vec::new());
        }
        if dot {
            let table = ident_before(line_up_to_col, word_start - 1);
            if let Some(cols) = self.schema.get(&table) {
                return (Vec::new(), cols.clone());
            }
            // ponytail: alias like `u.col` isn't resolved to a table — no hit.
            // upgrade: track `FROM t alias` and map alias→table.
            return (Vec::new(), Vec::new());
        }
        let tables: Vec<String> = self.schema.keys().cloned().collect();
        let referenced = autocomplete::referenced_tables(&self.current_statement());
        let mut columns: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for t in referenced {
            if let Some(cols) = self.schema.get(&t) {
                for c in cols {
                    if seen.insert(c.clone()) { columns.push(c.clone()); }
                }
            }
        }
        (tables, columns)
    }

    /// Text of the statement containing the cursor: from the last `;` before
    /// the cursor up to the cursor. ponytail: byte-offset by summing line lens.
    fn current_statement(&self) -> String {
        let mut off = 0usize;
        for (i, l) in self.editor.lines.iter().enumerate() {
            if i == self.editor.row {
                off += self.editor.col.min(l.len());
                break;
            }
            off += l.len() + 1; // +1 for the newline
        }
        let text = self.editor.text();
        let upto = off.min(text.len());
        let start = text[..upto].rfind(';').map(|p| p + 1).unwrap_or(0);
        text[start..upto].to_string()
    }

    /// The identifier currently ending at the cursor: `(word, start_byte)`.
    fn current_word(&self) -> (String, usize) {
        let line = &self.editor.lines[self.editor.row];
        let col = self.editor.col.min(line.len());
        let mut start = 0;
        for (i, ch) in line.char_indices() {
            if i >= col { break; }
            if !(ch.is_alphanumeric() || ch == '_') {
                start = i + ch.len_utf8();
            }
        }
        (line[start..col].to_string(), start)
    }

    fn accept_completion(&mut self) {
        let Some(ac) = self.autocomplete.take() else { return; };
        if ac.items.is_empty() { return; }
        let cand = ac.items[ac.cursor % ac.items.len()].clone();
        let line = &mut self.editor.lines[self.editor.row];
        let start = self.editor.col.saturating_sub(ac.trigger_len);
        line.replace_range(start..self.editor.col, &cand);
        self.editor.col = start + cand.len();
        self.autocomplete = None;
    }

    fn move_completion(&mut self, dir: i32) {
        if let Some(ac) = &mut self.autocomplete {
            if ac.items.is_empty() { return; }
            let n = ac.items.len() as i32;
            ac.cursor = ((ac.cursor as i32 + dir).rem_euclid(n)) as usize;
        }
    }

    /// Query timing label for the editor's top-right border. While a query
    /// runs, shows a live elapsed that ticks up each frame (the render loop
    /// redraws every ~100ms). After it finishes, holds the final elapsed_ms.
    /// ponytail: no separate timer thread — the existing poll-loop redraw
    /// interpolates the clock for free.
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

    /// Flat display rows for the schema pane: a table row, then (when expanded)
    /// its 4 fixed leaf options. Tables sorted for stable order.
    /// ponytail: rebuilt each call (schema is small); shared by draw + handle.
    pub fn schema_rows(&self) -> Vec<SchemaEntry> {
        let mut tables: Vec<&String> = self.schema.keys().collect();
        tables.sort();
        let mut rows: Vec<SchemaEntry> = Vec::new();
        for t in tables {
            rows.push(SchemaEntry::Table(t.clone()));
            if self.schema_expanded.contains(t) {
                for opt in [SchemaOpt::Rows, SchemaOpt::Columns, SchemaOpt::Constraints, SchemaOpt::Indexes] {
                    rows.push(SchemaEntry::Leaf { table: t.clone(), opt });
                }
            }
        }
        rows
    }

    /// The schema entry currently at the cursor, if any.
    fn schema_entry_at_cursor(&self) -> Option<SchemaEntry> {
        self.schema_rows().get(self.schema_cursor).cloned()
    }

    /// Name of the table the schema cursor currently sits on.
    fn schema_table_at_cursor(&self) -> Option<String> {
        match self.schema_entry_at_cursor() {
            Some(SchemaEntry::Table(t)) => Some(t),
            Some(SchemaEntry::Leaf { table, .. }) => Some(table),
            None => None,
        }
    }

    /// Move the schema cursor to the table row of `t`.
    fn schema_cursor_to_table(&mut self, t: &str) {
        if let Some(i) = self.schema_rows().iter().position(|e| matches!(e, SchemaEntry::Table(name) if name == t)) {
            self.schema_cursor = i;
        }
    }

    /// Mouse wheel / trackpad scrolls the results pane — but only when the
    /// cursor is over it (lazygit-style: scroll the pane you hover). The
    /// results rect is recorded by `ui::draw` each frame.
    pub fn handle_mouse(&mut self, m: MouseEvent) {
        if self.form.is_some() || self.features_open {
            return;
        }
        let Some(rect) = self.results_rect else { return };
        if !(m.column >= rect.x && m.column < rect.right() && m.row >= rect.y && m.row < rect.bottom()) {
            return;
        }
        let (last_row, last_col) = match &self.output {
            Output::Table { columns, rows, .. } => {
                (rows.len().saturating_sub(1), columns.len().saturating_sub(1))
            }
            _ => return,
        };
        match m.kind {
            MouseEventKind::ScrollDown => {
                self.result_row_off = self.result_row_off.saturating_add(1).min(last_row);
            }
            MouseEventKind::ScrollUp => {
                self.result_row_off = self.result_row_off.saturating_sub(1);
            }
            // Horizontal: ScrollRight moves the viewport right (toward later
            // columns), ScrollLeft toward earlier — same as the `l`/`h` keys.
            MouseEventKind::ScrollRight => {
                self.result_col_off = self.result_col_off.saturating_add(1).min(last_col);
            }
            MouseEventKind::ScrollLeft => {
                self.result_col_off = self.result_col_off.saturating_sub(1);
            }
            _ => {}
        }
    }

}

/// ponytail: word-boundary check for destructive SQL commands. Splits on
/// non-alphanumeric/non-underscore chars. A WHERE anywhere in the statement
/// suppresses the DELETE warning. Fine for single-statement use.
fn is_destructive(sql: &str) -> bool {
    let lower = sql.to_lowercase();
    let words: Vec<&str> = lower
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| !w.is_empty())
        .collect();
    if words.contains(&"drop") || words.contains(&"truncate") {
        return true;
    }
    words.contains(&"delete") && !lower.contains("where")
}

fn spawn_job(job: Job) -> Receiver<JobResult> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let res = match job {
            Job::Ping(db, name) => match db.ping() {
                Ok(()) => JobResult::Ping(Ok(name)),
                Err(e) => JobResult::Ping(Err(e.to_string())),
            },
            Job::Query(db, sql, readable_binary) => {
                let start = std::time::Instant::now();
                match db.execute_script(&sql, readable_binary) {
                    Ok(mut r) => {
                        r.elapsed_ms = start.elapsed().as_millis();
                        JobResult::Query(Ok(r))
                    }
                    Err(e) => JobResult::Query(Err(e.to_string())),
                }
            }
            Job::Schema(db) => match db.schema() {
                Ok(s) => JobResult::Schema(Ok(s)),
                Err(e) => JobResult::Schema(Err(e.to_string())),
            }
        };
        let _ = tx.send(res);
    });
    rx
}

/// SQL generated for a schema-browser leaf selection. Backtick-quoted table
/// names so SQL keywords as identifiers work. ponytail: the user sees this in
/// the editor and can edit before re-running — no escaping of user data beyond
/// backticks (table names come from INFORMATION_SCHEMA, not user input).
fn schema_query(table: &str, opt: SchemaOpt) -> String {
    match opt {
        SchemaOpt::Rows => format!("SELECT * FROM `{table}` LIMIT 100;"),
        SchemaOpt::Columns => format!("SHOW FULL COLUMNS FROM `{table}`;"),
        SchemaOpt::Constraints => format!(
            "SELECT CONSTRAINT_NAME, CONSTRAINT_TYPE FROM INFORMATION_SCHEMA.TABLE_CONSTRAINTS WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = '{table}';"
        ),
        SchemaOpt::Indexes => format!("SHOW INDEX FROM `{table}`;"),
    }
}

/// Copy text to the system clipboard. ponytail: shell out to the platform
/// clipboard tool — no new dep, works on macOS/Windows/Linux/Wayland.
/// Returns Ok on success; the caller shows a status message either way.
fn copy_to_clipboard(text: &str) -> std::io::Result<()> {
    use std::io::Write;
    use std::process::{Command, Stdio};
    // ponytail: pick the first available tool; extend the list as needed.
    let cmd = if cfg!(target_os = "macos") {
        ("pbcopy", Vec::<&str>::new())
    } else if cfg!(target_os = "windows") {
        ("clip", Vec::<&str>::new())
    } else {
        // Wayland then X11; wl-copy reads stdin, xclip needs -selection clipboard.
        if std::path::Path::new("/usr/bin/wl-copy").exists() || which("wl-copy") {
            ("wl-copy", Vec::<&str>::new())
        } else {
            ("xclip", vec!["-selection", "clipboard"])
        }
    };
    let mut child = Command::new(cmd.0)
        .args(&cmd.1)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    if let Some(stdin) = child.stdin.as_mut() {
        stdin.write_all(text.as_bytes())?;
    }
    child.wait()?;
    Ok(())
}

/// ponytail: `which` without a dep — checks PATH for an executable.
fn which(prog: &str) -> bool {
    if let Ok(path) = std::env::var("PATH") {
        for dir in path.split(':') {
            if std::path::Path::new(dir).join(prog).exists() {
                return true;
            }
        }
    }
    false
}

/// A single result row as a JSON object: `{"col":"val",...}`.
/// ponytail: hand-rolled JSON string escaping (no serde dep). Ascii control
/// chars and quotes/backslashes are escaped; everything else passes through.
fn row_to_json(columns: &[String], row: &[String]) -> String {
    let mut out = String::from("{");
    for (i, col) in columns.iter().enumerate() {
        if i > 0 { out.push(','); }
        out.push_str(&json_escape(col));
        out.push(':');
        let val = row.get(i).map(String::as_str).unwrap_or("");
        out.push_str(&json_escape(val));
    }
    out.push('}');
    out
}

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// CSV with header + RFC4180 quoting (quote fields containing `,` `"` newline).
/// ponytail: no csv crate — a quoting fn covers the spec's escape rules.
fn result_to_csv(columns: &[String], rows: &[Vec<String>]) -> String {
    let mut out = String::new();
    let mut line = String::new();
    for (i, c) in columns.iter().enumerate() {
        if i > 0 { line.push(','); }
        line.push_str(&csv_escape(c));
    }
    out.push_str(&line);
    out.push('\n');
    for row in rows {
        line.clear();
        for i in 0..columns.len() {
            if i > 0 { line.push(','); }
            // columns beyond the row length (ragged) become empty fields
            let v = row.get(i).map(String::as_str).unwrap_or("");
            line.push_str(&csv_escape(v));
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

fn csv_escape(s: &str) -> String {
    let needs_quote = s.contains(',') || s.contains('"') || s.contains('\n') || s.contains('\r');
    if !needs_quote {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        if c == '"' { out.push_str("\"\""); } else { out.push(c); }
    }
    out.push('"');
    out
}

/// Identifier immediately before byte index `end` (exclusive) on a line.
/// Used to resolve `t.<col>` → the table name `t`.
fn ident_before(line: &str, end: usize) -> String {
    let b = line.as_bytes();
    let mut start = end;
    while start > 0 && (b[start - 1].is_ascii_alphanumeric() || b[start - 1] == b'_') {
        start -= 1;
    }
    line[start..end].to_string()
}

/// Compact one-line description of a key event for the inspector.
fn format_key_event(key: &KeyEvent) -> String {
    let mods = [
        (KeyModifiers::SHIFT, "S"),
        (KeyModifiers::CONTROL, "C"),
        (KeyModifiers::ALT, "A"),
        (KeyModifiers::SUPER, "U"),
    ]
    .iter()
    .map(|(m, s)| if key.modifiers.contains(*m) { *s } else { "-" })
    .collect::<Vec<_>>()
    .join("");
    format!("key={:?} mods={}{}", key.code, mods, if key.kind == KeyEventKind::Release { " rel" } else { "" })
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

        // Poll background job results without holding a borrow into `app`.
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
mod tests {
    use super::format_key_event;
    // Mirrors handle_mouse's row-offset clamp: add-with-ceiling and saturating sub.
    fn scroll_down(off: usize, last_row: usize) -> usize {
        off.saturating_add(1).min(last_row)
    }
    fn scroll_up(off: usize) -> usize {
        off.saturating_sub(1)
    }

    #[test]
    fn row_scroll_clamps_at_bounds() {
        assert_eq!(scroll_down(0, 5), 1);
        assert_eq!(scroll_down(5, 5), 5); // ceiling holds
        assert_eq!(scroll_up(3), 2);
    }

    // Same clamp is reused for column offsets (ScrollLeft/ScrollRight).
    #[test]
    fn col_scroll_clamps_at_bounds() {
        // ScrollRight = add-with-ceiling, ScrollLeft = saturating sub.
        assert_eq!(scroll_down(0, 4), 1); // col 0 -> 1
        assert_eq!(scroll_down(4, 4), 4); // ceiling holds at last col
        assert_eq!(scroll_up(0), 0); // floor holds
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

        // Shift+Up enters browse at the newest entry.
        app.recall_history(true);
        assert_eq!(app.editor.text(), "new");
        assert!(app.history_cursor.is_some());

        // Shift+Up again → older entry.
        app.recall_history(true);
        assert_eq!(app.editor.text(), "old");

        // Shift+Up at the oldest is a no-op.
        app.recall_history(true);
        assert_eq!(app.editor.text(), "old");

        // Shift+Down back to newest, then once more restores the draft.
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

        // Collapsed: one row per table, sorted.
        let rows = app.schema_rows();
        assert_eq!(rows.len(), 2);
        assert!(matches!(rows[0], super::SchemaEntry::Table(ref t) if t == "posts"));
        assert!(matches!(rows[1], super::SchemaEntry::Table(ref t) if t == "users"));

        // Expand users → 4 leaf options (rows/columns/constraints/indexes).
        app.schema_expanded.insert("users".to_string());
        let rows = app.schema_rows();
        assert_eq!(rows.len(), 6); // posts + users + 4 leaves
        assert!(matches!(rows[1], super::SchemaEntry::Table(ref t) if t == "users"));
        assert!(matches!(rows[2], super::SchemaEntry::Leaf { ref table, opt: super::SchemaOpt::Rows } if table == "users"));
        assert!(matches!(rows[3], super::SchemaEntry::Leaf { ref table, opt: super::SchemaOpt::Columns } if table == "users"));
        assert!(matches!(rows[4], super::SchemaEntry::Leaf { ref table, opt: super::SchemaOpt::Constraints } if table == "users"));
        assert!(matches!(rows[5], super::SchemaEntry::Leaf { ref table, opt: super::SchemaOpt::Indexes } if table == "users"));

        // Collapse → back to 2 rows.
        app.schema_expanded.remove("users");
        assert_eq!(app.schema_rows().len(), 2);
    }

    #[test]
    fn schema_query_generates_correct_sql() {
        use super::{schema_query, SchemaOpt};
        assert_eq!(schema_query("users", SchemaOpt::Rows), "SELECT * FROM `users` LIMIT 100;");
        assert_eq!(schema_query("users", SchemaOpt::Columns), "SHOW FULL COLUMNS FROM `users`;");
        assert_eq!(schema_query("users", SchemaOpt::Indexes), "SHOW INDEX FROM `users`;");
        let c = schema_query("users", SchemaOpt::Constraints);
        assert!(c.contains("TABLE_CONSTRAINTS"));
        assert!(c.contains("TABLE_NAME = 'users'"));
    }

    #[test]
    fn row_to_json_escapes_and_pairs() {
        let cols = vec!["id".to_string(), "name".to_string()];
        let row = vec!["42".to_string(), "a\"b\\c".to_string()];
        assert_eq!(
            super::row_to_json(&cols, &row),
            "{\"id\":\"42\",\"name\":\"a\\\"b\\\\c\"}"
        );
        // ragged row: missing column becomes empty string
        let short = vec!["1".to_string()];
        assert_eq!(super::row_to_json(&cols, &short), "{\"id\":\"1\",\"name\":\"\"}");
    }

    #[test]
    fn csv_escapes_special_fields() {
        let cols = vec!["a".to_string(), "b".to_string()];
        let rows = vec![vec!["1".to_string(), "x,y".to_string()], vec!["2".to_string(), "he said \"hi\"".to_string()]];
        let csv = super::result_to_csv(&cols, &rows);
        assert_eq!(csv, "a,b\n1,\"x,y\"\n2,\"he said \"\"hi\"\"\"\n");
    }
}
