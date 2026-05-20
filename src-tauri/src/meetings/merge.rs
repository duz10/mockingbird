//! Cross-channel chronological merge for two-channel meetings.
//!
//! Takes the mic and system `TimedSegment` vectors out of
//! [`super::long_form_stt::LongFormOutput`] and produces a single
//! Markdown body where each paragraph is labeled `**You:**` (mic) or
//! `**Other(s):**` (system) and the paragraphs appear in chronological
//! order by `t0_ms`. Consecutive paragraphs on the same channel
//! collapse into a single labeled paragraph (no double labels).
//!
//! Wave 4 ships this. Lives in its own module rather than inside
//! [`super::runtime`] because:
//!   1. It's pure — easy to unit test in isolation.
//!   2. Keeps `runtime.rs` focused on lifecycle (Drop, thread joining,
//!      AppHandle event emission) rather than text munging.
//!   3. The `mc-two-channel-merged` judge (Wave 6) reads the test
//!      file paths; co-locating logic + tests under the merge name
//!      makes the judge config obvious.
//!
//! See `phase-mc-wave4-brief.md` §4.2 for the wider lifecycle context.

use super::capture::Channel;
use super::long_form_stt::TimedSegment;

/// Speaker label rendered next to each paragraph.
const YOU_LABEL: &str = "**You:**";
const OTHER_LABEL: &str = "**Other(s):**";

/// Merge two per-channel segment streams into a single labeled
/// Markdown body. The output mirrors what the export module emits
/// for `MeetingSource::Both` when the merged channel is present.
///
/// Algorithm:
/// 1. Tag each segment with its channel.
/// 2. Stable-sort by `t0_ms` (stable so two segments at the same
///    timestamp keep mic-before-system, matching how the long-form
///    driver tends to land mic chunks slightly earlier in
///    multi-threaded races).
/// 3. Walk the tagged stream; emit a paragraph break + new label
///    when the channel flips, else append the text to the running
///    paragraph with a single space separator.
/// 4. Trim trailing whitespace per paragraph.
///
/// Empty inputs → empty string. Skips empty/whitespace-only segment
/// texts so a transcript with one all-whitespace segment doesn't
/// leak a stray `**Other(s):**` label.
pub fn merge_two_channels(mic: &[TimedSegment], sys: &[TimedSegment]) -> String {
    if mic.is_empty() && sys.is_empty() {
        return String::new();
    }
    let mut tagged: Vec<(Channel, &TimedSegment)> = Vec::with_capacity(mic.len() + sys.len());
    for s in mic {
        if !s.text.trim().is_empty() {
            tagged.push((Channel::Mic, s));
        }
    }
    for s in sys {
        if !s.text.trim().is_empty() {
            tagged.push((Channel::Sys, s));
        }
    }
    // Stable sort by t0_ms; on ties, the mic entries (pushed first)
    // stay first by virtue of stability.
    tagged.sort_by_key(|(_, s)| s.t0_ms);

    let mut out = String::new();
    let mut current_channel: Option<Channel> = None;
    for (ch, seg) in tagged {
        let text = seg.text.trim();
        match current_channel {
            None => {
                out.push_str(label_for(ch));
                out.push(' ');
                out.push_str(text);
                current_channel = Some(ch);
            }
            Some(prev) if prev == ch => {
                // Same speaker continues — single-space join.
                out.push(' ');
                out.push_str(text);
            }
            Some(_) => {
                // Speaker change — paragraph break + new label.
                out.push_str("\n\n");
                out.push_str(label_for(ch));
                out.push(' ');
                out.push_str(text);
                current_channel = Some(ch);
            }
        }
    }
    out
}

fn label_for(ch: Channel) -> &'static str {
    match ch {
        Channel::Mic => YOU_LABEL,
        Channel::Sys => OTHER_LABEL,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(t0: u32, t1: u32, text: &str) -> TimedSegment {
        TimedSegment {
            t0_ms: t0,
            t1_ms: t1,
            text: text.to_string(),
        }
    }

    #[test]
    fn empty_inputs_produce_empty_string() {
        assert_eq!(merge_two_channels(&[], &[]), "");
    }

    #[test]
    fn mic_only_labels_you_for_every_paragraph() {
        let mic = vec![seg(0, 1000, "hello"), seg(1000, 2000, "world")];
        let out = merge_two_channels(&mic, &[]);
        // Same speaker continues; one **You:** prefix.
        assert_eq!(out, "**You:** hello world");
    }

    #[test]
    fn sys_only_labels_other_for_every_paragraph() {
        let sys = vec![seg(0, 1000, "hi there")];
        let out = merge_two_channels(&[], &sys);
        assert_eq!(out, "**Other(s):** hi there");
    }

    #[test]
    fn speaker_alternation_creates_paragraph_breaks() {
        let mic = vec![seg(0, 1000, "hi"), seg(4000, 5000, "okay")];
        let sys = vec![seg(2000, 3000, "hello"), seg(6000, 7000, "great")];
        let out = merge_two_channels(&mic, &sys);
        // Order by t0: mic@0, sys@2000, mic@4000, sys@6000.
        // Four alternations → four paragraphs.
        let expected = "**You:** hi\n\n**Other(s):** hello\n\n**You:** okay\n\n**Other(s):** great";
        assert_eq!(out, expected);
    }

    #[test]
    fn consecutive_same_channel_collapse_to_one_paragraph() {
        let mic = vec![
            seg(0, 1000, "one"),
            seg(1000, 2000, "two"),
            seg(2000, 3000, "three"),
        ];
        let sys = vec![seg(4000, 5000, "four")];
        let out = merge_two_channels(&mic, &sys);
        assert_eq!(out, "**You:** one two three\n\n**Other(s):** four");
    }

    #[test]
    fn whitespace_only_segments_are_skipped() {
        let mic = vec![seg(0, 1000, "real"), seg(1000, 2000, "   ")];
        let sys = vec![seg(500, 600, "")];
        let out = merge_two_channels(&mic, &sys);
        // Only "real" survives — no stray labels.
        assert_eq!(out, "**You:** real");
    }

    #[test]
    fn tie_break_puts_mic_before_sys() {
        // Same t0_ms; stable sort preserves the mic-first push order.
        let mic = vec![seg(1000, 2000, "mine")];
        let sys = vec![seg(1000, 2000, "theirs")];
        let out = merge_two_channels(&mic, &sys);
        assert_eq!(out, "**You:** mine\n\n**Other(s):** theirs");
    }

    #[test]
    fn segments_internally_trimmed() {
        let mic = vec![seg(0, 1000, "  hello  ")];
        let out = merge_two_channels(&mic, &[]);
        assert_eq!(out, "**You:** hello");
    }

    #[test]
    fn out_of_order_segments_resorted_globally() {
        // Even if a single channel's segments are passed out of
        // order (shouldn't happen in production but defensive),
        // the merge sorts globally by t0_ms.
        let mic = vec![seg(3000, 4000, "later"), seg(0, 1000, "earlier")];
        let out = merge_two_channels(&mic, &[]);
        // Stable sort: earlier comes first; same-channel collapse joins them.
        assert_eq!(out, "**You:** earlier later");
    }
}
