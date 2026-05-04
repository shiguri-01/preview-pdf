use std::sync::Arc;

use crate::backend::{TextGlyph, TextPage};
use crate::command::SearchMatcherKind;
use crate::highlight::geometry::merge_text_glyph_rects;

use super::engine::SearchOccurrence;

pub trait SearchMatcher: Send + Sync {
    fn prepare_query(&self, raw_query: &str) -> String;
    fn matches_page(&self, page_text: &str, prepared_query: &str) -> bool;
    fn locate_text_matches(&self, page_text: &str, prepared_query: &str) -> Vec<SearchOccurrence>;
    fn locate_matches(&self, page: &TextPage, prepared_query: &str) -> Vec<SearchOccurrence>;
}

pub fn matcher_for_kind(kind: SearchMatcherKind) -> Arc<dyn SearchMatcher> {
    Arc::new(ContainsMatcher {
        case_sensitive: kind == SearchMatcherKind::ContainsSensitive,
    })
}

#[derive(Debug)]
struct ContainsMatcher {
    case_sensitive: bool,
}

impl SearchMatcher for ContainsMatcher {
    fn prepare_query(&self, raw_query: &str) -> String {
        prepare_contains_query(raw_query, self.case_sensitive)
    }

    fn matches_page(&self, page_text: &str, prepared_query: &str) -> bool {
        page_matches_contains(page_text, prepared_query, self.case_sensitive)
    }

    fn locate_text_matches(&self, page_text: &str, prepared_query: &str) -> Vec<SearchOccurrence> {
        locate_text_occurrences(page_text, prepared_query, self.case_sensitive)
    }

    fn locate_matches(&self, page: &TextPage, prepared_query: &str) -> Vec<SearchOccurrence> {
        locate_occurrences(&page.glyphs, prepared_query, self.case_sensitive)
    }
}

pub(crate) fn prepare_contains_query(raw_query: &str, case_sensitive: bool) -> String {
    normalize_text_for_search(raw_query, case_sensitive, false)
}

pub(crate) fn page_matches_contains(
    page_text: &str,
    prepared_query: &str,
    case_sensitive: bool,
) -> bool {
    let prepared_page = normalize_text_for_search(page_text, case_sensitive, false);
    if prepared_page.contains(prepared_query) {
        return true;
    }

    let whitespace_insensitive_page = normalize_text_for_search(page_text, case_sensitive, true);
    let whitespace_insensitive_query = normalize_text_for_search(prepared_query, true, true);
    whitespace_insensitive_page.contains(&whitespace_insensitive_query)
}

pub(crate) fn locate_occurrences(
    glyphs: &[TextGlyph],
    prepared_query: &str,
    case_sensitive: bool,
) -> Vec<SearchOccurrence> {
    let occurrences =
        locate_occurrences_with_strategy(glyphs, prepared_query, case_sensitive, false);
    if !occurrences.is_empty() {
        return occurrences;
    }

    locate_occurrences_with_strategy(glyphs, prepared_query, case_sensitive, true)
}

pub(crate) fn locate_text_occurrences(
    text: &str,
    prepared_query: &str,
    case_sensitive: bool,
) -> Vec<SearchOccurrence> {
    let occurrences =
        locate_text_occurrences_with_strategy(text, prepared_query, case_sensitive, false);
    if !occurrences.is_empty() {
        return occurrences;
    }

    locate_text_occurrences_with_strategy(text, prepared_query, case_sensitive, true)
}

pub(crate) fn occurrence_highlight_unavailable(
    occurrence: &SearchOccurrence,
    glyphs: &[TextGlyph],
) -> bool {
    if occurrence.rects.is_empty() {
        return true;
    }

    let Some(slice) = glyphs.get(occurrence.match_start..=occurrence.match_end) else {
        return true;
    };

    slice
        .iter()
        .any(|glyph| !glyph.ch.is_whitespace() && glyph.bbox.is_none())
}

pub(crate) fn apply_hit_snippet(occurrence: &mut SearchOccurrence, glyphs: &[TextGlyph]) {
    let snippet = build_hit_snippet(glyphs, occurrence.match_start, occurrence.match_end);
    occurrence.snippet = snippet.text;
    occurrence.snippet_match_start = snippet.match_start;
    occurrence.snippet_match_end = snippet.match_end;
}

struct SnippetPresentation {
    text: String,
    match_start: Option<usize>,
    match_end: Option<usize>,
}

