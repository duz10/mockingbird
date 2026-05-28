//! Tag-normalization pass — pure Rust, no LLM. The load-bearing
//! tag-collapse mechanism per spec §12.
//!
//! Per spec §7.2 Layer 3:
//! - lowercase
//! - replace whitespace and `_` with `-`
//! - collapse repeated `-`
//! - trim leading/trailing `-`
//! - conservative singularization (see [`singularize`])
//! - dedupe (set-equality semantics; preserve first-seen order)
//!
//! Over-singularization ("boss" → "bos") is worse than
//! under-singularization, so the rules are deliberately conservative.
//! Cleverness here directly produces false-positive tag merges that
//! a Wave-3 LLM judge cannot distinguish from genuinely-equivalent
//! tags.

/// Normalize a batch of raw tags from the extract pass into the
/// canonical form the answer keys are written in. Order of
/// first-seen tags is preserved; duplicates after normalization are
/// dropped. Empty input yields empty output.
pub fn normalize_tags(raw: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(raw.len());
    for tag in raw {
        let n = normalize_one(tag);
        if n.is_empty() {
            continue;
        }
        if !out.iter().any(|existing| existing == &n) {
            out.push(n);
        }
    }
    out
}

fn normalize_one(raw: &str) -> String {
    // Step 1: lowercase; step 2: whitespace + `_` → `-`.
    let mut s: String = raw
        .chars()
        .map(|c| {
            let lower = c.to_ascii_lowercase();
            if lower.is_whitespace() || lower == '_' {
                '-'
            } else {
                lower
            }
        })
        .collect();

    // Step 3: collapse repeated `-`. Cheap O(n) state machine.
    let mut collapsed = String::with_capacity(s.len());
    let mut prev_dash = false;
    for c in s.drain(..) {
        if c == '-' {
            if !prev_dash {
                collapsed.push('-');
            }
            prev_dash = true;
        } else {
            collapsed.push(c);
            prev_dash = false;
        }
    }

    // Step 4: trim leading/trailing `-`.
    let trimmed = collapsed.trim_matches('-').to_string();

    // Step 5: singularize the LAST hyphen-delimited segment. The
    // last segment is the head noun in compounds like
    // `marketing-leads` → `marketing-lead`. Earlier segments stay
    // untouched (singularizing "leads" only is correct;
    // singularizing "marketing" would be wrong).
    if let Some(idx) = trimmed.rfind('-') {
        let (head, last) = trimmed.split_at(idx + 1);
        let mut s = String::with_capacity(head.len() + last.len());
        s.push_str(head);
        s.push_str(&singularize(last));
        s
    } else {
        singularize(&trimmed)
    }
}

/// Conservative English singularization. Rules:
///
/// - `ies` → `y`        (parties → party)
/// - drop trailing `s`  ONLY when:
///     - word length > 3
///     - prior char is NOT in {s, x, z, u, i, o}    (avoid bus → bu, taxes handled below)
///     - word does NOT end in {ss, sh, ch, us}      (boss → boss, cash → cash, watch → watch)
///
/// "boss" stays "boss". "kids" → "kid". "taxes" → "tax".
/// "marketing" stays "marketing" (no trailing s).
fn singularize(word: &str) -> String {
    if word.len() <= 3 {
        return word.to_string();
    }
    // `ies` → `y` (parties → party). Requires word length > 3 already.
    if word.ends_with("ies") && word.len() > 3 {
        let mut s = word[..word.len() - 3].to_string();
        s.push('y');
        return s;
    }
    if !word.ends_with('s') {
        return word.to_string();
    }
    // Block double-s, sh, ch, us suffixes.
    if word.ends_with("ss") || word.ends_with("sh") || word.ends_with("ch") || word.ends_with("us")
    {
        return word.to_string();
    }
    // Handle the "taxes" / "boxes" family: -xes → -x, -zes → -z.
    if word.ends_with("xes") || word.ends_with("zes") {
        return word[..word.len() - 2].to_string();
    }
    // Block words whose char before the trailing `s` is in the
    // ambiguity set (vowels other than 'a'/'e' + sibilants we
    // already handled). Bytes are fine because we restrict to ASCII
    // tags — non-ASCII tags would already have failed an earlier
    // sanity check; in any event, falling through to "leave as-is"
    // is the safe default.
    let bytes = word.as_bytes();
    let prior = bytes[bytes.len() - 2] as char;
    if matches!(prior, 's' | 'x' | 'z' | 'u' | 'i' | 'o') {
        return word.to_string();
    }
    word[..word.len() - 1].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(input: &[&str]) -> Vec<String> {
        normalize_tags(&input.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn lowercase_and_hyphenate() {
        assert_eq!(
            n(&["Reading List", "Reading_List", "READING list"]),
            vec!["reading-list"]
        );
    }

    #[test]
    fn dedupes_after_normalization() {
        assert_eq!(
            n(&["mockingbird", "Mockingbird", "mockingbird"]),
            vec!["mockingbird"]
        );
    }

    #[test]
    fn collapses_repeated_hyphens_and_trims_edges() {
        assert_eq!(n(&["--car--repair--"]), vec!["car-repair"]);
        assert_eq!(n(&["car  repair"]), vec!["car-repair"]);
    }

    #[test]
    fn ies_to_y() {
        assert_eq!(n(&["parties"]), vec!["party"]);
        assert_eq!(n(&["categories"]), vec!["category"]);
    }

    #[test]
    fn drops_simple_plural_s() {
        assert_eq!(n(&["kids"]), vec!["kid"]);
        assert_eq!(n(&["leads"]), vec!["lead"]);
    }

    #[test]
    fn preserves_ss_sh_ch_us_words() {
        assert_eq!(n(&["boss"]), vec!["boss"]);
        assert_eq!(n(&["cash"]), vec!["cash"]);
        assert_eq!(n(&["watch"]), vec!["watch"]);
        assert_eq!(n(&["focus"]), vec!["focus"]);
    }

    #[test]
    fn taxes_to_tax() {
        assert_eq!(n(&["taxes"]), vec!["tax"]);
        assert_eq!(n(&["boxes"]), vec!["box"]);
    }

    #[test]
    fn does_not_over_shrink_short_words() {
        // 3 chars: leave alone even if ends in s.
        assert_eq!(n(&["yes"]), vec!["yes"]);
        // "bus" — short AND ends in `us`; either rule alone protects it.
        assert_eq!(n(&["bus"]), vec!["bus"]);
    }

    #[test]
    fn compound_tag_singularizes_only_head_noun() {
        assert_eq!(n(&["marketing-leads"]), vec!["marketing-lead"]);
        // Earlier segment ending in `s` stays put.
        assert_eq!(n(&["business-cards"]), vec!["business-card"]);
    }

    #[test]
    fn empty_input_yields_empty_output() {
        assert!(normalize_tags(&[]).is_empty());
    }

    #[test]
    fn whitespace_only_tags_dropped() {
        assert_eq!(n(&["   ", "kids"]), vec!["kid"]);
    }
}
