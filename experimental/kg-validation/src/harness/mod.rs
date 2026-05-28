//! Wave 2 — harness binary, corpus walker, pipeline driver.
//!
//! Two-layer split:
//!
//! - [`pipeline`] — runs the four passes for a single dictation,
//!   persists each pass's raw + parsed output, returns the assembled
//!   `Vec<Entry>`.
//! - [`runner`] — walks the corpus directory, dispatches each
//!   dictation through [`pipeline`], and writes a per-run
//!   `SUMMARY.json`.
//!
//! The binary `run-corpus` (in `src/bin/`) is a thin CLI shell over
//! [`runner::run_corpus`].

pub mod pipeline;
pub mod runner;