fn build_hit_snippet(
    glyphs: &[TextGlyph],
    match_start: usize,
    match_end: usize,
) -> SnippetPresentation {
    const CONTEXT_CHARS: usize = 16;

    if glyphs.is_empty() || match_start >= glyphs.len() || match_end < match_start {
        return SnippetPresentation {
            text: String::new(),
            match_start: None,
            match_end: None,
        };
    }

    let match_end = match_end.min(glyphs.len() - 1);
    let context_start = match_start.saturating_sub(CONTEXT_CHARS);
    let context_end = match_end
        .saturating_add(CONTEXT_CHARS)
        .saturating_add(1)
        .min(glyphs.len());

    let before = glyphs[context_start..match_start]
        .iter()
        .map(|glyph| glyph.ch)
        .collect::<String>();
    let matched = glyphs[match_start..=match_end]
        .iter()
        .map(|glyph| glyph.ch)
        .collect::<String>();
    let after = glyphs[match_end + 1..context_end]
        .iter()
        .map(|glyph| glyph.ch)
        .collect::<String>();

    build_snippet_text(
        before,
        matched,
        after,
        context_start > 0,
        context_end < glyphs.len(),
    )
}

fn build_text_hit_snippet(text: &str, char_start: usize, char_end: usize) -> SnippetPresentation {
    const CONTEXT_CHARS: usize = 16;

    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() || char_start >= chars.len() || char_end < char_start {
        return SnippetPresentation {
            text: String::new(),
            match_start: None,
            match_end: None,
        };
    }

    let char_end = char_end.min(chars.len() - 1);
    let context_start = char_start.saturating_sub(CONTEXT_CHARS);
    let context_end = char_end
        .saturating_add(CONTEXT_CHARS)
        .saturating_add(1)
        .min(chars.len());

    let before = chars[context_start..char_start].iter().collect::<String>();
    let matched = chars[char_start..=char_end].iter().collect::<String>();
    let after = chars[char_end + 1..context_end].iter().collect::<String>();

    build_snippet_text(
        before,
        matched,
        after,
        context_start > 0,
        context_end < chars.len(),
    )
}

fn build_snippet_text(
    before: String,
    matched: String,
    after: String,
    has_prefix: bool,
    has_suffix: bool,
) -> SnippetPresentation {
    let mut snippet = String::new();
    let mut match_start = None;
    let mut match_end = None;
    if has_prefix {
        snippet.push('…');
    }
    snippet.push_str(&before);
    if !matched.is_empty() {
        match_start = Some(snippet.len());
    }
    snippet.push_str(&matched);
    if !matched.is_empty() {
        match_end = Some(snippet.len());
    }
    snippet.push_str(&after);
    if has_suffix {
        snippet.push('…');
    }

    SnippetPresentation {
        text: snippet,
        match_start,
        match_end,
    }
}

fn locate_text_occurrences_with_strategy(
    text: &str,
    prepared_query: &str,
    case_sensitive: bool,
    ignore_whitespace: bool,
) -> Vec<SearchOccurrence> {
    if prepared_query.is_empty() {
        return Vec::new();
    }

    let (search_text, char_map) =
        normalize_text_with_char_map(text, case_sensitive, ignore_whitespace);
    if search_text.is_empty() {
        return Vec::new();
    }

    let query_text = normalize_text_for_search(prepared_query, true, ignore_whitespace);
    if query_text.is_empty() || query_text.len() > search_text.len() {
        return Vec::new();
    }

    let char_byte_offsets: Vec<usize> = search_text
        .char_indices()
        .map(|(offset, _)| offset)
        .collect();
    let query_char_len = query_text.chars().count();
    if query_char_len == 0 || query_char_len > char_map.len() {
        return Vec::new();
    }

    let mut occurrences = Vec::new();
    let mut cursor_byte = 0;
    while cursor_byte <= search_text.len() {
        let Some(relative_match_byte) = search_text[cursor_byte..].find(&query_text) else {
            break;
        };
        let match_byte = cursor_byte + relative_match_byte;
        let match_char_start = char_byte_offsets
            .binary_search(&match_byte)
            .expect("str::find returned a non-character-boundary offset");
        let char_start = char_map[match_char_start];
        let char_end = char_map[match_char_start + query_char_len - 1];
        let snippet = build_text_hit_snippet(text, char_start, char_end);
        occurrences.push(SearchOccurrence {
            match_start: char_start,
            match_end: char_end,
            rects: Vec::new(),
            snippet: snippet.text,
            snippet_match_start: snippet.match_start,
            snippet_match_end: snippet.match_end,
        });
        cursor_byte = match_byte + query_text.len();
    }

    occurrences
}

