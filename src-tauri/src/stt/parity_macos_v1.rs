//! macOS Metal STT parity eval + WER/CER metric
//! (mb-mac-v1.6.2 / judge `mac-v1-parity-whisper-metal`, Phase 5).
//!
//! Phase 5 asks a single, load-bearing question: does the macOS **Metal**
//! Whisper backend produce transcription quality on par with the Windows
//! **CUDA** build? The parity premise is that both platforms run the same
//! whisper.cpp engine over the same GGUF weights
//! (`whisper-large-v3-turbo-q5_0`), so the Metal output should be
//! near-identical to the CUDA output.
//!
//! This module provides two things:
//!
//!   1. A **cross-platform** WER/CER metric (word- and character-level
//!      Levenshtein over normalized text). It has no platform/feature
//!      gate so it compiles and unit-tests on every target -- the metric
//!      itself is pure text math and deserves coverage independent of
//!      whether a Metal model is on the box.
//!
//!   2. A **macOS + `metal`** eval driver ([`run_parity_eval`]) that runs
//!      the PRODUCTION [`crate::stt::whisper::WhisperStt`] (Metal-backed)
//!      over a reference corpus, confirms the Metal backend actually
//!      engaged (reusing [`crate::stt::judges_macos_v1`]'s ggml/whisper
//!      log-capture -- DRY, no silent CPU fallback), and scores each
//!      clip's transcript against its ground-truth reference.
//!
//! ## Acceptance (the judge's bar)
//!
//! Parity is met when the Metal transcript's WER/CER against the
//! ground-truth reference is within the corpus budget (see
//! `eval/stt_reference_transcripts.json`). `large-v3-turbo-q5` on a clean
//! clip like `jfk.wav` is expected to land at or very near WER 0; the
//! default budget (WER <= 0.10, CER <= 0.05) leaves a few % of slack for
//! q5 quantization plus Metal-vs-CUDA numeric drift.
//!
//! ## What this does NOT do
//!
//! It scores Metal-vs-**ground-truth**, which is the primary parity
//! acceptance. A stricter Metal-vs-**CUDA-golden** byte/near-byte
//! comparison would need CUDA reference transcripts generated on the
//! Windows box; none exist in-repo. See the shim + final report for what
//! to generate there if that tighter check is wanted for v1.

// --- Cross-platform metric ------------------------------------------------
// No cfg gate: pure text math, unit-tested on every target.

/// Normalize text for WER/CER comparison: lowercase, drop punctuation and
/// symbols (treated as separators), collapse runs of whitespace to a
/// single space, and trim. Digits and letters are preserved (lowercased).
///
/// This is the standard STT-eval normalization: it removes the things a
/// transcriber has no reliable way to reproduce (capitalization, comma vs
/// dash) so the metric measures *content* fidelity, not formatting.
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    for c in s.chars() {
        if c.is_alphanumeric() {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            for lc in c.to_lowercase() {
                out.push(lc);
            }
        } else {
            // Whitespace, punctuation, or any other symbol -> separator.
            pending_space = true;
        }
    }
    out
}

