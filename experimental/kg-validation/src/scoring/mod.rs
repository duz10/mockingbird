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
//! - [`metrics`] — per-dictation, per-metric scoring against the
//!   answer key; uses [`judge`] for the tag-equivalence metric.
//! - [`judge_validation`] — JVP: five mechanical gates the judge
//!   must clear (Gate 1/2/3 STOP, Gate 4/5 WARN).
//! - [`persona_review`] — PCRP: structured qualitative audit
//!   composed as a single bounded LLM call per persona.

pub mod judge;
pub mod judge_validation;
pub mod metrics;
pub mod persona_review;
