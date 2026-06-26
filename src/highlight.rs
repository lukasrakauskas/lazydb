// ponytail: hand-rolled SQL tokenizer → ratatui Spans, one line at a time.
// No deps. Covers keywords, functions (identifier followed by `(`), strings,
// quoted identifiers, numbers, and `--` / `#` / `/* */` comments. Block-comment
// state is carried across lines via the `in_block` flag.
// ASCII-only scanning (the editor itself is byte-indexed ASCII); non-ASCII
// identifiers fall through as plain text, which is correct for highlighting.

use ratatui::text::Span;

use crate::theme;

/// SQL keywords + common types, uppercased. Matched case-insensitively.
pub const KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "JOIN", "INNER", "LEFT", "RIGHT", "FULL", "OUTER", "CROSS",
    "ON", "USING", "GROUP", "BY", "ORDER", "ASC", "DESC", "HAVING", "LIMIT", "OFFSET",
    "INSERT", "INTO", "VALUES", "UPDATE", "SET", "DELETE", "CREATE", "TABLE", "ALTER",
    "DROP", "INDEX", "VIEW", "DATABASE", "SCHEMA", "IF", "EXISTS", "NOT", "NULL", "IS",
    "AS", "DISTINCT", "UNION", "ALL", "AND", "OR", "IN", "BETWEEN", "LIKE", "ILIKE",
    "CASE", "WHEN", "THEN", "ELSE", "END", "WITH", "RECURSIVE", "RETURNING",
    "PRIMARY", "KEY", "FOREIGN", "REFERENCES", "UNIQUE", "DEFAULT", "CHECK",
    "CONSTRAINT", "ADD", "COLUMN", "RENAME", "TO", "TRUNCATE", "BEGIN", "COMMIT",
    "ROLLBACK", "TRANSACTION", "EXPLAIN", "DESCRIBE", "SHOW", "USE", "GRANT", "REVOKE",
    "AUTO_INCREMENT", "UNSIGNED", "ZEROFILL", "INT", "INTEGER", "BIGINT", "SMALLINT",
    "TINYINT", "MEDIUMINT", "VARCHAR", "CHAR", "TEXT", "TINYTEXT", "MEDIUMTEXT",
    "LONGTEXT", "BLOB", "TINYBLOB", "MEDIUMBLOB", "LONGBLOB", "DATE", "DATETIME",
    "TIMESTAMP", "TIME", "YEAR", "DECIMAL", "NUMERIC", "FLOAT", "DOUBLE", "REAL",
    "BOOLEAN", "BOOL", "JSON", "ENUM", "SET", "BINARY", "VARBINARY", "SERIAL",
];

pub fn is_keyword(w: &str) -> bool {
    KEYWORDS.iter().any(|k| k.eq_ignore_ascii_case(w))
}

