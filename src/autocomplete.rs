// ponytail: keyword + function + schema-name completion. Schema names come from
// the DB (table/column lists fetched on connect, see app::Job::Schema). Context
// (dot vs general position) is decided by the caller, which passes the right
// pools; this module just merges + prefix-filters.

use crate::highlight::{KEYWORDS, is_keyword};

/// Extra function names offered as completions (conventionally called with `(`).
const FUNCTIONS: &[&str] = &[
    "COUNT",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
    "CONCAT",
    "CONCAT_WS",
    "COALESCE",
    "IFNULL",
    "NULLIF",
    "LENGTH",
    "CHAR_LENGTH",
    "CHARACTER_LENGTH",
    "SUBSTRING",
    "SUBSTR",
    "TRIM",
    "LTRIM",
    "RTRIM",
    "LOWER",
    "LCASE",
    "UPPER",
    "UCASE",
    "REPLACE",
    "INSERT",
    "LEFT",
    "RIGHT",
    "MID",
    "LPAD",
    "RPAD",
    "REPEAT",
    "REVERSE",
    "SPACE",
    "FIELD",
    "ROUND",
    "CEIL",
    "CEILING",
    "FLOOR",
    "ABS",
    "SIGN",
    "POW",
    "POWER",
    "SQRT",
    "MOD",
    "RAND",
    "GREATEST",
    "LEAST",
    "NOW",
    "SYSDATE",
    "CURDATE",
    "CURRENT_DATE",
    "CURTIME",
    "CURRENT_TIME",
    "UTC_TIMESTAMP",
    "UTC_DATE",
    "UTC_TIME",
    "DATE",
    "TIME",
    "YEAR",
    "MONTH",
    "MONTHNAME",
    "DAY",
    "DAYNAME",
    "DAYOFWEEK",
    "DAYOFMONTH",
    "DAYOFYEAR",
    "HOUR",
    "MINUTE",
    "SECOND",
    "MICROSECOND",
    "QUARTER",
    "WEEK",
    "DATE_FORMAT",
    "STR_TO_DATE",
    "DATE_ADD",
    "DATE_SUB",
    "DATEDIFF",
    "TIMEDIFF",
    "UNIX_TIMESTAMP",
    "FROM_UNIXTIME",
    "IF",
    "CASE",
    "CAST",
    "CONVERT",
    "VERSION",
    "DATABASE",
    "SCHEMA",
    "USER",
    "CURRENT_USER",
    "CONNECTION_ID",
    "LAST_INSERT_ID",
    "ROW_NUMBER",
    "RANK",
    "DENSE_RANK",
    "FOUND_ROWS",
    "ROW_COUNT",
];

/// Merge keywords + functions + the two schema pools, keep those whose
/// case-insensitive prefix matches `word` (excluding the word itself), sort
/// case-insensitively, dedup. Schema names keep their original case.
pub fn completions(word: &str, tables: &[String], columns: &[String]) -> Vec<String> {
    if word.is_empty() {
        return Vec::new();
    }
    let lw = word.to_ascii_lowercase();
    let mut out: Vec<String> = Vec::new();
    let mut push = |cand: &str| {
        let lc = cand.to_ascii_lowercase();
        if lc.starts_with(&lw) && lc != lw {
            out.push(cand.to_string());
        }
    };
    for &cand in KEYWORDS.iter().chain(FUNCTIONS.iter()) {
        push(cand);
    }
    for cand in tables.iter().chain(columns.iter()) {
        push(cand);
    }
    out.sort_by_key(|a| a.to_ascii_lowercase());
    out.dedup();
    out
}

/// Table names referenced in FROM/JOIN/INTO/UPDATE clauses of `stmt`.
/// ponytail: naive token scan — aliases (e.g. `FROM users u`) are collected too
/// but harmlessly dropped later: the caller looks each one up in the schema map,
/// and `u` won't match a real table. No alias→table resolution; upgrade when
/// `alias.col` completion needs it.
pub fn referenced_tables(stmt: &str) -> Vec<String> {
    const TABLE_KW: &[&str] = &["FROM", "JOIN", "INTO", "UPDATE"];
    let b = stmt.as_bytes();
    let n = b.len();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0usize;
    let mut expecting = false;
    while i < n {
        let c = b[i];
        if c.is_ascii_whitespace() || c == b',' {
            i += 1;
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            // identifier or keyword; dots allowed for schema-qualified names.
            let mut j = i;
            while j < n && (b[j].is_ascii_alphanumeric() || b[j] == b'_' || b[j] == b'.') {
                j += 1;
            }
            let word = &stmt[i..j];
            if is_keyword(word) {
                expecting = TABLE_KW.iter().any(|k| word.eq_ignore_ascii_case(k));
            } else if expecting {
                // take the part after the last dot (schema.table → table)
                let table = word.rsplit('.').next().unwrap_or(word).to_string();
                out.push(table);
            }
            i = j;
            continue;
        }
        // any other punctuation ends a table list
        expecting = false;
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_filter_excludes_self() {
        let r = completions("se", &[], &[]);
        assert!(r.contains(&"SELECT".to_string()));
        assert!(r.contains(&"SET".to_string()));
        assert!(!r.iter().any(|s| s.eq_ignore_ascii_case("se")));
    }

    #[test]
    fn empty_word_no_completions() {
        assert!(completions("", &[], &[]).is_empty());
    }

    #[test]
    fn no_match_returns_empty() {
        assert!(completions("zzz", &[], &[]).is_empty());
    }

    #[test]
    fn schema_names_keep_case_and_filter() {
        let tables = vec!["users".to_string(), "UserOrders".to_string()];
        let r = completions("use", &tables, &[]);
        assert!(r.contains(&"users".to_string()));
        assert!(r.contains(&"UserOrders".to_string()));
        // original case preserved
        assert!(r.iter().any(|s| s == "UserOrders"));
    }

    #[test]
    fn referenced_tables_basic() {
        let t = referenced_tables("SELECT * FROM users WHERE 1");
        assert_eq!(t, vec!["users".to_string()]);
    }

    #[test]
    fn referenced_tables_join_and_list() {
        let t = referenced_tables("SELECT * FROM users u JOIN posts p ON 1");
        // aliases `u` and `p` are collected too — harmless (no schema match).
        assert!(t.contains(&"users".to_string()));
        assert!(t.contains(&"posts".to_string()));
    }

    #[test]
    fn referenced_tables_schema_qualified() {
        let t = referenced_tables("SELECT * FROM db.users");
        assert_eq!(t, vec!["users".to_string()]);
    }
}
