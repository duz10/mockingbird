//! ID generators for the activity subsystem.
//!
//! Wave 1B uses UUID v4 (already a workspace dep). A future wave can
//! swap to ULID for lexicographic time-ordering on `activity_events.id`
//! if event-volume ever makes the secondary `(session_id, ts)` index
//! insufficient. The PK column is `TEXT` — the swap is a one-shot
//! function-body replacement here, no schema migration needed.
//!
//! Keeping the ID generators in their own tiny module rather than
//! inlining `uuid::Uuid::new_v4()` at the call sites means we have ONE
//! place to enforce that contract — and one place to add a `tracing::span!`
//! breadcrumb if we ever need to debug "where did this id come from".

use uuid::Uuid;

/// Generate a fresh session id. Format: lowercase UUID v4 string.
pub fn new_session_id() -> String {
    Uuid::new_v4().to_string()
}

/// Generate a fresh event id. Format: lowercase UUID v4 string.
pub fn new_event_id() -> String {
    Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_per_call() {
        let a = new_session_id();
        let b = new_session_id();
        assert_ne!(a, b);
    }

    #[test]
    fn event_and_session_ids_share_uuid_shape() {
        let s = new_session_id();
        let e = new_event_id();
        // 8-4-4-4-12 hex = 36 chars.
        assert_eq!(s.len(), 36);
        assert_eq!(e.len(), 36);
        assert!(s.chars().filter(|c| *c == '-').count() == 4);
    }
}
