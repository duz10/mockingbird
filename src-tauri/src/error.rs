//! Top-level application error type.
//!
//! Every fallible operation in `mockingbird_lib` returns
//! `Result<T, AppError>`. Module-specific error variants are added
//! here as the modules land in later Phase-1 waves.

use thiserror::Error;

/// The single error type returned from public API surfaces.
///
/// Variants are added per module as Phase 1 progresses. Keeping them
/// concentrated in one enum (rather than per-module sub-errors that
/// nest via `#[from]`) trades a slight loss of granularity for a much
/// simpler public surface and a single conversion point at the Tauri
/// command boundary.
#[derive(Error, Debug)]
pub enum AppError {
    /// Wrap a `std::io::Error`.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Wrap a Tauri-side error.
    #[error("tauri error: {0}")]
    Tauri(#[from] tauri::Error),

    /// Generic catch-all for early Phase 1; replaced by typed variants
    /// as concrete modules surface their errors.
    #[error("{0}")]
    Other(String),
}

/// Convenience alias.
pub type AppResult<T> = std::result::Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_error_round_trips_via_from() {
        let inner = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let err: AppError = inner.into();
        assert!(matches!(err, AppError::Io(_)));
    }

    #[test]
    fn other_displays_payload() {
        let err = AppError::Other("explanation".to_string());
        assert_eq!(err.to_string(), "explanation");
    }
}
