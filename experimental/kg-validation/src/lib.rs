//! Mockingbird Knowledge Graph — Phase 0 validation harness.
//!
//! Sandboxed per [ADR 0048](../../../docs/adr/0048-knowledge-graph-phase-0-validation.md)
//! and spec §5. Deleting `experimental/kg-validation/` must leave the
//! production app completely untouched. See the crate README for the
//! full isolation contract.

pub mod harness;
pub mod judges;
pub mod ollama;
pub mod passes;
pub mod schema;
pub mod schema_loader;
pub mod scoring;
pub mod wiggum;
