//! Wiggum-style iteration-acceptance machinery.
//!
//! Named for the Ralph Wiggum loop pattern (Code Puppy's `/goal` flow):
//! iterate a candidate change against a baseline, accept-or-reject by a
//! deterministic protocol, advance the baseline only on accept.
//!
//! Only one submodule today; lives in its own crate-level mod so the
//! IAP can grow neighbours (e.g. judge-bundle dispatcher) without
//! reshuffling `lib.rs`.

pub mod iap;
