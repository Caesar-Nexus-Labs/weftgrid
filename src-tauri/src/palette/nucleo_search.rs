//! Fuzzy ranking for the command palette via the `nucleo` crate, in-process
//! (P16). cmux reached for an FFI/cdylib only because Swift can't call a Rust
//! crate directly — weftgrid's core IS Rust, so we just `use nucleo::...`.
//!
//! Exposed as the Tauri command `palette_search(query, corpus, boosts)`. The
//! corpus is a flat list of `{id, text, keywords?, rank?}`; `text` is the
//! display title (matched + highlighted) and `keywords` add match weight without
//! contributing highlight spans. `boosts` is an id→f64 map the TS layer fills
//! from usage history (recency + count) so recently-run commands float up.
//!
//! Returns `{id, score, indices}` ranked best-first. Tie-break is
//! score → rank → text → id (stable + deterministic). Matched `indices` are
//! char offsets into `text` for the UI to highlight.

use std::collections::HashMap;

use nucleo::pattern::{CaseMatching, Normalization, Pattern};
use nucleo::{Config, Matcher, Utf32Str};
use serde::{Deserialize, Serialize};

/// One searchable palette entry (built by the TS command registry).
#[derive(Debug, Clone, Deserialize)]
pub struct PaletteCandidate {
    /// Stable command id (also the boost-map key + result id).
    pub id: String,
    /// Display title — the string matched against AND highlighted.
    pub text: String,
    /// Extra match text (keywords/subtitle); contributes score, no highlight.
    #[serde(default)]
    pub keywords: String,
    /// Caller-supplied ordering rank (lower = earlier) used only to break ties.
    #[serde(default)]
    pub rank: i32,
}

/// A ranked match: the candidate id, its final score (incl. boost) and the
/// char offsets in `text` that matched (sorted, de-duped) for highlighting.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PaletteMatch {
    pub id: String,
    pub score: f64,
    pub indices: Vec<u32>,
}

/// Rank `corpus` against `query`, adding any per-id `boosts` to the raw score.
///
/// An empty (or whitespace-only) query returns every candidate ordered by boost
/// then rank/text/id, with no highlight indices — the "initial list" the
/// switcher/commands overlay shows before the user types.
pub fn search(
    query: &str,
    corpus: &[PaletteCandidate],
    boosts: &HashMap<String, f64>,
) -> Vec<PaletteMatch> {
    let trimmed = query.trim();
    let mut matcher = Matcher::new(Config::DEFAULT);

    let mut scored: Vec<Scored> = Vec::with_capacity(corpus.len());

    if trimmed.is_empty() {
        for (index, cand) in corpus.iter().enumerate() {
            scored.push(Scored {
                index,
                score: boost_for(boosts, &cand.id),
                indices: Vec::new(),
            });
        }
    } else {
        let pattern = Pattern::parse(trimmed, CaseMatching::Smart, Normalization::Smart);
        let mut title_buf: Vec<char> = Vec::new();
        let mut kw_buf: Vec<char> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();

        for (index, cand) in corpus.iter().enumerate() {
            indices.clear();
            let title_score = pattern.indices(
                Utf32Str::new(&cand.text, &mut title_buf),
                &mut matcher,
                &mut indices,
            );
            let keyword_score = if cand.keywords.is_empty() {
                None
            } else {
                pattern.score(Utf32Str::new(&cand.keywords, &mut kw_buf), &mut matcher)
            };

            // A candidate matches if its title OR its keywords matched.
            let raw = match (title_score, keyword_score) {
                (Some(t), Some(k)) => t.max(k),
                (Some(t), None) => t,
                (None, Some(k)) => k,
                (None, None) => continue,
            };

            // Highlight spans only ever come from the title match.
            let mut matched = if title_score.is_some() {
                indices.clone()
            } else {
                Vec::new()
            };
            matched.sort_unstable();
            matched.dedup();

            scored.push(Scored {
                index,
                score: f64::from(raw) + boost_for(boosts, &cand.id),
                indices: matched,
            });
        }
    }

    scored.sort_by(|a, b| order(a, b, corpus));
    scored
        .into_iter()
        .map(|s| PaletteMatch {
            id: corpus[s.index].id.clone(),
            score: s.score,
            indices: s.indices,
        })
        .collect()
}

/// Intermediate scored row keyed by corpus index (so tie-break can read rank/text).
struct Scored {
    index: usize,
    score: f64,
    indices: Vec<u32>,
}

fn boost_for(boosts: &HashMap<String, f64>, id: &str) -> f64 {
    boosts.get(id).copied().unwrap_or(0.0)
}

