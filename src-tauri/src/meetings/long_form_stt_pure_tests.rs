//! Pure-helper tests for [`super::long_form_stt`]. No file I/O, no
//! threads, no STT stub. Split out of `long_form_stt_tests.rs` so the
//! main test file stays under the 600-line cap.

use super::*;
use crate::meetings::capture::Channel;
use std::path::PathBuf;

// ---------------------------------------------------------------------
// Public type smoke tests (carry-forward from the Wave 1 scaffold)
// ---------------------------------------------------------------------

#[test]
fn timed_segment_constructs() {
    let s = TimedSegment {
        text: "hello".into(),
        t0_ms: 0,
        t1_ms: 500,
    };
    assert!(s.t1_ms > s.t0_ms);
}

#[test]
fn timed_segment_is_stt_segment_alias() {
    let s: TimedSegment = crate::stt::SttSegment {
        text: "x".into(),
        t0_ms: 1,
        t1_ms: 2,
    };
    assert_eq!(s.text, "x");
}

#[test]
fn progress_struct_constructs() {
    let p = LongFormProgress {
        channel: Channel::Mic,
        chunk_seq: 0,
        chunks_done: 1,
        chunks_total: None,
    };
    assert_eq!(p.chunks_done, 1);
}

#[test]
fn config_overlap_ms_matches_samples() {
    let c = LongFormConfig::default();
    assert_eq!(c.overlap_ms(), 2_000); // 32_000 / 16
    let tiny = LongFormConfig {
        overlap_samples: 1_600,
        sample_rate: 16_000,
        max_prompt_tokens: 224,
    };
    assert_eq!(tiny.overlap_ms(), 100); // 1_600 / 16
}

// ---------------------------------------------------------------------
// parse_seq_from_path
// ---------------------------------------------------------------------

#[test]
fn parse_seq_from_path_extracts_trailing_u32() {
    let p = PathBuf::from("/tmp/myuuid_mic_42.wav");
    assert_eq!(parse_seq_from_path(&p).unwrap(), 42);
}

#[test]
fn parse_seq_from_path_handles_uuid_with_dashes() {
    // Real UUIDs contain dashes (not underscores) so the rsplit on
    // `_` lands on the seq field cleanly.
    let p = PathBuf::from("/tmp/abc-def-ghi_sys_7.wav");
    assert_eq!(parse_seq_from_path(&p).unwrap(), 7);
}

#[test]
fn parse_seq_from_path_rejects_non_numeric() {
    let p = PathBuf::from("/tmp/uuid_mic_notanumber.wav");
    assert!(parse_seq_from_path(&p).is_err());
}

// ---------------------------------------------------------------------
// chunk_global_offset_ms
// ---------------------------------------------------------------------

#[test]
fn chunk_global_offset_ms_is_correct() {
    assert_eq!(chunk_global_offset_ms(0, 16_000), 0);
    assert_eq!(chunk_global_offset_ms(16_000, 16_000), 1_000);
    assert_eq!(chunk_global_offset_ms(480_000, 16_000), 30_000);
    // 4-hour meeting sample count must not overflow.
    let four_hour_samples = 4u64 * 3600 * 16_000;
    let expected_ms = 4u32 * 3600 * 1000;
    assert_eq!(
        chunk_global_offset_ms(four_hour_samples, 16_000),
        expected_ms
    );
}

// ---------------------------------------------------------------------
// tail_tokens
// ---------------------------------------------------------------------

#[test]
fn tail_tokens_returns_none_on_empty() {
    assert_eq!(tail_tokens("", 224), None);
    assert_eq!(tail_tokens("   ", 224), None);
}

#[test]
fn tail_tokens_returns_full_text_when_under_budget() {
    let text = "hello world this is short";
    assert_eq!(tail_tokens(text, 224).unwrap(), text);
}

#[test]
fn tail_tokens_truncates_to_word_budget() {
    let words: Vec<String> = (0..1000).map(|i| format!("w{i}")).collect();
    let text = words.join(" ");
    let out = tail_tokens(&text, 224).unwrap();
    // 224 tokens / 1.3 ≈ 172 words floored.
    let out_words: Vec<&str> = out.split_whitespace().collect();
    assert_eq!(out_words.len(), 172);
    // Must be the TRAILING 172, not the first 172.
    assert_eq!(out_words[0], "w828"); // 1000 - 172 = 828
    assert_eq!(out_words[171], "w999");
}

// ---------------------------------------------------------------------
// crc32_of_i16 — must agree byte-for-byte with chunker's CRC.
// ---------------------------------------------------------------------

#[test]
fn crc32_of_i16_is_deterministic_and_endian_stable() {
    let samples = vec![0i16, 1, -1, 32767, -32768];
    let a = crc32_of_i16(&samples);
    let b = crc32_of_i16(&samples);
    assert_eq!(a, b);
    // Cross-check against crc32fast over the same LE byte sequence.
    let mut h = crc32fast::Hasher::new();
    for s in &samples {
        h.update(&s.to_le_bytes());
    }
    assert_eq!(a, h.finalize());
}