fn normalize_text_with_char_map(
    text: &str,
    case_sensitive: bool,
    ignore_whitespace: bool,
) -> (String, Vec<usize>) {
    let mut search_text = String::new();
    let mut char_map = Vec::new();

    for (char_index, ch) in text.chars().enumerate() {
        if ignore_whitespace && ch.is_whitespace() {
            continue;
        }
        push_normalized_chars(ch, case_sensitive, |normalized| {
            if !ignore_whitespace || !normalized.is_whitespace() {
                search_text.push(normalized);
                char_map.push(char_index);
            }
        });
    }

    (search_text, char_map)
}

fn locate_occurrences_with_strategy(
    glyphs: &[TextGlyph],
    prepared_query: &str,
    case_sensitive: bool,
    ignore_whitespace: bool,
) -> Vec<SearchOccurrence> {
    if prepared_query.is_empty() {
        return Vec::new();
    }

    let (search_text, char_map) =
        normalize_glyphs_for_search(glyphs, case_sensitive, ignore_whitespace);
    if search_text.is_empty() {
        return Vec::new();
    }

    let query_text = normalize_text_for_search(prepared_query, true, ignore_whitespace);
    if query_text.is_empty() || query_text.len() > search_text.len() {
        return Vec::new();
    }

    let char_byte_offsets: Vec<usize> = search_text
        .char_indices()
        .map(|(offset, _)| offset)
        .collect();
    let query_char_len = query_text.chars().count();
    if query_char_len == 0 || query_char_len > char_map.len() {
        return Vec::new();
    }

    let mut occurrences = Vec::new();
    let mut cursor_byte = 0;
    while cursor_byte <= search_text.len() {
        let Some(relative_match_byte) = search_text[cursor_byte..].find(&query_text) else {
            break;
        };
        let match_byte = cursor_byte + relative_match_byte;
        let match_char_start = char_byte_offsets.binary_search(&match_byte);
        debug_assert!(
            match_char_start.is_ok(),
            "str::find returned a non-character-boundary offset"
        );
        let match_char_start =
            match_char_start.expect("str::find returned a non-character-boundary offset");
        let glyph_start = char_map[match_char_start];
        let glyph_end = char_map[match_char_start + query_char_len - 1];
        let rects = merge_text_glyph_rects(&glyphs[glyph_start..=glyph_end]);
        occurrences.push(SearchOccurrence {
            match_start: glyph_start,
            match_end: glyph_end,
            rects,
            snippet: String::new(),
            snippet_match_start: None,
            snippet_match_end: None,
        });
        cursor_byte = match_byte + query_text.len();
    }

    occurrences
}

fn normalize_glyphs_for_search(
    glyphs: &[TextGlyph],
    case_sensitive: bool,
    ignore_whitespace: bool,
) -> (String, Vec<usize>) {
    let mut search_text = String::new();
    let mut char_map = Vec::new();

    for (glyph_index, glyph) in glyphs.iter().enumerate() {
        if ignore_whitespace && glyph.ch.is_whitespace() {
            continue;
        }
        push_normalized_chars(glyph.ch, case_sensitive, |normalized| {
            if !ignore_whitespace || !normalized.is_whitespace() {
                search_text.push(normalized);
                char_map.push(glyph_index);
            }
        });
    }

    (search_text, char_map)
}

fn push_normalized_chars(ch: char, case_sensitive: bool, mut push: impl FnMut(char)) {
    if case_sensitive {
        push(ch);
    } else {
        for normalized in ch.to_lowercase() {
            push(normalized);
        }
    }
}

fn normalize_text_for_search(text: &str, case_sensitive: bool, ignore_whitespace: bool) -> String {
    let mut normalized_text = String::with_capacity(text.len());
    for ch in text.chars() {
        if ignore_whitespace && ch.is_whitespace() {
            continue;
        }
        push_normalized_chars(ch, case_sensitive, |normalized| {
            if !ignore_whitespace || !normalized.is_whitespace() {
                normalized_text.push(normalized);
            }
        });
    }
    normalized_text
}

#[cfg(test)]
mod tests {
    use super::{apply_hit_snippet, locate_occurrences, normalize_text_for_search};
    use crate::backend::{PdfRect, TextGlyph};
    use crate::search::engine::SearchOccurrence;

