//! Windows Task Scheduler integration via `schtasks.exe`.
//!
//! Per PLAN §10 Phase 8: nightly job at 2 AM that runs the `learn`
//! binary. We don't link the Task Scheduler COM API (huge surface);
//! `schtasks.exe` is a stable, documented command-line interface
//! that ships with every Windows install.
//!
//! ## Behaviour
//!
//! - `install_task(binary_path)` — registers (or updates) a daily
//!   2 AM task named `MockingbirdLearningLoop`. Replaces any existing
//!   task with the same name.
//! - `uninstall_task()` — removes the task. Idempotent.
//! - `is_installed()` — reports whether the task exists.
//!
//! ## Why a trait
//!
//! `WinTaskScheduler` shells out; unit tests use `RecordingScheduler`
//! to assert the command-build logic without actually mutating the
//! user's task list. Avoids "did I just register a task on the dev
//! box every time the test runs?"

use crate::error::{AppError, AppResult};

/// Stable name we use for the scheduled task. Single tasks per user.
pub const TASK_NAME: &str = "MockingbirdLearningLoop";

/// Default schedule — daily at 02:00. Picked per PLAN §10.
pub const DEFAULT_SCHEDULE: &str = "DAILY";
/// Default start time.
pub const DEFAULT_START_TIME: &str = "02:00";

/// Cross-impl trait so tests don't shell out.
pub trait Scheduler {
    /// Install (or replace) the nightly task. `binary_path` is the
    /// absolute path to the `learn.exe` (or `mockingbird.exe learn`)
    /// to invoke.
    fn install(&mut self, binary_path: &str) -> AppResult<()>;

    /// Remove the task if present. Idempotent.
    fn uninstall(&mut self) -> AppResult<()>;

    /// `true` iff the task currently exists.
    fn is_installed(&self) -> AppResult<bool>;
}

/// `schtasks.exe`-backed impl. Windows-only.
#[cfg(target_os = "windows")]
pub struct WinTaskScheduler;

#[cfg(target_os = "windows")]
impl Scheduler for WinTaskScheduler {
    fn install(&mut self, binary_path: &str) -> AppResult<()> {
        // /F = force replace if present.
        let status = std::process::Command::new("schtasks.exe")
            .args([
                "/Create",
                "/F",
                "/TN",
                TASK_NAME,
                "/SC",
                DEFAULT_SCHEDULE,
                "/ST",
                DEFAULT_START_TIME,
                "/TR",
                binary_path,
                "/RL",
                "LIMITED",
            ])
            .status()
            .map_err(|e| AppError::Other(format!("schtasks install spawn: {e}")))?;
        if !status.success() {
            return Err(AppError::Other(format!(
                "schtasks install exited with {status}"
            )));
        }
        Ok(())
    }

    fn uninstall(&mut self) -> AppResult<()> {
        let status = std::process::Command::new("schtasks.exe")
            .args(["/Delete", "/F", "/TN", TASK_NAME])
            .status()
            .map_err(|e| AppError::Other(format!("schtasks uninstall spawn: {e}")))?;
        // Exit code 1 is "task not found" — treat as success
        // (idempotent uninstall).
        if !status.success() && status.code() != Some(1) {
            return Err(AppError::Other(format!(
                "schtasks uninstall exited with {status}"
            )));
        }
        Ok(())
    }

    fn is_installed(&self) -> AppResult<bool> {
        let output = std::process::Command::new("schtasks.exe")
            .args(["/Query", "/TN", TASK_NAME])
            .output()
            .map_err(|e| AppError::Other(format!("schtasks query spawn: {e}")))?;
        Ok(output.status.success())
    }
}

/// Test-only scheduler that records what would have happened.
pub struct RecordingScheduler {
    /// True after `install` has been called at least once.
    pub installed: bool,
    /// Last `binary_path` passed to `install`.
    pub last_binary: Option<String>,
    /// Count of `install` calls.
    pub install_calls: u32,
    /// Count of `uninstall` calls.
    pub uninstall_calls: u32,
}

impl Default for RecordingScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl RecordingScheduler {
    /// Empty starting state.
    pub fn new() -> Self {
        Self {
            installed: false,
            last_binary: None,
            install_calls: 0,
            uninstall_calls: 0,
        }
    }
}

impl Scheduler for RecordingScheduler {
    fn install(&mut self, binary_path: &str) -> AppResult<()> {
        self.installed = true;
        self.last_binary = Some(binary_path.to_string());
        self.install_calls += 1;
        Ok(())
    }
    fn uninstall(&mut self) -> AppResult<()> {
        self.installed = false;
        self.uninstall_calls += 1;
        Ok(())
    }
    fn is_installed(&self) -> AppResult<bool> {
        Ok(self.installed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_install_then_uninstall_is_observable() {
        let mut s = RecordingScheduler::new();
        assert!(!s.is_installed().unwrap());
        s.install("C:\\foo\\learn.exe").unwrap();
        assert!(s.is_installed().unwrap());
        assert_eq!(s.last_binary.as_deref(), Some("C:\\foo\\learn.exe"));
        s.uninstall().unwrap();
        assert!(!s.is_installed().unwrap());
        assert_eq!(s.install_calls, 1);
        assert_eq!(s.uninstall_calls, 1);
    }

    #[test]
    fn install_replaces_last_binary() {
        let mut s = RecordingScheduler::new();
        s.install("v1.exe").unwrap();
        s.install("v2.exe").unwrap();
        assert_eq!(s.last_binary.as_deref(), Some("v2.exe"));
        assert_eq!(s.install_calls, 2);
    }

    #[test]
    fn task_name_and_schedule_are_stable() {
        // Pin so renames are intentional + caught by CI.
        assert_eq!(TASK_NAME, "MockingbirdLearningLoop");
        assert_eq!(DEFAULT_SCHEDULE, "DAILY");
        assert_eq!(DEFAULT_START_TIME, "02:00");
    }

    /// Live test — actually installs + uninstalls a Windows task.
    /// `#[ignore]`d so CI on non-admin runners doesn't fail.
    #[test]
    #[cfg(target_os = "windows")]
    #[ignore = "actually mutates Windows Task Scheduler"]
    fn live_install_uninstall_round_trip() {
        let mut s = WinTaskScheduler;
        s.install("cmd.exe /c echo mockingbird-test-task").unwrap();
        assert!(s.is_installed().unwrap());
        s.uninstall().unwrap();
        assert!(!s.is_installed().unwrap());
    }
}
