//! Twin-stream capture coordinator (ADR 0028).
//!
//! Coordinates ≤2 simultaneous `cpal` streams: the system default
//! input device (mic) and the system default output device's loopback
//! endpoint (system audio). Per-channel SPSC ringbufs decouple the
//! `cpal` callback from the drain consumer; clock-aligned drain emits
//! per-channel PCM in lockstep.
//!
//! Wave 1 scaffold — types + trait + stubs only. Wave 3 ships the
//! `TwinStreamCapture` impl and integration tests against synthetic
//! PCM sources.

use crate::error::AppResult;

/// Which audio channel(s) to capture.
///
/// String form matches the `meeting_sessions.source` column and the
/// `SettingKey::MeetingDefaultSource` setting:
///   - [`MeetingSource::Mic`]    → `"mic"`
///   - [`MeetingSource::System`] → `"system"`
///   - [`MeetingSource::Both`]   → `"both"`
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MeetingSource {
    Mic,
    System,
    Both,
}

impl MeetingSource {
    /// Persisted form for the `meeting_sessions.source` column.
    pub fn as_db_str(self) -> &'static str {
        match self {
            Self::Mic => "mic",
            Self::System => "system",
            Self::Both => "both",
        }
    }

    /// Parse from the persisted form.
    pub fn from_db_str(s: &str) -> Option<Self> {
        match s {
            "mic" => Some(Self::Mic),
            "system" => Some(Self::System),
            "both" => Some(Self::Both),
            _ => None,
        }
    }

    pub fn needs_mic(self) -> bool {
        matches!(self, Self::Mic | Self::Both)
    }

    pub fn needs_system(self) -> bool {
        matches!(self, Self::System | Self::Both)
    }
}

/// Source probe — what's actually capturable on this machine right now.
/// Returned by the `meeting_probe_sources` IPC command (Wave 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeetingSourceProbe {
    pub mic_available: bool,
    pub system_available: bool,
}

/// Twin-stream capture coordinator.
///
/// Wave 3 will provide a concrete `CpalTwinStreamCapture` impl. The
/// trait shape exists in Wave 1 so the runtime (Wave 4) can be wired
/// against the trait, not the impl.
pub trait TwinStreamCapture {
    /// Start any streams the configured `MeetingSource` requires.
    fn start(&mut self) -> AppResult<()>;

    /// Stop all running streams. Idempotent.
    fn stop(&mut self) -> AppResult<()>;

    /// Drain pending mic samples. Returns `Ok(0)` if mic isn't being
    /// captured (e.g. source is `System` only).
    fn drain_mic(&mut self, buf: &mut Vec<i16>) -> AppResult<usize>;

    /// Drain pending system samples. Returns `Ok(0)` if system isn't
    /// being captured (e.g. source is `Mic` only).
    fn drain_system(&mut self, buf: &mut Vec<i16>) -> AppResult<usize>;

    /// Sample rate of the captured streams (16 kHz mono i16, matching
    /// the dictation pipeline).
    fn sample_rate(&self) -> u32 {
        16_000
    }
}

/// Probe the current system for available capture sources. Pure
/// observation; does NOT start any streams.
///
/// Wave 1: stub returning `mic_available=true, system_available=false`
/// so the trait-level surface exists. Wave 3 replaces this with the
/// real cpal-driven probe.
pub fn probe_sources() -> AppResult<MeetingSourceProbe> {
    Ok(MeetingSourceProbe {
        mic_available: true,
        system_available: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_str_round_trip() {
        for s in [
            MeetingSource::Mic,
            MeetingSource::System,
            MeetingSource::Both,
        ] {
            assert_eq!(MeetingSource::from_db_str(s.as_db_str()), Some(s));
        }
    }

    #[test]
    fn from_db_str_rejects_unknown() {
        assert!(MeetingSource::from_db_str("speakers").is_none());
        assert!(MeetingSource::from_db_str("").is_none());
        assert!(MeetingSource::from_db_str("MIC").is_none()); // case-sensitive
    }

    #[test]
    fn needs_flags_correct() {
        assert!(MeetingSource::Mic.needs_mic());
        assert!(!MeetingSource::Mic.needs_system());
        assert!(!MeetingSource::System.needs_mic());
        assert!(MeetingSource::System.needs_system());
        assert!(MeetingSource::Both.needs_mic());
        assert!(MeetingSource::Both.needs_system());
    }
}
