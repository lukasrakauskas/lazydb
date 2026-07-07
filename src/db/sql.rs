//! Tokenizer-aware SQL statement splitting.
//!
//! ponytail: minimal MySQL-aware splitter. Handles quoted strings, comments,
//! and DELIMITER directives. Does not parse full SQL syntax.

/// Split SQL into individual statements on the active delimiter.
///
/// ponytail: supports ';' default, single/double quoted strings with backslash
/// and doubled-quote escapes, `--`/`#` line comments, `/* */` block comments,
/// and MySQL `DELIMITER <tok>` directives. Backtick identifiers are not
/// special-cased.
pub fn split_statements(sql: &str) -> Vec<String> {
    let bytes = sql.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut i = 0;
    let mut delimiter = ";".to_string();
    let mut at_stmt_start = true;

    while i < bytes.len() {
        // Skip whitespace and comments while preserving statement-start state.
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }

        if let Some(next_i) = skip_comment(bytes, i) {
            i = next_i;
            continue;
        }

        // At a real token position, check for DELIMITER directive.
        if at_stmt_start {
            if let Some((new_delim, next_i)) = parse_delimiter(bytes, i) {
                delimiter = new_delim;
                i = next_i;
                start = i; // DELIMITER is a client-side directive; don't emit it
                continue;
            }
        }

        // Skip string literals.
        if bytes[i] == b'\'' || bytes[i] == b'"' {
            i = skip_quoted_string(bytes, i);
            at_stmt_start = false;
            continue;
        }

        // Statement terminator.
        if bytes[i..].starts_with(delimiter.as_bytes()) {
            let stmt = sql[start..i].trim();
            if !stmt.is_empty() {
                out.push(stmt.to_string());
            }
            i += delimiter.len();
            start = i;
            at_stmt_start = true;
            continue;
        }

        // Any other significant character ends the statement-start window.
        at_stmt_start = false;
        i += 1;
    }

    // ponytail: trailing fragment without terminator is returned as a statement.
    let stmt = sql[start..].trim();
    if !stmt.is_empty() {
        out.push(stmt.to_string());
    }
    out
}

fn skip_comment(bytes: &[u8], i: usize) -> Option<usize> {
    // -- line comment
    if bytes[i] == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
        return Some(skip_to_newline(bytes, i));
    }
    // # line comment
    if bytes[i] == b'#' {
        return Some(skip_to_newline(bytes, i));
    }
    // /* */ block comment
    if bytes[i] == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'*' {
        return Some(skip_block_comment(bytes, i));
    }
    None
}

fn skip_to_newline(bytes: &[u8], i: usize) -> usize {
    let mut j = i;
    while j < bytes.len() && bytes[j] != b'\n' {
        j += 1;
    }
    j
}

fn skip_block_comment(bytes: &[u8], i: usize) -> usize {
    let mut j = i + 2;
    while j + 1 < bytes.len() {
        if bytes[j] == b'*' && bytes[j + 1] == b'/' {
            return j + 2;
        }
        j += 1;
    }
    // ponytail: unterminated block comment → consume to EOF; upgrade: error.
    bytes.len()
}

fn skip_quoted_string(bytes: &[u8], i: usize) -> usize {
    let quote = bytes[i];
    let mut j = i + 1;
    while j < bytes.len() {
        if bytes[j] == b'\\' && j + 1 < bytes.len() {
            // ponytail: backslash escape; doubled-quote escape is handled below.
            j += 2;
        } else if bytes[j] == quote {
            if j + 1 < bytes.len() && bytes[j + 1] == quote {
                // Doubled quote escape (SQL standard).
                j += 2;
            } else {
                return j + 1;
            }
        } else {
            j += 1;
        }
    }
    // ponytail: unterminated string → consume to EOF; upgrade: error.
    bytes.len()
}

fn parse_delimiter(bytes: &[u8], i: usize) -> Option<(String, usize)> {
    if i + 9 > bytes.len() {
        return None;
    }
    if !bytes[i..i + 9].eq_ignore_ascii_case(b"DELIMITER") {
        return None;
    }
    let mut j = i + 9;
    // Must be followed by whitespace.
    if j >= bytes.len() || !bytes[j].is_ascii_whitespace() {
        return None;
    }
    // Skip whitespace to find the delimiter token.
    while j < bytes.len() && bytes[j].is_ascii_whitespace() {
        j += 1;
    }
    if j >= bytes.len() {
        return None;
    }
    let start = j;
    while j < bytes.len() && !bytes[j].is_ascii_whitespace() {
        j += 1;
    }
    let new_delim = String::from_utf8_lossy(&bytes[start..j]).to_string();
    if new_delim.is_empty() {
        return None;
    }
    Some((new_delim, j))
}

#[cfg(test)]
mod tests {
    use super::split_statements;

    #[test]
    fn semicolon_inside_string() {
        assert_eq!(
            split_statements("SELECT 'a;b'; SELECT 2"),
            vec!["SELECT 'a;b'", "SELECT 2"]
        );
    }

    #[test]
    fn double_quoted_string() {
        assert_eq!(
            split_statements("SELECT \"a;b\"; SELECT 2"),
            vec!["SELECT \"a;b\"", "SELECT 2"]
        );
    }

    #[test]
    fn dash_line_comment_with_semicolon() {
        assert_eq!(
            split_statements("SELECT 1 -- comment with ; \n; SELECT 2"),
            vec!["SELECT 1 -- comment with ;", "SELECT 2"]
        );
    }

    #[test]
    fn hash_line_comment_with_semicolon() {
        assert_eq!(
            split_statements("SELECT 1 # comment with ; \n; SELECT 2"),
            vec!["SELECT 1 # comment with ;", "SELECT 2"]
        );
    }

    #[test]
    fn block_comment_with_semicolon() {
        assert_eq!(
            split_statements("SELECT 1 /* comment with ; */ ; SELECT 2"),
            vec!["SELECT 1 /* comment with ; */", "SELECT 2"]
        );
    }

    #[test]
    fn delimiter_change() {
        let sql = "DELIMITER // SELECT 1 // DELIMITER ; SELECT 2;";
        assert_eq!(split_statements(sql), vec!["SELECT 1", "SELECT 2"]);
    }

    #[test]
    fn trailing_statement_without_semicolon() {
        assert_eq!(
            split_statements("SELECT 1; SELECT 2"),
            vec!["SELECT 1", "SELECT 2"]
        );
    }

    #[test]
    fn escaped_quote_in_string() {
        assert_eq!(
            split_statements("SELECT 'a\\';b'; SELECT 2"),
            vec!["SELECT 'a\\';b'", "SELECT 2"]
        );
    }
}
