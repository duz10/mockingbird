#![allow(missing_docs)] // Brief documents the API.

//! Voice-activity-aware audio trimming.
//!
//! Pure-ish helper: takes 16 kHz mono i16 PCM + a `VoiceActivityDetector`,
//! returns speech-only PCM with `lead_in_ms`/`hangover_ms` padding
//! around each contiguous speech region. Short runs below
//! `min_speech_ms` are discarded.

use super::vad::VoiceActivityDetector;
use crate::error::AppResult;

#[derive(Debug, Clone)]
pub struct TrimConfig {
    /// Samples to keep BEFORE each speech run starts.
    pub lead_in_ms: u32,
    /// Samples to keep AFTER each speech run ends.
    pub hangover_ms: u32,
    /// Discard speech runs shorter than this.
    pub min_speech_ms: u32,
}

impl Default for TrimConfig {
    fn default() -> Self {
        Self {
            lead_in_ms: 100,
            hangover_ms: 300,
            min_speech_ms: 200,
        }
    }
}

const SAMPLE_RATE: usize = 16_000;

/// Trim `audio` to speech-only regions per `detector`. Returns
/// concatenated speech regions with `lead_in` + `hangover` padding.
pub fn vad_trim(
    audio: &[i16],
    detector: &mut dyn VoiceActivityDetector,
    cfg: &TrimConfig,
) -> AppResult<Vec<i16>> {
    let fs = detector.frame_samples();
    if fs == 0 {
        return Ok(Vec::new());
    }
    let lead_in_samples = (cfg.lead_in_ms as usize * SAMPLE_RATE) / 1000;
    let hangover_samples = (cfg.hangover_ms as usize * SAMPLE_RATE) / 1000;
    let min_speech_samples = (cfg.min_speech_ms as usize * SAMPLE_RATE) / 1000;

    // First pass — score each full frame.
    let n_frames = audio.len() / fs;
    let mut frame_speech = Vec::with_capacity(n_frames);
    for i in 0..n_frames {
        let start = i * fs;
        let end = start + fs;
        let decision = detector.process_frame(&audio[start..end])?;
        frame_speech.push(decision.is_speech);
    }

    // Second pass — gather contiguous speech runs with padding.
    let mut out = Vec::new();
    let mut i = 0;
    while i < frame_speech.len() {
        if !frame_speech[i] {
            i += 1;
            continue;
        }
        let speech_start_frame = i;
        while i < frame_speech.len() && frame_speech[i] {
            i += 1;
        }
        let speech_end_frame = i;

        let speech_start = speech_start_frame * fs;
        let speech_end = speech_end_frame * fs;
        if speech_end - speech_start < min_speech_samples {
            continue;
        }

        let region_start = speech_start.saturating_sub(lead_in_samples);
        let region_end = (speech_end + hangover_samples).min(audio.len());
        out.extend_from_slice(&audio[region_start..region_end]);
    }

    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::vad::VadFrame;

    /// Fake VAD: flags any frame containing non-zero samples as speech.
    /// Lets us unit-test the trim helper without loading Silero.
    struct AmplitudeVad {
        frame_size: usize,
    }
    impl VoiceActivityDetector for AmplitudeVad {
        fn process_frame(&mut self, frame: &[i16]) -> AppResult<VadFrame> {
            let any_nonzero = frame.iter().any(|&s| s != 0);
            Ok(VadFrame {
                is_speech: any_nonzero,
                confidence: if any_nonzero { 1.0 } else { 0.0 },
            })
        }
        fn reset(&mut self) {}
        fn frame_samples(&self) -> usize {
            self.frame_size
        }
    }

    #[test]
    fn empty_audio_returns_empty() {
        let mut v = AmplitudeVad { frame_size: 512 };
        let out = vad_trim(&[], &mut v, &TrimConfig::default()).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn all_silence_returns_empty() {
        let mut v = AmplitudeVad { frame_size: 512 };
        let audio = vec![0i16; 16_000];
        let out = vad_trim(&audio, &mut v, &TrimConfig::default()).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn pure_speech_returns_with_no_padding() {
        let mut v = AmplitudeVad { frame_size: 512 };
        // 6400 samples of tone (~ 400 ms; well above min_speech_ms = 200).
        let audio = vec![1000i16; 6400];
        let out = vad_trim(
            &audio,
            &mut v,
            &TrimConfig {
                lead_in_ms: 0,
                hangover_ms: 0,
                min_speech_ms: 0,
            },
        )
        .unwrap();
        // We process 6400/512 = 12 full frames = 6144 samples; the
        // 256-sample tail is discarded by the frame loop. Output should
        // be 6144 (no padding).
        assert_eq!(out.len(), 6144);
    }

    #[test]
    fn min_speech_threshold_drops_short_runs() {
        let mut v = AmplitudeVad { frame_size: 512 };
        // 512 samples = ~32 ms — below default 200 ms threshold.
        let audio = vec![1000i16; 512];
        let out = vad_trim(&audio, &mut v, &TrimConfig::default()).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn lead_in_pads_before_speech_start() {
        let mut v = AmplitudeVad { frame_size: 512 };
        // 1024 silence + 6400 speech. lead_in 100 ms (1600 samples)
        // should pad the speech region's start back into the silence.
        let mut audio = vec![0i16; 1024];
        audio.extend(vec![1000i16; 6400]);
        // Total = 7424; 14 full frames; 14*512 = 7168 samples processed.
        let out = vad_trim(
            &audio,
            &mut v,
            &TrimConfig {
                lead_in_ms: 100,
                hangover_ms: 0,
                min_speech_ms: 0,
            },
        )
        .unwrap();
        // Speech starts at frame 2 (sample 1024), lead_in pulls back
        // 1600 samples — but saturating_sub clamps to start of audio.
        // So output should run from 0 to end of speech (7168).
        assert_eq!(out.len(), 7168);
    }

    #[test]
    fn detector_with_zero_frame_size_returns_empty() {
        struct ZeroSize;
        impl VoiceActivityDetector for ZeroSize {
            fn process_frame(&mut self, _: &[i16]) -> AppResult<VadFrame> {
                unreachable!()
            }
            fn reset(&mut self) {}
            fn frame_samples(&self) -> usize {
                0
            }
        }
        let mut v = ZeroSize;
        let audio = vec![1000i16; 1024];
        let out = vad_trim(&audio, &mut v, &TrimConfig::default()).unwrap();
        assert!(out.is_empty());
    }
}
