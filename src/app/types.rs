use std::collections::HashMap;

use ratatui::layout::Rect;

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

pub struct Autocomplete {
    pub items: Vec<String>,
    pub cursor: usize,
    pub trigger_len: usize,
}