    #[test]
    fn locate_occurrences_skips_whitespace_insensitive_fallback_after_direct_match() {
        let glyphs = vec![
            glyph('f', 10.0, 20.0, 18.0, 32.0),
            glyph('o', 20.0, 20.0, 28.0, 32.0),
            glyph('o', 30.0, 20.0, 38.0, 32.0),
            glyph('b', 40.0, 20.0, 48.0, 32.0),
            glyph('a', 50.0, 20.0, 58.0, 32.0),
            glyph('r', 60.0, 20.0, 68.0, 32.0),
            glyph(' ', 70.0, 20.0, 74.0, 32.0),
            glyph('f', 80.0, 20.0, 88.0, 32.0),
            glyph('o', 90.0, 20.0, 98.0, 32.0),
            glyph('o', 100.0, 20.0, 108.0, 32.0),
            glyph(' ', 110.0, 20.0, 114.0, 32.0),
            glyph('b', 120.0, 20.0, 128.0, 32.0),
            glyph('a', 130.0, 20.0, 138.0, 32.0),
            glyph('r', 140.0, 20.0, 148.0, 32.0),
        ];

        let occurrences = locate_occurrences(&glyphs, "foobar", false);

        assert_eq!(occurrences.len(), 1);
        assert_eq!(occurrences[0].match_start, 0);
        assert_eq!(occurrences[0].match_end, 5);
    }

    #[test]
    fn locate_occurrences_uses_whitespace_insensitive_fallback_without_direct_match() {
        let glyphs = vec![
            glyph('f', 10.0, 20.0, 18.0, 32.0),
            glyph('o', 20.0, 20.0, 28.0, 32.0),
            glyph('o', 30.0, 20.0, 38.0, 32.0),
            glyph('b', 40.0, 20.0, 48.0, 32.0),
            glyph('a', 50.0, 20.0, 58.0, 32.0),
            glyph('r', 60.0, 20.0, 68.0, 32.0),
        ];

        let occurrences = locate_occurrences(&glyphs, "foo bar", false);

        assert_eq!(occurrences.len(), 1);
        assert_eq!(occurrences[0].match_start, 0);
        assert_eq!(occurrences[0].match_end, 5);
    }

    #[test]
    fn locate_occurrences_maps_multibyte_byte_offsets_to_char_positions() {
        let glyphs = vec![
            glyph('a', 10.0, 20.0, 18.0, 32.0),
            glyph('β', 20.0, 20.0, 28.0, 32.0),
            glyph('a', 30.0, 20.0, 38.0, 32.0),
            glyph('β', 40.0, 20.0, 48.0, 32.0),
        ];

        let occurrences = locate_occurrences(&glyphs, "βa", true);

        assert_eq!(occurrences.len(), 1);
        assert_eq!(occurrences[0].match_start, 1);
        assert_eq!(occurrences[0].match_end, 2);
    }

    #[test]
    fn normalize_text_for_search_preserves_search_semantics() {
        assert_eq!(normalize_text_for_search("İ", false, false), "i\u{307}");
        assert_eq!(normalize_text_for_search("İ", true, false), "İ");
        assert_eq!(
            normalize_text_for_search("A \tİ\nB", false, true),
            "ai\u{307}b"
        );
    }

    #[test]
    fn apply_hit_snippet_uses_original_glyph_boundaries_after_case_fold_expansion() {
        let glyphs = vec![
            glyph('İ', 10.0, 20.0, 18.0, 32.0),
            glyph('x', 20.0, 20.0, 28.0, 32.0),
        ];
        let mut occurrence = SearchOccurrence {
            match_start: 0,
            match_end: 0,
            rects: vec![PdfRect {
                x0: 10.0,
                y0: 20.0,
                x1: 18.0,
                y1: 32.0,
            }],
            snippet: String::new(),
            snippet_match_start: None,
            snippet_match_end: None,
        };

        apply_hit_snippet(&mut occurrence, &glyphs);

        assert_eq!(occurrence.snippet, "İx");
        assert_eq!(occurrence.snippet_match_start, Some(0));
        assert_eq!(occurrence.snippet_match_end, Some('İ'.len_utf8()));
    }

    fn glyph(ch: char, x0: f32, y0: f32, x1: f32, y1: f32) -> TextGlyph {
        TextGlyph {
            ch,
            bbox: Some(PdfRect { x0, y0, x1, y1 }),
        }
    }
}
