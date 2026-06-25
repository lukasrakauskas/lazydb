// ponytail: keyword + common-function completion. Schema-name completion is a
// deliberate deferral — add a `Vec<String>` of table/column names fetched from
// INFORMATION_SCHEMA on connect and merge them into `completions()` when needed.

use crate::highlight::KEYWORDS;

/// Extra function names offered as completions (conventionally called with `(`).
const FUNCTIONS: &[&str] = &[
    "COUNT", "SUM", "AVG", "MIN", "MAX", "CONCAT", "CONCAT_WS", "COALESCE", "IFNULL",
    "NULLIF", "LENGTH", "CHAR_LENGTH", "CHARACTER_LENGTH", "SUBSTRING", "SUBSTR",
    "TRIM", "LTRIM", "RTRIM", "LOWER", "LCASE", "UPPER", "UCASE", "REPLACE", "INSERT",
    "LEFT", "RIGHT", "MID", "LPAD", "RPAD", "REPEAT", "REVERSE", "SPACE", "FIELD",
    "ROUND", "CEIL", "CEILING", "FLOOR", "ABS", "SIGN", "POW", "POWER", "SQRT", "MOD",
    "RAND", "GREATEST", "LEAST", "NOW", "SYSDATE", "CURDATE", "CURRENT_DATE",
    "CURTIME", "CURRENT_TIME", "UTC_TIMESTAMP", "UTC_DATE", "UTC_TIME", "DATE", "TIME",
    "YEAR", "MONTH", "MONTHNAME", "DAY", "DAYNAME", "DAYOFWEEK", "DAYOFMONTH",
    "DAYOFYEAR", "HOUR", "MINUTE", "SECOND", "MICROSECOND", "QUARTER", "WEEK",
    "DATE_FORMAT", "STR_TO_DATE", "DATE_ADD", "DATE_SUB", "DATEDIFF", "TIMEDIFF",
    "UNIX_TIMESTAMP", "FROM_UNIXTIME", "IF", "CASE", "CAST", "CONVERT", "VERSION",
    "DATABASE", "SCHEMA", "USER", "CURRENT_USER", "CONNECTION_ID", "LAST_INSERT_ID",
    "ROW_NUMBER", "RANK", "DENSE_RANK", "FOUND_ROWS", "ROW_COUNT",
];

/// Return completions whose prefix matches `word` (case-insensitively),
/// excluding the word itself. Result is sorted + deduped, uppercased.
pub fn completions(word: &str) -> Vec<String> {
    if word.is_empty() {
        return Vec::new();
    }
    let lw = word.to_ascii_lowercase();
    let mut out: Vec<String> = Vec::new();
    for &cand in KEYWORDS.iter().chain(FUNCTIONS.iter()) {
        let lc = cand.to_ascii_lowercase();
        if lc.starts_with(&lw) && lc != lw {
            out.push(cand.to_string());
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefix_filter_excludes_self() {
        let r = completions("se");
        assert!(r.contains(&"SELECT".to_string()));
        assert!(r.contains(&"SET".to_string()));
        assert!(!r.iter().any(|s| s.eq_ignore_ascii_case("se")));
    }

    #[test]
    fn empty_word_no_completions() {
        assert!(completions("").is_empty());
    }

    #[test]
    fn no_match_returns_empty() {
        assert!(completions("zzz").is_empty());
    }
}
