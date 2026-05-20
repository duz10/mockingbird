//! Meeting capture runtime — lifecycle owner.
//!
//! `MeetingCaptureRuntime` is the long-lived object held in Tauri's
//! `manage(...)` registry. It owns:
//!   - the dedicated meetings message-pump thread (Wave 3) that
//!     installs the second `WH_KEYBOARD_LL` hook and feeds the
//!     [`super::activation::Activation`] state machine;
//!   - the meeting-capture worker thread (Wave 4) that the chord
//!     activation spawns on Start and joins on Stop;
//!   - a shared handle to the SQLite connection (for persist on
//!     completion) and to the Tauri `AppHandle` (for emitting overlay
//!     events).
//!
//! Wave 1 scaffold — type + `todo!()` lifecycle stubs only.
//!
//! Wave 3 fills in the hook install + activation event loop.
//! Wave 4 fills in the capture worker spawn / stop / persist pipeline.

use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::error::AppResult;

/// Configuration captured at runtime spawn. Read once from the
/// `settings` table; the runtime does NOT re-read on every toggle
/// (settings changes take effect on app restart, mirroring the
/// dictation runtime's behaviour).
#[derive(Debug, Clone)]
pub struct MeetingRuntimeConfig {
    /// Resolved modifier VK code. Conflict probe (Wave 3) verifies
    /// this is disjoint from the dictation hotkey.
    pub modifier_vk: u32,
    /// Resolved main-key VK code.
    pub main_vk: u32,
    /// Hard cap on meeting duration before forced stop.
    pub max_duration_seconds: u32,
    /// Default source preselected in the overlay.
    pub default_source: super::activation::LastChosenSource,
}

/// Long-lived owner of the meeting-capture subsystem.
///
/// Spawned once at app startup from `lib.rs::run`'s `.setup(...)`
/// callback (mirrors `DictationRuntime`). Drop tears down the meetings
/// thread + any in-flight capture worker.
#[derive(Debug)]
pub struct MeetingCaptureRuntime {
    /// Shared SQLite handle (WAL mode; safe to share across the
    /// activation-thread, capture-worker-thread, and IPC handlers).
    #[allow(dead_code)] // Wave 4: read in persist.
    shared_conn: Arc<Mutex<Connection>>,
    #[allow(dead_code)] // Wave 3: read by the activation thread.
    config: MeetingRuntimeConfig,
}

impl MeetingCaptureRuntime {
    /// Spawn the meetings message-pump thread (Wave 3) and return the
    /// owning runtime handle. Idempotent: calling twice in the same
    /// process panics (we don't support multiple meeting subsystems).
    ///
    /// Wave 1: returns `Ok(Self)` with no thread spawned. Wave 3
    /// spawns the activation thread + installs the second hook.
    pub fn spawn(
        shared_conn: Arc<Mutex<Connection>>,
        config: MeetingRuntimeConfig,
    ) -> AppResult<Self> {
        Ok(Self {
            shared_conn,
            config,
        })
    }
}

impl Drop for MeetingCaptureRuntime {
    fn drop(&mut self) {
        // Wave 3 posts WM_QUIT to the meetings message-pump thread
        // here; Wave 4 also joins the in-flight capture worker (if
        // any) with a 5 s timeout and marks the meeting status
        // 'interrupted' on Drop-without-clean-Stop.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_conn() -> Arc<Mutex<Connection>> {
        Arc::new(Mutex::new(Connection::open_in_memory().expect("mem db")))
    }

    fn dummy_config() -> MeetingRuntimeConfig {
        MeetingRuntimeConfig {
            modifier_vk: 0xA3, // VK_RCONTROL
            main_vk: 0x4D,     // 'M'
            max_duration_seconds: 14_400,
            default_source: super::super::activation::LastChosenSource::Mic,
        }
    }

    /// Wave 1 smoke: spawn-and-drop doesn't panic. Wave 3 replaces
    /// this with hook-install integration tests.
    #[test]
    fn spawn_smoke() {
        let _rt = MeetingCaptureRuntime::spawn(dummy_conn(), dummy_config()).expect("spawn");
    }
}
