use std::io::Stdout;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{backend::CrosstermBackend, Terminal};

use crate::config::Config;
use crate::db::{self, Connection, Database, ExecutionResult};
use crate::editor::Editor;
use crate::ui;

type Term = Terminal<CrosstermBackend<Stdout>>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Connections,
    Editor,
    Results,
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
    Query(Box<dyn Database>, String),
}

enum JobResult {
    Ping(Result<String, String>),
    Query(Result<ExecutionResult, String>),
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
    pub form: Option<FormState>,
    rx: Option<Receiver<JobResult>>,
    pub status: String,
    pub result_row_off: usize,
    pub result_col_off: usize,
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
            form: None,
            rx: None,
            status: "Press 'n' to add a connection, then Enter to connect.".into(),
            result_row_off: 0,
            result_col_off: 0,
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
        let _ = self.config.save();
        self.status = "Connection deleted.".into();
    }

    fn run_query(&mut self) {
        if self.running_query {
            self.status = "A query is already running.".into();
            return;
        }
        let Some(db) = self.db.as_ref() else {
            self.status = "Not connected — select a connection and press Enter.".into();
            return;
        };
        let sql = self.editor.text();
        if sql.trim().is_empty() {
            self.status = "Editor is empty.".into();
            return;
        }
        let db = db.boxed_clone();
        self.rx = Some(spawn_job(Job::Query(db, sql)));
        self.running_query = true;
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
        let _ = self.config.save();
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
                    self.status = format!("Connected to {name}.");
                    self.output = Output::Message(format!("Connected to {name}."));
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
                }
                Err(e) => {
                    self.output = Output::Message(format!("Error: {e}"));
                    self.status = "Query failed.".into();
                }
            },
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && (key.code == KeyCode::Char('c') || key.code == KeyCode::Char('q')) {
            self.running = false;
            return;
        }
        if ctrl && key.code == KeyCode::Char('r') || key.code == KeyCode::F(5) {
            self.run_query();
            return;
        }
        if self.form.is_some() {
            self.handle_form(key);
            return;
        }
        match key.code {
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::Connections => Focus::Editor,
                    Focus::Editor => Focus::Results,
                    Focus::Results => Focus::Connections,
                };
            }
            KeyCode::Char('q') if self.focus != Focus::Editor => self.running = false,
            _ => match self.focus {
                Focus::Connections => self.handle_connections(key),
                Focus::Editor => self.handle_editor(key),
                Focus::Results => self.handle_results(key),
            },
        }
    }

    fn handle_connections(&mut self, key: KeyEvent) {
        let n = self.config.connections.len();
        match key.code {
            KeyCode::Down | KeyCode::Char('j') if n > 0 && self.conn_cursor + 1 < n => {
                self.conn_cursor += 1;
            }
            KeyCode::Up | KeyCode::Char('k') if n > 0 && self.conn_cursor > 0 => {
                self.conn_cursor -= 1;
            }
            KeyCode::Enter if n > 0 => self.connect_selected(),
            KeyCode::Char('n') => self.form = Some(FormState::new()),
            KeyCode::Char('d') if n > 0 => self.delete_selected(),
            _ => {}
        }
    }

    fn handle_editor(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.editor.insert_char(c);
            }
            KeyCode::Enter => self.editor.newline(),
            KeyCode::Backspace => self.editor.backspace(),
            KeyCode::Left => self.editor.left(),
            KeyCode::Right => self.editor.right(),
            KeyCode::Up => self.editor.up(),
            KeyCode::Down => self.editor.down(),
            KeyCode::Home => self.editor.home(),
            KeyCode::End => self.editor.end(),
            _ => {}
        }
    }

    fn handle_results(&mut self, key: KeyEvent) {
        let (nrows, ncols) = match &self.output {
            Output::Table { columns, rows, .. } => (rows.len(), columns.len()),
            _ => return,
        };
        let last_row = nrows.saturating_sub(1);
        let last_col = ncols.saturating_sub(1);
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => {
                self.result_row_off = self.result_row_off.saturating_add(1).min(last_row);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.result_row_off = self.result_row_off.saturating_sub(1);
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.result_col_off = self.result_col_off.saturating_add(1).min(last_col);
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.result_col_off = self.result_col_off.saturating_sub(1);
            }
            KeyCode::PageDown => {
                self.result_row_off = self.result_row_off.saturating_add(10).min(last_row);
            }
            KeyCode::PageUp => {
                self.result_row_off = self.result_row_off.saturating_sub(10);
            }
            KeyCode::Home => self.result_row_off = 0,
            KeyCode::End => self.result_row_off = last_row,
            _ => {}
        }
    }

    fn handle_form(&mut self, key: KeyEvent) {
        // Esc / Enter are handled before borrowing `self.form`.
        match key.code {
            KeyCode::Esc => {
                self.form = None;
                return;
            }
            KeyCode::Enter => {
                self.save_form();
                return;
            }
            _ => {}
        }
        let form = self.form.as_mut().unwrap();
        let last = FormState::LABELS.len() - 1;
        match key.code {
            KeyCode::Tab | KeyCode::Down => {
                form.active = if form.active == last { 0 } else { form.active + 1 };
                form.cursor = form.fields[form.active].len();
            }
            KeyCode::BackTab | KeyCode::Up => {
                form.active = if form.active == 0 { last } else { form.active - 1 };
                form.cursor = form.fields[form.active].len();
            }
            KeyCode::Left if form.cursor > 0 => form.cursor -= 1,
            KeyCode::Right if form.cursor < form.fields[form.active].len() => form.cursor += 1,
            KeyCode::Home => form.cursor = 0,
            KeyCode::End => form.cursor = form.fields[form.active].len(),
            KeyCode::Backspace if form.cursor > 0 => {
                form.cursor -= 1;
                form.fields[form.active].remove(form.cursor);
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                form.fields[form.active].insert(form.cursor, c);
                form.cursor += 1;
            }
            _ => {}
        }
    }
}

fn spawn_job(job: Job) -> Receiver<JobResult> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let res = match job {
            Job::Ping(db, name) => match db.ping() {
                Ok(()) => JobResult::Ping(Ok(name)),
                Err(e) => JobResult::Ping(Err(e.to_string())),
            },
            Job::Query(db, sql) => {
                let start = std::time::Instant::now();
                match db.execute_script(&sql) {
                    Ok(mut r) => {
                        r.elapsed_ms = start.elapsed().as_millis();
                        JobResult::Query(Ok(r))
                    }
                    Err(e) => JobResult::Query(Err(e.to_string())),
                }
            }
        };
        let _ = tx.send(res);
    });
    rx
}

pub fn run(terminal: &mut Term, mut app: App) -> Result<()> {
    while app.running {
        terminal.draw(|f| ui::draw(f, &app))?;
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                app.handle_key(key);
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
