//! Fuzzy row filtering for the Results pane, backed by `neo_frizbee`
//! (SIMD Smith-Waterman, FZF/FZY-like).
//!
//! `fuzzy_filter_indices(query, rows)` returns the matched rows (best fuzzy
//! score first) paired with the *cells* that matched — used both to pick the
//! displayed subset and to highlight the matched chars. An empty query returns
//! every row (with no matched cells) — so "no filter" and "empty filter input"
//! are the same thing, no special-casing.
//!
//! Matching is **per-cell**: a row passes if ANY single cell contains the full
//! query as a fuzzy subsequence. A match never bridges cells (the prior
//! tab-joined approach let "1e" hit `id` then `email` in one row — fixed).
//! ponytail: 0 typos (fuzzy subsequence, FZF/fzy default) — the filter's job is
//! to narrow, so typo-tolerant substitution would keep matching almost
//! everything on short cells. Bump `max_typos` for looser "did you mean" matching.

use std::cmp::Reverse;
use std::collections::HashMap;

use neo_frizbee::{Config, match_list_indices};

/// The cells of one row that matched a filter query: `(column, byte offsets
/// within that cell's text)`. Offsets ascending.
pub type CellMatches = Vec<(usize, Vec<usize>)>;

/// `(abs_row, matched_cells)` for each row that passes the filter, best row
/// score first. `matched_cells` is `Vec<(col, byte_offsets_within_cell)>` —
/// one entry per cell that independently matched the full query; offsets are
/// ascending (the matcher returns them reversed, we flip them). A row appears
/// if ANY of its cells matched; the row's score is its best cell's score.
/// Empty query → every row with an empty `matched_cells` (nothing to highlight).
pub fn fuzzy_filter_indices(
    query: &str,
    rows: &[Vec<String>],
) -> Vec<(usize, CellMatches)> {
    if query.is_empty() {
        return (0..rows.len()).map(|i| (i, Vec::new())).collect();
    }
    // Flatten all cells into one haystack list; one frizbee call matches the
    // needle against every cell independently. `row_starts` maps a flat cell
    // index back to (row, col) so ragged rows don't break the lookup.
    let mut row_starts: Vec<usize> = Vec::with_capacity(rows.len() + 1);
    let mut flat: Vec<String> = Vec::new();
    for r in rows {
        row_starts.push(flat.len());
        flat.extend(r.iter().cloned());
    }
    row_starts.push(flat.len());
    let cfg = Config {
        max_typos: Some(0),
        ..Config::default()
    };
    let mut by_row: HashMap<usize, CellMatches> = HashMap::new();
    let mut best: HashMap<usize, u16> = HashMap::new();
    for m in match_list_indices(query, &flat, &cfg) {
        let cell_idx = m.index as usize;
        let row = row_starts
            .partition_point(|&s| s <= cell_idx)
            .saturating_sub(1);
        let col = cell_idx - row_starts[row];
        let mut idx = m.indices;
        idx.sort_unstable();
        by_row.entry(row).or_default().push((col, idx));
        best.entry(row)
            .and_modify(|s| *s = (*s).max(m.score))
            .or_insert(m.score);
    }
    let mut out: Vec<(usize, CellMatches)> = by_row.into_iter().collect();
    // Best row score first; tie-break by row index for a stable order.
    out.sort_unstable_by_key(|(r, _)| (Reverse(best.get(r).copied().unwrap_or(0)), *r));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<Vec<String>> {
        vec![
            vec!["1".into(), "john doe".into(), "engineer".into()],
            vec!["2".into(), "jane smith".into(), "manager".into()],
            vec!["3".into(), "alice".into(), "engineer".into()],
            vec!["4".into(), "bob jones".into(), "intern".into()],
        ]
    }

    /// Just the matched row indices from a `fuzzy_filter_indices` result.
    fn idx_of(pairs: Vec<(usize, CellMatches)>) -> Vec<usize> {
        pairs.into_iter().map(|(i, _)| i).collect()
    }

    #[test]
    fn empty_query_returns_all_in_order() {
        let idx = idx_of(fuzzy_filter_indices("", &rows()));
        assert_eq!(idx, vec![0, 1, 2, 3]);
    }

    #[test]
    fn substring_match_in_any_single_cell() {
        // "eng" matches "engineer" in column 2 of rows 0 and 2.
        let idx = idx_of(fuzzy_filter_indices("eng", &rows()));
        assert!(idx.contains(&0) && idx.contains(&2), "engineer rows must match: {idx:?}");
    }

    #[test]
    fn best_score_first() {
        // "j" matches "john" (row 0), "jane" (row 1), "bob jones" (row 3).
        let idx = idx_of(fuzzy_filter_indices("j", &rows()));
        assert!(
            idx.contains(&0) && idx.contains(&1) && idx.contains(&3),
            "j rows must match: {idx:?}"
        );
    }

    #[test]
    fn subsequence_match_skips_chars_within_a_cell() {
        // 0 typos = subsequence match: "jne" matches "jane smith" (row 1) by
        // skipping the 'a' and the space — within the single name cell.
        let idx = idx_of(fuzzy_filter_indices("jne", &rows()));
        assert!(idx.contains(&1), "subsequence 'jne' should match 'jane smith': {idx:?}");
        // an exact typo ("jahn" for "john") does NOT match at 0 typos.
        let typo = idx_of(fuzzy_filter_indices("jahn", &rows()));
        assert!(!typo.contains(&0), "typo should not match at 0 typos: {typo:?}");
    }

    #[test]
    fn no_match_returns_empty() {
        assert!(idx_of(fuzzy_filter_indices("zzzzz", &rows())).is_empty());
    }

    #[test]
    fn match_never_bridges_cells() {
        // "1e" would match row 0 under the old tab-joined approach: '1' in the
        // id cell, 'e' in "engineer". Per-cell, no single cell contains both
        // '1' and 'e' as a subsequence, so row 0 must NOT match.
        let idx = idx_of(fuzzy_filter_indices("1e", &rows()));
        assert!(
            !idx.contains(&0),
            "cross-cell match must be rejected: {idx:?}"
        );
        // "2j" — '2' in id, 'j' in "jane smith" — also rejected.
        let idx2 = idx_of(fuzzy_filter_indices("2j", &rows()));
        assert!(!idx2.contains(&1), "cross-cell '2j' must be rejected: {idx2:?}");
    }

    #[test]
    fn matched_cell_offsets_are_within_the_cell() {
        // "jne" matches "jane smith" in column 1 (the name cell), at byte
        // offsets within that cell: j@0, n@2, e@3.
        let m = fuzzy_filter_indices("jne", &rows());
        let row1 = m.iter().find(|(i, _)| *i == 1).unwrap();
        assert_eq!(row1.1, vec![(1, vec![0, 2, 3])], "got {:?}", row1.1);
    }

    #[test]
    fn ragged_rows_dont_break_index_mapping() {
        // A ragged row (fewer cells) must not shift the next row's cell lookup.
        let rag = vec![
            vec!["x".into()],                       // row 0: 1 cell
            vec!["y".into(), "jane".into()],        // row 1: 2 cells
        ];
        let m = fuzzy_filter_indices("jn", &rag);
        // "jn" matches "jane" in row 1, col 1, offsets 0,2.
        let row1 = m.iter().find(|(i, _)| *i == 1).unwrap();
        assert_eq!(row1.1, vec![(1, vec![0, 2])], "got {:?}", row1.1);
    }
}