/// Generic Levenshtein (edit) distance over two slices, O(n*m) time,
/// O(m) space via a rolling row.
fn levenshtein<T: PartialEq>(a: &[T], b: &[T]) -> usize {
    let (n, m) = (a.len(), b.len());
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut cur: Vec<usize> = vec![0; m + 1];
    for i in 1..=n {
        cur[0] = i;
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[m]
}

/// Split normalized text into word tokens.
fn words(s: &str) -> Vec<String> {
    normalize(s)
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

/// Edit counts + denominator for one clip, so aggregate scores can be a
/// principled micro-average (total edits / total reference length) rather
/// than a mean-of-ratios.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditCounts {
    /// Levenshtein distance (edits).
    pub distance: usize,
    /// Reference length (word or char count) -- the WER/CER denominator.
    pub reference_len: usize,
}

impl EditCounts {
    /// Ratio (distance / reference_len). Empty reference => 0.0 when the
    /// hypothesis is also empty (distance 0), else 1.0 (fully wrong).
    pub fn ratio(self) -> f64 {
        if self.reference_len == 0 {
            return if self.distance == 0 { 0.0 } else { 1.0 };
        }
        self.distance as f64 / self.reference_len as f64
    }
}

/// Word-level edit counts between a reference and a hypothesis.
pub fn word_edits(reference: &str, hypothesis: &str) -> EditCounts {
    let r = words(reference);
    let h = words(hypothesis);
    EditCounts {
        distance: levenshtein(&r, &h),
        reference_len: r.len(),
    }
}

/// Character-level edit counts over the normalized strings (spaces
/// included, so word boundaries count -- applied identically to both
/// sides, so the measure stays consistent).
pub fn char_edits(reference: &str, hypothesis: &str) -> EditCounts {
    let r: Vec<char> = normalize(reference).chars().collect();
    let h: Vec<char> = normalize(hypothesis).chars().collect();
    EditCounts {
        distance: levenshtein(&r, &h),
        reference_len: r.len(),
    }
}

/// Word Error Rate: word-level Levenshtein / reference word count.
pub fn wer(reference: &str, hypothesis: &str) -> f64 {
    word_edits(reference, hypothesis).ratio()
}

/// Character Error Rate: char-level Levenshtein / reference char count.
pub fn cer(reference: &str, hypothesis: &str) -> f64 {
    char_edits(reference, hypothesis).ratio()
}

/// One clip's specification for the eval: a name, the WAV path, and the
/// ground-truth reference transcript.
#[derive(Debug, Clone)]
pub struct ClipSpec {
    /// Display name (e.g. the fixture filename).
    pub name: String,
    /// Path to the 16 kHz mono i16 WAV.
    pub wav_path: std::path::PathBuf,
    /// Human ground-truth transcript.
    pub reference: String,
}

/// Per-clip parity result.
#[derive(Debug, Clone)]
pub struct ClipParity {
    /// Clip display name.
    pub name: String,
    /// Ground-truth reference.
    pub reference: String,
    /// The Metal transcript produced.
    pub hypothesis: String,
    /// Word Error Rate vs reference.
    pub wer: f64,
    /// Character Error Rate vs reference.
    pub cer: f64,
    /// Word-level edit counts (feed the aggregate micro-average).
    pub word_counts: EditCounts,
    /// Char-level edit counts (feed the aggregate micro-average).
    pub char_counts: EditCounts,
    /// End-to-end transcribe latency (ms).
    pub latency_ms: u64,
}

/// Aggregate parity report over the corpus.
#[derive(Debug, Clone)]
pub struct ParityReport {
    /// Per-clip results.
    pub clips: Vec<ClipParity>,
    /// Corpus WER (micro-average: total word edits / total reference words).
    pub aggregate_wer: f64,
    /// Corpus CER (micro-average: total char edits / total reference chars).
    pub aggregate_cer: f64,
    /// The confirmed backend the model loaded on (`"Metal"` or `"CPU"`).
    pub backend: String,
    /// Whether the Metal backend actually engaged (parity precondition).
    pub metal_engaged: bool,
    /// The exact ggml/whisper log line proving the backend, if captured.
    pub backend_evidence: Option<String>,
}

impl ParityReport {
    /// Build the aggregate (micro-averaged) report from per-clip results.
    pub fn from_clips(
        clips: Vec<ClipParity>,
        backend: String,
        metal_engaged: bool,
        backend_evidence: Option<String>,
    ) -> Self {
        let (mut w_dist, mut w_ref, mut c_dist, mut c_ref) = (0usize, 0usize, 0usize, 0usize);
        for c in &clips {
            w_dist += c.word_counts.distance;
            w_ref += c.word_counts.reference_len;
            c_dist += c.char_counts.distance;
            c_ref += c.char_counts.reference_len;
        }
        let aggregate_wer = EditCounts {
            distance: w_dist,
            reference_len: w_ref,
        }
        .ratio();
        let aggregate_cer = EditCounts {
            distance: c_dist,
            reference_len: c_ref,
        }
        .ratio();
        Self {
            clips,
            aggregate_wer,
            aggregate_cer,
            backend,
            metal_engaged,
            backend_evidence,
        }
    }
}

// --- macOS + metal eval driver -------------------------------------------

/// Run the parity eval over `clips` using the PRODUCTION Metal-backed
/// [`crate::stt::whisper::WhisperStt`] loaded from `model_path`.
///
/// The model is loaded ONCE (a ~550 MB GGUF; reloading per-clip would be
/// wasteful) and every clip is transcribed through it. The Metal backend
/// is confirmed via the shared ggml/whisper log capture; a silent CPU
/// fallback is surfaced (`metal_engaged = false`) so the judge can fail
/// rather than rubber-stamp a non-parity run.
///
/// Returns `Err` only on hard failures (model/WAV missing or malformed,
/// whisper init/inference error). Scoring the transcripts against the
/// budget is the caller's (judge shim's) job.
#[cfg(all(target_os = "macos", feature = "metal"))]
pub fn run_parity_eval(
    model_path: &std::path::Path,
    clips: &[ClipSpec],
) -> Result<ParityReport, String> {
    use crate::stt::judges_macos_v1::{
        begin_backend_capture, classify_captured_backend, read_wav_16k_mono_i16, Backend,
    };
    use crate::stt::whisper::WhisperStt;
    use crate::stt::{SpeechToText, TranscribeRequest};

    if !model_path.is_file() {
        return Err(format!(
            "whisper model not found at {} (run scripts/download-models.sh)",
            model_path.display()
        ));
    }

    // Begin backend capture BEFORE constructing the context so the
    // Metal-init lines land in the buffer, then build the production STT.
    // whisper.cpp emits its backend-init line ("whisper_backend_init_gpu:
    // using Metal backend") LAZILY -- on the first inference, not at
    // context construction -- so we classify only AFTER the first
    // transcribe below (mirrors `metal_transcript_probe` +
    // `ungate_backends_probe`).
    begin_backend_capture();
    let mut stt = WhisperStt::from_path(model_path, false)
        .map_err(|e| format!("WhisperStt::from_path: {e}"))?;

    let mut results = Vec::with_capacity(clips.len());
    for clip in clips {
        let audio = read_wav_16k_mono_i16(&clip.wav_path)?;
        let transcript = stt
            .transcribe(TranscribeRequest {
                audio: &audio,
                initial_prompt: None,
                force_cpu: false,
            })
            .map_err(|e| format!("transcribe {}: {e}", clip.name))?;
        let hyp = transcript.text.trim().to_string();
        let word_counts = word_edits(&clip.reference, &hyp);
        let char_counts = char_edits(&clip.reference, &hyp);
        results.push(ClipParity {
            name: clip.name.clone(),
            reference: clip.reference.clone(),
            wer: word_counts.ratio(),
            cer: char_counts.ratio(),
            word_counts,
            char_counts,
            hypothesis: hyp,
            latency_ms: transcript.latency_ms,
        });
    }

    // Classify the backend now that at least one inference has run, so
    // whisper.cpp's lazily-emitted Metal-init line is in the buffer.
    let (backend, backend_evidence, _log) = classify_captured_backend();
    let metal_engaged = backend == Backend::Metal;

    Ok(ParityReport::from_clips(
        results,
        backend.to_string(),
        metal_engaged,
        backend_evidence,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_lowercases_strips_punct_collapses_ws() {
        assert_eq!(
            normalize("  And so,  my FELLOW Americans:  ask-not!  "),
            "and so my fellow americans ask not"
        );
    }

    #[test]
    fn identical_text_scores_zero() {
        let s = "and so my fellow americans";
        assert_eq!(wer(s, s), 0.0);
        assert_eq!(cer(s, s), 0.0);
    }

    #[test]
    fn normalization_ignores_case_and_punctuation() {
        // Same content, different formatting => WER/CER both 0.
        let reference = "And so, my fellow Americans: ask not.";
        let hypothesis = "and so my fellow americans ask not";
        assert_eq!(wer(reference, hypothesis), 0.0);
        assert_eq!(cer(reference, hypothesis), 0.0);
    }

    #[test]
    fn one_wrong_word_of_five_is_wer_0_2() {
        // reference has 5 words; one substitution => 1/5.
        let wer_val = wer("the quick brown fox jumps", "the quick brown cat jumps");
        assert!((wer_val - 0.2).abs() < 1e-9, "got {wer_val}");
    }

    #[test]
    fn deletion_and_insertion_counted() {
        // ref 3 words, hyp drops one => 1 deletion / 3.
        let e = word_edits("alpha beta gamma", "alpha gamma");
        assert_eq!(e.distance, 1);
        assert_eq!(e.reference_len, 3);
    }

    #[test]
    fn empty_reference_empty_hypothesis_is_zero() {
        assert_eq!(wer("", ""), 0.0);
        assert_eq!(cer("", ""), 0.0);
    }

    #[test]
    fn empty_reference_nonempty_hypothesis_is_one() {
        // A hallucination over silence: fully wrong against an empty ref.
        assert_eq!(wer("", "thank you"), 1.0);
        assert_eq!(cer("", "thank you"), 1.0);
    }

    #[test]
    fn levenshtein_basic() {
        assert_eq!(levenshtein(b"kitten", b"sitting"), 3);
        assert_eq!(levenshtein::<u8>(b"", b"abc"), 3);
        assert_eq!(levenshtein::<u8>(b"abc", b""), 3);
    }

    #[test]
    fn aggregate_is_micro_averaged() {
        // Clip A: 1 edit / 5 ref words. Clip B: 0 edits / 5 ref words.
        // Micro-average = (1+0)/(5+5) = 0.1, NOT mean(0.2, 0.0)=0.1 here
        // by coincidence -- use uneven lengths to prove micro != macro.
        let a = ClipParity {
            name: "a".into(),
            reference: "one two three four five".into(),
            hypothesis: "one two three four SIX".into(),
            wer: 0.2,
            cer: 0.0,
            word_counts: EditCounts {
                distance: 1,
                reference_len: 5,
            },
            char_counts: EditCounts {
                distance: 0,
                reference_len: 20,
            },
            latency_ms: 0,
        };
        let b = ClipParity {
            name: "b".into(),
            reference: "solo".into(),
            hypothesis: "wrong".into(),
            wer: 1.0,
            cer: 0.0,
            word_counts: EditCounts {
                distance: 1,
                reference_len: 1,
            },
            char_counts: EditCounts {
                distance: 0,
                reference_len: 4,
            },
            latency_ms: 0,
        };
        let report = ParityReport::from_clips(vec![a, b], "Metal".into(), true, None);
        // micro WER = (1+1)/(5+1) = 0.3333...; macro would be (0.2+1.0)/2 = 0.6.
        assert!((report.aggregate_wer - (2.0 / 6.0)).abs() < 1e-9);
    }
}