/// Tie-break: score desc → rank asc → text asc → id asc. Deterministic so the
/// same corpus + query always renders in the same order.
fn order(a: &Scored, b: &Scored, corpus: &[PaletteCandidate]) -> std::cmp::Ordering {
    let ca = &corpus[a.index];
    let cb = &corpus[b.index];
    b.score
        .total_cmp(&a.score)
        .then_with(|| ca.rank.cmp(&cb.rank))
        .then_with(|| ca.text.cmp(&cb.text))
        .then_with(|| ca.id.cmp(&cb.id))
}

/// Tauri command: fuzzy-rank `corpus` against `query` with optional `boosts`.
///
/// Registered in `command_registry::register_all` as
/// `palette::nucleo_search::palette_search`.
#[tauri::command]
pub fn palette_search(
    query: String,
    corpus: Vec<PaletteCandidate>,
    boosts: Option<HashMap<String, f64>>,
) -> Vec<PaletteMatch> {
    search(&query, &corpus, &boosts.unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(id: &str, text: &str) -> PaletteCandidate {
        PaletteCandidate {
            id: id.to_string(),
            text: text.to_string(),
            keywords: String::new(),
            rank: 0,
        }
    }

    fn ids(matches: &[PaletteMatch]) -> Vec<&str> {
        matches.iter().map(|m| m.id.as_str()).collect()
    }

    #[test]
    fn ranks_exact_above_prefix_above_contains() {
        let corpus = vec![
            cand("contains", "Open in New Window"),
            cand("prefix", "New Terminal"),
            cand("exact", "New"),
        ];
        let out = search("new", &corpus, &HashMap::new());
        // All three contain the subsequence "new"; nucleo scores exact > prefix > contains.
        assert_eq!(ids(&out), vec!["exact", "prefix", "contains"]);
    }

    #[test]
    fn returns_matched_indices_for_highlight() {
        let corpus = vec![cand("split", "Split Right")];
        let out = search("splt", &corpus, &HashMap::new());
        assert_eq!(out.len(), 1);
        // "splt" matches S,p,l,...,t in "Split Right" → 4 highlight offsets.
        assert_eq!(out[0].indices.len(), 4);
        // Offsets are sorted + within the title length.
        assert!(out[0].indices.windows(2).all(|w| w[0] < w[1]));
        assert!(out[0]
            .indices
            .iter()
            .all(|&i| (i as usize) < "Split Right".len()));
    }

    #[test]
    fn non_matches_are_dropped() {
        let corpus = vec![cand("a", "Split Right"), cand("b", "New Terminal")];
        let out = search("zzz", &corpus, &HashMap::new());
        assert!(out.is_empty());
    }

    #[test]
    fn history_boost_pushes_recency_up() {
        // Two equally-good matches; the boosted id must rank first.
        let corpus = vec![
            cand("first", "Toggle Sidebar"),
            cand("second", "Toggle Statusbar"),
        ];
        let mut boosts = HashMap::new();
        boosts.insert("second".to_string(), 5_000.0);
        let out = search("toggle", &corpus, &boosts);
        assert_eq!(ids(&out), vec!["second", "first"]);
    }

    #[test]
    fn keywords_match_without_polluting_highlight() {
        let mut c = cand("find", "Find in Terminal");
        c.keywords = "search grep".to_string();
        let corpus = vec![c];
        // "grep" only appears in keywords, not the title.
        let out = search("grep", &corpus, &HashMap::new());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "find");
        // No title chars matched → no highlight spans.
        assert!(out[0].indices.is_empty());
    }

    #[test]
    fn empty_query_lists_all_ordered_by_boost_then_rank() {
        let mut a = cand("a", "Alpha");
        a.rank = 2;
        let mut b = cand("b", "Bravo");
        b.rank = 1;
        let corpus = vec![a, b];
        let mut boosts = HashMap::new();
        boosts.insert("a".to_string(), 10.0);
        let out = search("", &corpus, &boosts);
        // "a" boosted above "b" despite higher rank; both present, no indices.
        assert_eq!(ids(&out), vec!["a", "b"]);
        assert!(out.iter().all(|m| m.indices.is_empty()));
    }

    #[test]
    fn tie_break_is_deterministic_by_text_then_id() {
        // Same title text + no boost → identical score; tie-break falls to id.
        let corpus = vec![cand("z", "Reload"), cand("a", "Reload")];
        let out = search("reload", &corpus, &HashMap::new());
        assert_eq!(ids(&out), vec!["a", "z"]);
    }
}
