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

pub struct FormState {
    pub kind: String,
    pub fields: [String; 6],
    pub active: usize,
    pub cursor: usize,
    pub edit_index: Option<usize>,
}

impl FormState {
    pub const LABELS: [&'static str; 6] = ["Name", "Host", "Port", "User", "Password", "Database"];
    /// Backend kinds cyclable via the Type row (Ctrl+K). Add a string here +
    /// a `default_port` arm + an `open` arm + an impl module to add a backend.
    pub const KINDS: [&'static str; 2] = ["mysql", "postgres"];

    /// Default port for a kind — the port field's fallback on parse failure,
    /// and swapped in automatically when cycling kinds leaves the other kind's
    /// default in the field.
    pub fn default_port(kind: &str) -> u16 {
        match kind {
            "postgres" => 5432,
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
        }
    }

    /// Cycle the Type row to the next backend kind, swapping the port field if
    /// it still holds the previous kind's default (so mysql→postgres flips
    /// 3306→5432). A user-edited port is left alone.
    pub fn cycle_kind(&mut self) {
        let i = Self::KINDS
            .iter()
            .position(|k| *k == self.kind)
            .unwrap_or(0);
        let next = Self::KINDS[(i + 1) % Self::KINDS.len()];
        if self.fields[2] == Self::default_port(&self.kind).to_string() {
            self.fields[2] = Self::default_port(next).to_string();
        }
        self.kind = next.into();
    }
}

pub struct Autocomplete {
    pub items: Vec<String>,
    pub cursor: usize,
    pub trigger_len: usize,
}
