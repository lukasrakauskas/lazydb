use crossterm::event::{KeyEvent, KeyModifiers};

use super::types::{ResultsClickGeom, SchemaOpt};

pub fn click_to_cell(
    geom: &ResultsClickGeom,
    scroll_row: usize,
    x: u16,
    y: u16,
) -> (Option<usize>, Option<usize>) {
    let col = geom
        .cols
        .iter()
        .find(|(_, xs, w)| x >= *xs && x < *xs + *w)
        .map(|(c, _, _)| *c);
    let row = if y >= geom.body.y && y < geom.body.y + geom.body.height {
        Some(scroll_row + (y - geom.body.y) as usize)
    } else {
        None
    };
    (row, col)
}

pub fn is_destructive(sql: &str) -> bool {
    // ponytail: naive ';' split; adopt crate::db::sql::split_statements (P0 item 5) once landed
    for stmt in sql.split(';') {
        let stripped = strip_comments_and_strings(stmt);
        let lower = stripped.to_lowercase();
        let words: Vec<&str> = lower
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|w| !w.is_empty())
            .collect();
        if words.contains(&"drop")
            || words.contains(&"truncate")
            || words.contains(&"alter")
            || words.contains(&"rename")
        {
            return true;
        }
        if (words.contains(&"delete") || words.contains(&"update")) && !lower.contains("where") {
            return true;
        }
    }
    false
}
fn strip_comments_and_strings(sql: &str) -> String {
    let mut out = String::with_capacity(sql.len());
    let mut chars = sql.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\'' {
            // consume string literal
            while let Some(c) = chars.next() {
                if c == '\\' {
                    chars.next();
                } else if c == '\'' {
                    if chars.peek() == Some(&'\'') {
                        chars.next();
                    } else {
                        break;
                    }
                }
            }
        } else if c == '-' && chars.peek() == Some(&'-') {
            chars.next();
            for c in chars.by_ref() {
                if c == '\n' {
                    break;
                }
            }
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            loop {
                match chars.next() {
                    Some('*') if chars.peek() == Some(&'/') => {
                        chars.next();
                        break;
                    }
                    Some(_) => {}
                    None => break,
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// SQL for a schema-pane detail view, backend-specific. `kind` is the
/// connected `Database::kind()`. mysql uses SHOW/INFORMATION_SCHEMA + DATABASE();
// postgres uses the catalog + current_schema() (no DATABASE() equivalent).
/// ponytail: identifier quoting differs (`mysql` backtick vs `postgres`
/// double-quote) and is escaped; table names from the schema tree are the
/// stored (lowercased-if-unquoted) form, so quoting round-trips.
pub fn schema_query(table: &str, opt: SchemaOpt, kind: &str) -> String {
    if kind == "postgres" {
        let t = table.replace('"', "\"\"");
        match opt {
            SchemaOpt::Rows => format!("SELECT * FROM \"{t}\" LIMIT 100;"),
            SchemaOpt::Columns => format!(
                "SELECT column_name, data_type, is_nullable, column_default \
                 FROM information_schema.columns \
                 WHERE table_schema = current_schema() AND table_name = '{table}' \
                 ORDER BY ordinal_position;"
            ),
            SchemaOpt::Constraints => format!(
                "SELECT constraint_name, constraint_type \
                 FROM information_schema.table_constraints \
                 WHERE table_schema = current_schema() AND table_name = '{table}';"
            ),
            // ponytail: postgres has no `SHOW INDEX`; pg_indexes gives the
            // definition string (indexname + indexdef) which renders the same
            // role. upgrade: pg_index for a structured (cols, unique) view.
            SchemaOpt::Indexes => format!(
                "SELECT indexname, indexdef FROM pg_indexes \
                 WHERE schemaname = current_schema() AND tablename = '{table}';"
            ),
        }
    } else {
        match opt {
            SchemaOpt::Rows => format!("SELECT * FROM `{table}` LIMIT 100;"),
            SchemaOpt::Columns => format!("SHOW FULL COLUMNS FROM `{table}`;"),
            SchemaOpt::Constraints => format!(
                "SELECT CONSTRAINT_NAME, CONSTRAINT_TYPE FROM INFORMATION_SCHEMA.TABLE_CONSTRAINTS WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = '{table}';"
            ),
            SchemaOpt::Indexes => format!("SHOW INDEX FROM `{table}`;"),
        }
    }
}

pub fn extract_table_name(sql: &str) -> Option<String> {
    let lower = sql.to_lowercase();
    let from_idx = lower.find("from")?;
    let after_from = sql[from_idx + 4..].trim_start();
    if let Some(rest) = after_from.strip_prefix('`') {
        let end = rest.find('`')?;
        return Some(rest[..end].to_string());
    }
    let table = after_from.split_whitespace().next()?;
    let table = table.split(',').next()?;
    Some(table.to_string())
}

/// Identifier quote char for a backend (`mysql` backtick, `postgres`
/// double-quote). Used by `build_update_sql` so cell-edit emits valid SQL
/// per backend.
pub fn ident_quote(kind: &str) -> char {
    if kind == "postgres" { '"' } else { '`' }
}

/// Quote + escape an identifier for the backend's quote char.
fn quote_ident(name: &str, q: char) -> String {
    let escaped = name.replace(q, &format!("{q}{q}"));
    format!("{q}{escaped}{q}")
}

pub fn build_update_sql(
    table: &str,
    col: &str,
    new_val: &str,
    pk_cols: &[String],
    pk_vals: &[String],
    kind: &str,
) -> String {
    let q = ident_quote(kind);
    format!(
        "UPDATE {} SET {} = '{}' WHERE {}",
        quote_ident(table, q),
        quote_ident(col, q),
        sql_escape(new_val),
        pk_cols
            .iter()
            .zip(pk_vals.iter())
            .map(|(pc, pv)| format!("{} = '{}'", quote_ident(pc, q), sql_escape(pv)))
            .collect::<Vec<_>>()
            .join(" AND ")
    )
}

pub fn sql_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "''")
}

pub fn copy_to_clipboard(text: &str) -> std::io::Result<()> {
    if cfg!(test) {
        return Ok(());
    }
    use std::io::Write;
    use std::process::{Command, Stdio};
    let cmd = if cfg!(target_os = "macos") {
        ("pbcopy", Vec::<&str>::new())
    } else if cfg!(target_os = "windows") {
        ("clip", Vec::<&str>::new())
    } else if std::path::Path::new("/usr/bin/wl-copy").exists() || which("wl-copy") {
        ("wl-copy", Vec::<&str>::new())
    } else if which("xclip") {
        ("xclip", vec!["-selection", "clipboard"])
    } else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no clipboard tool found",
        ));
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

pub fn row_to_json(columns: &[String], row: &[String]) -> String {
    let mut out = String::from("{");
    for (i, col) in columns.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
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

pub fn result_to_csv(columns: &[String], rows: &[Vec<String>]) -> String {
    let mut out = String::new();
    let mut line = String::new();
    for (i, c) in columns.iter().enumerate() {
        if i > 0 {
            line.push(',');
        }
        line.push_str(&csv_escape(c));
    }
    out.push_str(&line);
    out.push('\n');
    for row in rows {
        line.clear();
        for i in 0..columns.len() {
            if i > 0 {
                line.push(',');
            }
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
        if c == '"' {
            out.push_str("\"\"");
        } else {
            out.push(c);
        }
    }
    out.push('"');
    out
}

pub fn ident_before(line: &str, end: usize) -> String {
    let b = line.as_bytes();
    let mut start = end;
    while start > 0 && (b[start - 1].is_ascii_alphanumeric() || b[start - 1] == b'_') {
        start -= 1;
    }
    line[start..end].to_string()
}

pub fn format_key_event(key: &KeyEvent) -> String {
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
    format!(
        "key={:?} mods={}{}",
        key.code,
        mods,
        if key.kind == crossterm::event::KeyEventKind::Release {
            " rel"
        } else {
            ""
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drop_is_destructive() {
        assert!(is_destructive("DROP TABLE foo;"));
    }

    #[test]
    fn update_with_where_is_not_destructive() {
        assert!(!is_destructive("UPDATE foo SET bar = 1 WHERE id = 2;"));
    }

    #[test]
    fn update_without_where_is_destructive() {
        assert!(is_destructive("UPDATE foo SET bar = 1;"));
    }

    #[test]
    fn destructive_later_in_script_is_detected() {
        assert!(is_destructive("SELECT 1; DROP TABLE foo;"));
    }

    #[test]
    fn semicolon_in_string_is_not_false_positive() {
        assert!(!is_destructive("SELECT ';';"));
    }
}