/// Highlight one line, carrying block-comment state in `in_block`.
/// Returns owned spans (`Span<'static>`).
pub fn highlight_line(line: &str, in_block: &mut bool) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let b = line.as_bytes();
    let n = b.len();
    let mut i = 0usize;
    let mut plain = String::new();

    macro_rules! flush {
        () => {
            if !plain.is_empty() {
                spans.push(Span::raw(std::mem::take(&mut plain)));
            }
        };
    }

    // Continuation of a multi-line block comment from a previous line.
    if *in_block {
        match line.find("*/") {
            Some(pos) => {
                spans.push(Span::styled(line[..pos + 2].to_string(), theme::SQL_COMMENT));
                i = pos + 2;
                *in_block = false;
            }
            None => {
                spans.push(Span::styled(line.to_string(), theme::SQL_COMMENT));
                return spans;
            }
        }
    }

    while i < n {
        let c = b[i];

        // Line comments: `--` and MySQL `#`.
        if c == b'#' || (c == b'-' && i + 1 < n && b[i + 1] == b'-') {
            flush!();
            spans.push(Span::styled(line[i..].to_string(), theme::SQL_COMMENT));
            return spans;
        }
        // Block comment start.
        if c == b'/' && i + 1 < n && b[i + 1] == b'*' {
            flush!();
            match line[i + 2..].find("*/") {
                Some(pos) => {
                    let end = i + 2 + pos + 2;
                    spans.push(Span::styled(line[i..end].to_string(), theme::SQL_COMMENT));
                    i = end;
                }
                None => {
                    spans.push(Span::styled(line[i..].to_string(), theme::SQL_COMMENT));
                    *in_block = true;
                    return spans;
                }
            }
            continue;
        }
        // Strings / quoted identifiers: `'...'`, `"..."`, `` `...` ``.
        if c == b'\'' || c == b'"' || c == b'`' {
            flush!();
            let quote = c;
            let mut j = i + 1;
            while j < n {
                if b[j] == quote {
                    if j + 1 < n && b[j + 1] == quote {
                        j += 2; // doubled quote escape
                        continue;
                    }
                    j += 1;
                    break;
                }
                j += 1;
            }
            let end = j.min(n);
            spans.push(Span::styled(line[i..end].to_string(), theme::SQL_STRING));
            i = end;
            continue;
        }
        // Numbers.
        if c.is_ascii_digit() || (c == b'.' && i + 1 < n && b[i + 1].is_ascii_digit()) {
            flush!();
            let mut j = i;
            while j < n && (b[j].is_ascii_digit() || b[j] == b'.') {
                j += 1;
            }
            spans.push(Span::styled(line[i..j].to_string(), theme::SQL_NUMBER));
            i = j;
            continue;
        }
        // Identifiers / keywords / functions.
        if c.is_ascii_alphabetic() || c == b'_' {
            flush!();
            let mut j = i;
            while j < n && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                j += 1;
            }
            let word = &line[i..j];
            if is_keyword(word) {
                spans.push(Span::styled(word.to_string(), theme::SQL_KEYWORD));
                i = j;
                continue;
            }
            // Function lookahead: identifier followed (after spaces) by `(`.
            let mut k = j;
            while k < n && b[k] == b' ' {
                k += 1;
            }
            if k < n && b[k] == b'(' {
                spans.push(Span::styled(word.to_string(), theme::SQL_FUNCTION));
            } else {
                // Plain identifier — keep as default text to avoid extra spans.
                plain.push_str(word);
            }
            i = j;
            continue;
        }

        // Anything else (operators, spaces, punctuation): batch as plain.
        let ch = line[i..].chars().next().unwrap();
        plain.push(ch);
        i += ch.len_utf8();
    }
    flush!();
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyword_string_number_comment() {
        let mut blk = false;
        let s = highlight_line("SELECT 'x', 42 -- done", &mut blk);
        assert!(!blk);
        // 5 spans expected: SELECT, " ", 'x', ", ", 42, " ", -- done
        // (plain whitespace/operators are coalesced into raw spans)
        let joined: String = s.iter().map(|sp| sp.content.as_ref()).collect();
        assert_eq!(joined, "SELECT 'x', 42 -- done");
        // Keyword span is styled (non-default), string span too.
        assert_eq!(s[0].style, theme::SQL_KEYWORD);
        assert_eq!(s[2].style, theme::SQL_STRING);
    }

    #[test]
    fn function_vs_identifier() {
        let mut blk = false;
        let s = highlight_line("count(*) from t", &mut blk);
        // `count` followed by `(` → FUNCTION style; `from` → KEYWORD; `t` plain.
        let count = s.iter().find(|sp| sp.content.as_ref() == "count").unwrap();
        assert_eq!(count.style, theme::SQL_FUNCTION);
        let from = s.iter().find(|sp| sp.content.as_ref() == "from").unwrap();
        assert_eq!(from.style, theme::SQL_KEYWORD);
    }

    #[test]
    fn block_comment_spans_lines() {
        let mut blk = false;
        let s1 = highlight_line("/* open", &mut blk);
        assert!(blk, "block comment should carry over");
        assert_eq!(s1.len(), 1);
        assert_eq!(s1[0].style, theme::SQL_COMMENT);

        let s2 = highlight_line("still inside */ select", &mut blk);
        assert!(!blk, "block comment should close");
        // First span is the comment up to */, last span is keyword select.
        assert_eq!(s2.first().unwrap().style, theme::SQL_COMMENT);
        let sel = s2.iter().find(|sp| sp.content.as_ref() == "select").unwrap();
        assert_eq!(sel.style, theme::SQL_KEYWORD);
    }
}
