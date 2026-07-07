use ratatui::style::{Color, Modifier, Style};

// ── Pane borders + badges ────────────────────────────────────────────
pub const FOCUSED_BORDER: Style = Style::new().fg(Color::Blue).add_modifier(Modifier::BOLD);
pub const FOCUSED_BADGE: Style = Style::new().fg(Color::Blue).add_modifier(Modifier::BOLD);
pub const UNFOCUSED_BORDER: Style = Style::new().fg(Color::Reset);
pub const UNFOCUSED_BADGE: Style = Style::new().fg(Color::Reset);

// ── Connections ──────────────────────────────────────────────────────
pub const ACTIVE_CONNECTION: Style = Style::new().fg(Color::Green);
pub const CONNECTION_HIGHLIGHT: Style = Style::new()
    .add_modifier(Modifier::BOLD)
    .add_modifier(Modifier::REVERSED);

// ── Schema ───────────────────────────────────────────────────────────
pub const SCHEMA_CURSOR: Style = Style::new().add_modifier(Modifier::REVERSED);

// ── Autocomplete ─────────────────────────────────────────────────────
pub const AUTOCOMPLETE_CURSOR: Style = Style::new().add_modifier(Modifier::REVERSED);
pub const AUTOCOMPLETE_ITEM: Style = Style::new().bg(Color::DarkGray).fg(Color::White);

// ── Results table ────────────────────────────────────────────────────
pub const ROW_HIGHLIGHT: Style = Style::new().add_modifier(Modifier::REVERSED);
pub const COLUMN_HIGHLIGHT: Style = Style::new().add_modifier(Modifier::REVERSED);
pub const CELL_HIGHLIGHT: Style = Style::new().add_modifier(Modifier::REVERSED);

// ── Filter bar ───────────────────────────────────────────────────────
pub const FILTER_PROMPT: Style = Style::new().fg(Color::Blue).add_modifier(Modifier::BOLD);
pub const FILTER_QUERY: Style = Style::new().fg(Color::Gray).add_modifier(Modifier::BOLD);
pub const FILTER_CURSOR: Style = Style::new().fg(Color::Blue).add_modifier(Modifier::BOLD);

// ── Edit bar ─────────────────────────────────────────────────────────
pub const EDIT_PROMPT: Style = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);
pub const EDIT_VALUE: Style = Style::new().fg(Color::White).add_modifier(Modifier::BOLD);
pub const EDIT_CURSOR: Style = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);

// ── Matched char highlight (fuzzy filter glow) ───────────────────────
pub const MATCHED_CHAR: Style = Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD);

// ── Shortcuts bar ────────────────────────────────────────────────────
pub const SHORTCUT_KEY: Style = Style::new().fg(Color::Blue).add_modifier(Modifier::BOLD);
pub const SHORTCUT_LABEL: Style = Style::new().fg(Color::DarkGray);

// ── Form modal ───────────────────────────────────────────────────────
pub const FORM_BORDER: Style = Style::new().fg(Color::Blue);
pub const FORM_ACTIVE_FIELD: Style = Style::new().add_modifier(Modifier::REVERSED);
pub const FORM_LABEL: Style = Style::new().fg(Color::Gray);

// ── Features modal ───────────────────────────────────────────────────
pub const FEATURES_BORDER: Style = Style::new().fg(Color::Blue);
pub const FEATURE_CURSOR: Style = Style::new().add_modifier(Modifier::REVERSED);
pub const FEATURE_TOGGLE_ON: Style = Style::new().fg(Color::Green);
pub const FEATURE_TOGGLE_OFF: Style = Style::new().fg(Color::DarkGray);
pub const FEATURE_DESC: Style = Style::new().fg(Color::DarkGray);

// ── Destructive confirmation modal ───────────────────────────────────
pub const DESTRUCTIVE_BORDER: Style = Style::new().fg(Color::Red);
pub const DESTRUCTIVE_TEXT: Style = Style::new().fg(Color::Red);

// ── Placeholder / hint text ──────────────────────────────────────────
pub const PLACEHOLDER: Style = Style::new().fg(Color::DarkGray);

// ── Syntax highlighting (SQL editor) ─────────────────────────────────
pub const SQL_KEYWORD: Style = Style::new().fg(Color::Blue).add_modifier(Modifier::BOLD);
pub const SQL_FUNCTION: Style = Style::new().fg(Color::Blue);
pub const SQL_STRING: Style = Style::new().fg(Color::Green);
pub const SQL_NUMBER: Style = Style::new().fg(Color::Magenta);
pub const SQL_COMMENT: Style = Style::new().fg(Color::DarkGray);
