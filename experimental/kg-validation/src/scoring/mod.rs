//! Wave 3 — per-metric scorer + LLM tag-equivalence judge + Judge
//! Validation Protocol (JVP) + Persona Cross-Reference Pass (PCRP).
//!
//! Spec §8.2–§8.5 (scoring + stability), §8.3 (judge), ADR 0048
//! §G4 (judge model family discipline), §G5 (JVP five gates), §G6
//! (PCRP discipline rules + go/no-go interaction).
//!
//! Module split:
//!
//! - [`judge`] — single LLM call: "are these two tag sets
//!   equivalent?" Strict reasoning-before-verdict parser.
//!   **Preserved for any future LLM-judged metric**; not invoked
//!   for tag-collapse under ADR 0048 §G7.
//! - [`tag_collapse`] — deterministic synonym-map + Jaccard scoring
//!   (ADR 0048 §G7). Supersedes the LLM judge for the tag-collapse
//!   metric only.
//! - [`metrics`] — per-dictation, per-metric scoring against the
//!   answer key; consumes [`tag_collapse`] for the tag metric.
//! - [`judge_validation`] — JVP: five mechanical gates the judge
//!   must clear. Preserved for future LLM-judged metrics; not
//!   invoked under §G7.
//! - [`persona_review`] — PCRP: structured qualitative audit
//!   composed as a single bounded LLM call per persona.

pub mod entity_quality;
pub mod judge;
pub mod judge_validation;
pub mod metrics;
pub mod persona_review;
pub mod tag_collapse;
