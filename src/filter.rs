//! Fuzzy row filtering for the Results pane, backed by `neo_frizbee`
//! (SIMD Smith-Waterman, FZF/FZY-like).
//!
//! `fuzzy_filter_indices(query, rows)` returns the matched rows (best fuzzy
//! score first) paired with the byte offsets where the needle matched — used
//! both to pick the displayed subset and to highlight the matched chars.
//! `split_cell_offsets` partitions those offsets per cell for the renderer.
//! An empty query returns every row (with no offsets) — so "no filter" and
//! "empty filter input" are the same thing, no special-casing.
//!
//! Each row is matched against the tab-joined cell text, so a query can hit any
//! column ("jo" matches a row with `john` in any cell). ponytail: 0 typos
//! (fuzzy subsequence, FZF/fzy default) — the filter's job is to narrow, so
//! typo-tolerant substitution would keep matching almost everything on short
//! cells. Bump `max_typos` if you want looser "did you mean" matching.

use neo_frizbee::{Config, match_list_indices};

/// Absolute indices of rows matching `query` (best score first), each
/// paired with the byte offsets in the row's tab-joined text where the
/// needle matched — so the renderer can highlight the matched chars (FZF-style
/// glow). Offsets are ascending (the matcher returns them in reverse, we flip
/// them). Empty query → every row with no offsets (nothing to highlight).
pub fn fuzzy_filter_indices(query: &str, rows: &[Vec<String>]) -> Vec<(usize, Vec<usize>)> {
    if query.is_empty() {
        return (0..rows.len()).map(|i| (i, Vec::new())).collect();
    }
    let joined: Vec<String> = rows.iter().map(|r| r.join("\t")).collect();
    let cfg = Config {
        max_typos: Some(0),
        ..Config::default()
    };
    match_list_indices(query, &joined, &cfg)
        .into_iter()
        .map(|m| {
            let mut idx = m.indices;
            idx.sort_unstable();
            (m.index as usize, idx)
        })
        .collect()
}

/// Split byte offsets in a row's tab-joined text into per-cell byte offsets.
// Returns one `Vec<usize>` per cell (ascending). Offsets landing on a tab
// separator (needle containing a literal tab — impossible from keyboard
// input) are dropped. ponytail: O(cells × hits) per row; result sets are small.
pub fn split_cell_offsets(joined_offsets: &[usize], cells: &[String]) -> Vec<Vec<usize>> {
    let mut per_cell = vec![Vec::new(); cells.len()];
    let mut start = 0usize;
    let cell_ranges: Vec<(usize, usize)> = cells
        .iter()
        .map(|c| {
            let r = (start, start + c.len());
            start += c.len() + 1; // +1 for the tab separator
            r
        })
        .collect();
    for &o in joined_offsets {
        for (k, &(s, e)) in cell_ranges.iter().enumerate() {
            if o >= s && o < e {
                per_cell[k].push(o - s);
                break;
            }
        }
    }
    per_cell
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
    fn idx_of(pairs: Vec<(usize, Vec<usize>)>) -> Vec<usize> {
        pairs.into_iter().map(|(i, _)| i).collect()
    }

    #[test]
    fn empty_query_returns_all_in_order() {
        let idx = idx_of(fuzzy_filter_indices("", &rows()));
        assert_eq!(idx, vec![0, 1, 2, 3]);
    }

    #[test]
    fn substring_match_across_any_column() {
        // "eng" matches column 2 of rows 0 and 2 (both "engineer"). Fuzzy
        // matching is permissive (out-of-order chars), so other rows may also
        // match weakly — assert the must-haves, not an exact count.
        let idx = idx_of(fuzzy_filter_indices("eng", &rows()));
        assert!(idx.contains(&0) && idx.contains(&2), "engineer rows must match: {idx:?}");
    }

    #[test]
    fn best_score_first() {
        // "j" matches "john" (row 0), "jane" (row 1), "bob jones" (row 3).
        // Fuzzy matching may also pick up weak out-of-order hits; assert the
        // three must-match rows are present.
        let idx = idx_of(fuzzy_filter_indices("j", &rows()));
        assert!(idx.contains(&0) && idx.contains(&1) && idx.contains(&3), "j rows must match: {idx:?}");
    }

    #[test]
    fn subsequence_match_skips_chars() {
        // 0 typos = subsequence match: "jne" matches "jane smith" (row 1) by
        // skipping the 'a' and spaces — the FZF/fzy default behavior.
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
    fn filter_indices_returns_byte_offsets_ascending() {
        // "jne" on "jane smith" (row 1, joined "2\tjane smith\tmanager")
        // matches j,n,e in "jane" (joined-byte 2,4,5).
        let m = fuzzy_filter_indices("jne", &rows());
        let row1 = m.iter().find(|(i, _)| *i == 1).unwrap();
        // ascending, no reversals.
        assert_eq!(row1.1, vec![2, 4, 5], "got {:?}", row1.1);
    }

    #[test]
    fn split_cell_offsets_partitions_across_cells() {
        // row 1 = ["2", "jane smith", "manager"], joined = "2\tjane smith\tmanager".
        // byte layout:  0='2' 1=\t 2='j' 3='a' 4='n' 5='e' 6=' ' 7='s'...  12=\t 13='m'...
        // offsets [2, 4] → cell 0: none, cell 1: [0, 2] (j, n within "jane smith"), cell 2: none.
        let cells = vec!["2".to_string(), "jane smith".to_string(), "manager".to_string()];
        let per = split_cell_offsets(&[2, 4], &cells);
        assert_eq!(per, vec![vec![], vec![0, 2], vec![]]);
        // an offset on the tab separator (1) is dropped — no cell claim.
        let per2 = split_cell_offsets(&[1, 13], &cells);
        assert_eq!(per2, vec![vec![], vec![], vec![0]], "tab offset dropped, 'm' at cell2 offset 0");
    }
}
