use std::collections::HashMap;

use ratatui::layout::Rect;

use crate::db::Connection;
use crate::filter::CellMatches;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Connections,
    Editor,
    Results,
    Schema,
}

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
    View(String),
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
        truncated: bool,
    },
}

pub struct ResultsClickGeom {
    pub body: Rect,
    pub cols: Vec<(usize, u16, u16)>,
}

pub struct ResultFilter {
    pub query: String,
    pub matched: Vec<usize>,
    pub offsets: HashMap<usize, CellMatches>,
}

pub struct EditCellState {
    pub raw_value: String,
    pub abs_row: usize,
    pub col: usize,
    pub col_name: String,
    pub table: String,
    pub pk_cols: Vec<String>,
    pub pk_vals: Vec<String>,
    pub cursor: usize,
    pub original_value: String,
}

pub struct EditCellPending {
    pub abs_row: usize,
    pub col: usize,
    pub cell_value: String,
    pub col_name: String,
    pub table: String,
    pub columns_len: usize,
    pub rows_len: usize,
}

pub struct KindPickerState {
    pub query: String,
    pub filtered: Vec<usize>,
    pub cursor: usize,
}

impl KindPickerState {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            filtered: (0..FormState::KINDS.len()).collect(),
            cursor: 0,
        }
    }

    pub fn set_query(&mut self, query: String) {
        self.query = query;
        let q = self.query.to_lowercase();
        self.filtered = FormState::KINDS
            .iter()
            .enumerate()
            .filter(|(_, k)| k.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect();
        let max = self.filtered.len().saturating_sub(1);
        if self.cursor > max {
            self.cursor = max;
        }
    }

    pub fn selected_kind(&self) -> Option<&'static str> {
        let i = *self.filtered.get(self.cursor)?;
        Some(FormState::KINDS[i])
    }
}

pub struct FormState {
    pub kind: String,
    pub fields: [String; 6],
    pub active: usize,
    pub cursor: usize,
    pub edit_index: Option<usize>,
    pub kind_picker: Option<KindPickerState>,
}

impl FormState {
    pub const LABELS: [&'static str; 6] = ["Name", "Host", "Port", "User", "Password", "Database"];
    /// Backend kinds. Add a string here + a `default_port` arm + an `open`
    /// arm + an impl module to add a backend.
    pub const KINDS: [&'static str; 4] = ["mysql", "postgres", "sqlite", "mssql"];

    /// Default port for a kind — the port field's fallback on parse failure,
    /// and swapped in when selecting a new kind in the picker.
    pub fn default_port(kind: &str) -> u16 {
        match kind {
            "postgres" => 5432,
            "sqlite" => 0,
            "mssql" => 1433,
            _ => 3306,
        }
    }

    pub fn new() -> Self {
        Self {
            kind: Self::KINDS[0].into(),
            fields: [
                String::new(),
                "127.0.0.1".into(),
                Self::default_port(Self::KINDS[0]).to_string(),
                String::new(),
                String::new(),
                String::new(),
            ],
            active: 0,
            cursor: 0,
            edit_index: None,
            kind_picker: None,
        }
    }

    /// Pre-fill the form from an existing connection for editing; `idx` is the
    /// connection slot to overwrite on save.
    pub fn from_connection(idx: usize, c: &Connection) -> Self {
        Self {
            kind: c.kind.clone(),
            fields: [
                c.name.clone(),
                c.host.clone(),
                c.port.to_string(),
                c.username.clone(),
                c.password.clone(),
                c.database.clone(),
            ],
            active: 0,
            cursor: c.name.len(),
            edit_index: Some(idx),
            kind_picker: None,
        }
    }
}

pub struct Autocomplete {
    pub items: Vec<String>,
    pub cursor: usize,
    pub trigger_len: usize,
}

// ── Sort ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

pub type SortState = Option<(usize, SortDir)>;

// ── Cell inspect popup ────────────────────────────────────────────────

#[derive(Clone)]
pub struct CellInspect {
    pub col_name: String,
    pub cell_value: String,
    pub abs_row: usize,
    pub col: usize,
    pub scroll: usize,
}

// ── Export path input ────────────────────────────────────────────────

#[derive(Clone)]
pub struct ExportInput {
    pub path: String,
    pub format: ExportFormat,
    pub cursor: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Json,
    JsonLines,
    SqlInsert,
}

impl ExportFormat {
    pub const LABELS: [&'static str; 4] = ["CSV", "JSON (array)", "JSON (lines)", "SQL INSERT"];
    pub fn extension(&self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
            Self::JsonLines => "jsonl",
            Self::SqlInsert => "sql",
        }
    }
}

// ── Row insert ────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct RowInsertState {
    pub table: String,
    pub columns: Vec<String>,
    pub values: Vec<String>,
    pub cursor_col: usize,
    pub cursor_char: usize,
}

// ponytail: Row delete uses the same PrimaryKeys job flow as cell edit.
