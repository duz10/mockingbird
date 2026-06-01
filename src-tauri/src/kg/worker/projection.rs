//! Vault projection phase (ADR 0053 §D4, steps 2 → 4).
//!
//! Owns:
//! - [`maybe_commit_to_vault`] -- builds the in-memory
//!   [`crate::vault::markdown_serializer::KgEntry`] from a
//!   [`PipelineResult`] + session row, then runs
//!   [`commit_entry_to_vault`].
//! - [`pick_vault_body`] -- selects the body bytes for the vault
//!   file (full cleaned transcript preferred over the segmenter's
//!   first-entry body; see `mb-wzui`).
//! - The KG → vault enum bridge functions
//!   ([`kg_entry_type_to_vault`] etc.), including the legacy
//!   `task`/`idea` downgrade sidecar enforced by hotfix `mb-wzcj`
//!   (LESSONS PINNED P15).
//! - Permissive ISO-8601 parsers used to populate the entry's
//!   `captured_at` / `due_date` fields.
//!
//! Split out of `worker.rs` during Wave 1E.7 Part 2 (`mb-5lla`).
//! Behaviour is unchanged.

use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::db::sessions::{self as db_sessions, CaptureKind as DbCaptureKind};
use crate::error::{AppError, AppResult};
use crate::settings::{model::SettingKey, Settings};
use crate::vault::markdown_serializer::{
    CaptureKind as VaultCaptureKind, Category as VaultCategory, EntryType as VaultEntryType,
    KgEntry, Status as VaultStatus,
};
use crate::vault::writer::{commit_entry_to_vault, CommitOutcome};

use super::super::pipeline::PipelineResult;
use super::super::schema::{
    Category as KgCategory, Entry as KgPipelineEntry, EntryType as KgEntryType, Status as KgStatus,
};
use super::transcripts::load_dictation_text;

/// Vault projection step (ADR 0053 §D4, steps 2 → 4): build the
/// in-memory [`KgEntry`] from the pipeline result + session row,
/// then run [`commit_entry_to_vault`].
///
/// Returns:
///   - `Ok(Some(outcome))` -- file successfully landed on disk;
///     caller seals (step 5) + marks queue done (step 6).
///   - `Ok(None)` -- vault projection skipped (no vault configured,
///     capture_kind is not a KG kind, pipeline produced no entries,
///     or KG toggle flipped off mid-run); caller marks queue done
///     without sealing.
///   - `Err(e)` -- two-phase commit failed mid-flight (rare; the
///     row is in a reconcile signature). Caller marks queue done
///     anyway since the kg_* rows are durable.
///
/// Why we still mark queue done on failure: the kg_filing_queue's
/// job is "the LLM pipeline has run for this session". The vault
/// projection is a downstream artefact whose own queue-of-sorts is
/// `sessions.vault_path IS NULL`. Conflating the two would tie the
/// LLM pipeline retry budget (3 attempts) to the file-write retry
/// budget (infinite via reconcile), and the reverse-watcher
/// (1E.5) can't tell which is broken from a `failed` queue row.
pub(super) fn maybe_commit_to_vault(
    conn: &Arc<Mutex<Connection>>,
    session_id: i64,
    result: &PipelineResult,
    captured_iso: &str,
) -> AppResult<Option<CommitOutcome>> {
    // Snapshot the session row + settings + transcript under a
    // single lock so we don't have to round-trip the mutex four
    // times. The transcript snapshot is what we use for the entry
    // body (`mb-wzui` fix) -- pre-1E.4 we wrote `entries[0].body`
    // which is one segment of the segmenter's output, so any KG
    // session that produced N>1 segments (bullet lists, multi-fact
    // notes) silently dropped segments 1..N from the vault file.
    let snapshot = {
        let c = conn
            .lock()
            .map_err(|_| AppError::Other("db mutex poisoned in maybe_commit_to_vault".into()))?;
        // Re-check the KG toggle defensively. A flip from on -> off
        // while a pipeline is mid-run shouldn't produce a file.
        if !Settings::new(&c)
            .get::<bool>(SettingKey::KgGraphEnabled)
            .unwrap_or(false)
        {
            return Ok(None);
        }
        let vault_path: Option<String> = Settings::new(&c)
            .get::<Option<String>>(SettingKey::VaultPath)
            .ok()
            .flatten();
        let session = db_sessions::get_by_id(&c, session_id)?;
        // final -> cleaned -> raw cascade; same precedence as the
        // Dictations view + history archive (P0 user-visible text).
        let transcript = load_dictation_text(&c, session_id)?;
        (vault_path, session, transcript)
    };
    let (vault_path_opt, session_opt, transcript_opt) = snapshot;

    let vault_path_str = match vault_path_opt {
        Some(s) if !s.trim().is_empty() => s,
        _ => return Ok(None), // not configured -- skip projection silently
    };
    let vault_root = std::path::PathBuf::from(&vault_path_str);

    let session = session_opt.ok_or_else(|| {
        AppError::Other(format!(
            "maybe_commit_to_vault: session id={session_id} disappeared mid-run"
        ))
    })?;

    // Only KG captures get a vault projection in 1E.3. Standard
    // dictation rows stay DB-only (Phase 1E ADR 0053 explicit).
    let vault_capture_kind = match session.capture_kind {
        DbCaptureKind::KgNote => VaultCaptureKind::KgNote,
        DbCaptureKind::KgNoteText => VaultCaptureKind::KgNoteText,
        DbCaptureKind::Dictation => return Ok(None),
    };

    // Pick the canonical Entry to project. v1 maps one session to
    // one markdown file (1:1) using entries[0] as the headline +
    // unioning tags/entities across the run. The multi-entry case
    // (rambly KG-note that produces 3 distinct facts) is filed as
    // mb-1E3-multi-entry P3 -- it produces one file with the
    // first entry's classification and the other entries' tags
    // merged in, which loses some structure but never loses bytes.
    let primary: &KgPipelineEntry = match result.entries.first() {
        Some(e) => e,
        None => return Ok(None), // nothing to project
    };

    // Union tags + entities across all surviving entries / segment
    // entity outputs so a multi-entry session's projection is not
    // silently lossy on the metadata axis (per mb-1E3-multi-entry).
    let mut all_tags: Vec<String> = Vec::new();
    let mut tag_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for e in &result.entries {
        for t in &e.topic_tags {
            if tag_seen.insert(t.clone()) {
                all_tags.push(t.clone());
            }
        }
    }
    let mut all_entities: Vec<String> = Vec::new();
    let mut ent_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for seg in &result.segment_entities {
        for ent in &seg.entities {
            if ent_seen.insert(ent.name.clone()) {
                all_entities.push(ent.name.clone());
            }
        }
    }

    // Bridge classifier-side EntryType -> vault-side EntryType.
    // Legacy variants (`task`/`idea`) downgrade with a `tracing::warn!`
    // so we can confirm-and-remove the bridge once `mb-qw7n` realigns
    // the classifier prompt. LESSONS PINNED P15.
    let (vault_entry_type, legacy_downgrade) = kg_entry_type_to_vault(primary.entry_type);
    if let Some(legacy_wire) = legacy_downgrade {
        let mapped_to = match legacy_wire {
            "task" => "note",
            "idea" => "observation",
            // unreachable today; defensive label keeps the warn
            // honest if a future arm forgets to update the match.
            _ => "<unmapped>",
        };
        tracing::warn!(
            target: "kg::worker::vocab_bridge",
            session_id,
            legacy_value = legacy_wire,
            mapped_to,
            "classifier emitted legacy entry-type value; downgrading to canonical knowledge shape (deployment-ordering bridge until mb-qw7n classifier realignment ships; LESSONS P15)"
        );
    }

    let entry = KgEntry {
        id: uuid::Uuid::new_v4().to_string(),
        captured_at: parse_iso_to_utc(captured_iso),
        captured_at_local_date: parse_iso_to_local_date(captured_iso),
        capture_kind: vault_capture_kind,
        title: primary.title.clone(),
        category: kg_category_to_vault(primary.category),
        entry_type: vault_entry_type,
        status: primary.status.map(kg_status_to_vault),
        due_date: primary.due_iso.as_deref().and_then(parse_iso_to_utc_opt),
        tags: all_tags,
        entities: all_entities,
        source_session_uuid: Some(session.uuid.clone()),
        // Body = full cleaned transcript, NOT `entries[0].body`
        // (which is only segment[0] of the segmenter's output --
        // see `mb-wzui` for the bug this fixes). The cleaned
        // transcript is what the Dictations view shows to the
        // user; the vault projection must round-trip the same
        // bytes so multi-bullet notes don't silently lose items.
        //
        // Fallback to `primary.body` when no transcript row exists
        // (defensive; should never happen for KG captures because
        // the dictation/ingest_text persistence layer always writes
        // transcripts before enqueueing). Multi-entry filing is
        // tracked separately as `mb-ng1o`; until that ships, 1:1
        // session->file means embedding the full transcript here
        // doesn't duplicate anything.
        body: pick_vault_body(transcript_opt.as_deref(), &primary.body),
    };

    // Snapshot of mutex traffic: writer takes its own &Connection
    // borrows (each UPDATE auto-commits) so we can drop the lock
    // around the file write.
    let outcome = {
        let c = conn
            .lock()
            .map_err(|_| AppError::Other("db mutex poisoned in maybe_commit_to_vault".into()))?;
        commit_entry_to_vault(&c, session_id, &entry, &vault_root)?
    };
    Ok(Some(outcome))
}

// ── KG → vault enum mappings ────────────────────────────
//
// The KG pipeline's vocabulary (5 entry types: Task/Research/Idea/
// Note/Reference; 3 categories; 3 statuses) and the vault markdown
// vocabulary (5 entry types: Note/Task/Idea/Question/Decision; 3
// categories; 3 statuses) drifted apart between Wave 1A (KG schema)
// and Wave 1E.2 (vault serializer). The two sets agree on
// Category and Status but DON'T fully agree on EntryType.
//
// Filed mb-1E3-vocab-drift (P3) to reconcile via a follow-up ADR.
// For 1E.3 we lossy-map the mismatched cases to Note (the
// catch-all). This is durable -- the original kg_entries.entry_type
// is still in the DB; only the vault projection is lossy.

fn kg_category_to_vault(c: KgCategory) -> VaultCategory {
    match c {
        KgCategory::Personal => VaultCategory::Personal,
        KgCategory::Professional => VaultCategory::Professional,
        KgCategory::Objective => VaultCategory::Objective,
    }
}

fn kg_status_to_vault(s: KgStatus) -> VaultStatus {
    match s {
        KgStatus::Todo => VaultStatus::Todo,
        KgStatus::Doing => VaultStatus::Doing,
        KgStatus::Done => VaultStatus::Done,
    }
}

/// Map the classifier's 5-variant `KgEntryType` onto the writer's
/// 9-shape `VaultEntryType`. Returns the target shape PLUS an
/// optional `Some(legacy_wire_label)` sidecar when the input was a
/// dropped-from-canonical-set legacy value (`task` / `idea`).
/// Callers emit a `tracing::warn!` carrying that label so the
/// downgrade is observable — the load-bearing fix from hotfix
/// `mb-wzcj` (LESSONS PINNED P15): writer-side permissiveness during
/// a vocabulary realignment must MATCH the reverse-watcher parser's
/// existing tolerance, not be silently stricter.
///
/// KG-side has {Task, Research, Idea, Note, Reference}; vault-side
/// has the nine knowledge shapes from ADR 0054 §G: {Source, Note,
/// Concept, Entity, Project, Question, Decision, Reference,
/// Observation}.
///
/// The classifier prompt realignment (`mb-qw7n`) is a separate
/// dispatch — until that ships, the KG pipeline still emits the
/// legacy 5-variant set, so this mapping is the migration bridge.
/// Per-variant behavior:
///
/// - `Reference` → `Reference` — pass-through, identical shape, silent.
/// - `Note` → `Note` — pass-through, identical shape, silent.
/// - `Research` → `Reference` — cross-cutting structural mapping
///   (research notes point to external material being studied; closer
///   to Reference than Note). Not in the parser's legacy-tolerance
///   set, so we keep this silent.
/// - `Task` → `Note` — LEGACY-DOWNGRADE. Task semantics dropped per
///   ADR 0054 §G; the parser also tolerates legacy `task` by
///   re-classifying as Note. Emits warn at call site.
/// - `Idea` → `Observation` — LEGACY-DOWNGRADE. An idea is the
///   inchoate noticing of a pattern; closest knowledge shape is
///   Observation, which the chat-LLM Lint pass can crystallize into
///   a Concept page later. The parser also tolerates legacy `idea`.
///   Emits warn at call site.
///
/// Lossy in the `Task` → `Note` direction (`mb-il83` was originally
/// filed against this gap; ADR 0054 closes the bead by redefining
/// the canonical set rather than by reconciling 5-variant Task).
///
/// Removal path: once `mb-qw7n` realigns the classifier prompt to
/// emit only the nine canonical shapes directly, the `Task` / `Idea`
/// arms become unreachable and a follow-up wave can collapse this
/// function back to a 3-arm pass-through. Until then the legacy arms
/// are the deployment-ordering bridge per P15.
fn kg_entry_type_to_vault(t: KgEntryType) -> (VaultEntryType, Option<&'static str>) {
    match t {
        KgEntryType::Task => (VaultEntryType::Note, Some("task")),
        KgEntryType::Idea => (VaultEntryType::Observation, Some("idea")),
        KgEntryType::Note => (VaultEntryType::Note, None),
        KgEntryType::Research => (VaultEntryType::Reference, None),
        KgEntryType::Reference => (VaultEntryType::Reference, None),
    }
}

/// Permissive ISO-8601 parser that returns a chrono `DateTime<Utc>`.
/// On parse failure falls back to the Unix epoch -- the worker
/// already logs the row context, and the vault projection is
/// non-critical-path, so we don't propagate a typed error here.
fn parse_iso_to_utc(iso: &str) -> chrono::DateTime<chrono::Utc> {
    parse_iso_to_utc_opt(iso).unwrap_or(chrono::DateTime::<chrono::Utc>::UNIX_EPOCH)
}

fn parse_iso_to_utc_opt(iso: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(iso)
        .ok()
        .map(|dt| dt.with_timezone(&chrono::Utc))
}

/// Best-effort local-date extraction from an ISO-8601 string. Used
/// only to drive the markdown filename's `YYYY-MM-DD` prefix.
/// Mirrors `parse_iso_to_utc`'s defensive posture.
fn parse_iso_to_local_date(iso: &str) -> chrono::NaiveDate {
    parse_iso_to_utc_opt(iso)
        .map(|dt| {
            use chrono::TimeZone as _;
            chrono::Local
                .from_utc_datetime(&dt.naive_utc())
                .date_naive()
        })
        .unwrap_or_else(|| {
            chrono::NaiveDate::from_ymd_opt(1970, 1, 1).expect("1970-01-01 is a valid date")
        })
}

/// Pick the body for the vault projection. Prefers the full cleaned
/// transcript (what the user sees in the Dictations view); falls
/// back to the pipeline's first-entry-body only when no transcript
/// row exists at all.
///
/// Extracted as a free function so the body-selection rule is
/// trivially unit-testable without standing up a Connection +
/// filesystem. See `mb-wzui` for the bug this fixes (multi-bullet
/// KG notes were dropping segments 1..N from the vault file because
/// `entries[0].body` is just one segment of the segmenter's output).
///
/// Whitespace-only transcripts fall through to the segment fallback
/// on the theory that an empty transcript is a data-loss signal we
/// shouldn't propagate into the vault file. (The serializer would
/// happily emit a body-less file -- defensible -- but losing the
/// only available content would be worse.)
fn pick_vault_body(transcript: Option<&str>, fallback_segment: &str) -> String {
    match transcript {
        Some(t) if !t.trim().is_empty() => t.to_string(),
        _ => fallback_segment.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_vault_body_prefers_cleaned_transcript_over_segment_zero() {
        // The bug `mb-wzui` fixed: the segmenter slices a bulleted
        // list into N segments, so `entries[0].body` is just the
        // first bullet. The cleaned transcript is the full list.
        let cleaned = "Need to make a quick grocery list. I need to get:\n\
                       - feta cheese\n\
                       - eggs\n\
                       - milk";
        let seg0 = "Need to make a quick grocery list. I need to get: feta cheese";
        let body = pick_vault_body(Some(cleaned), seg0);
        assert_eq!(body, cleaned);
        // The bug symptom -- segment[0] surviving alone -- must not
        // recur. Pinning the negation explicitly.
        assert!(
            body.contains("eggs") && body.contains("milk"),
            "body must contain all bullets, got: {body}"
        );
    }

    #[test]
    fn pick_vault_body_falls_back_when_transcript_missing() {
        let seg0 = "the segment body";
        assert_eq!(pick_vault_body(None, seg0), seg0);
    }

    #[test]
    fn pick_vault_body_falls_back_when_transcript_is_whitespace() {
        // Defensive: a transcripts row that exists but only carries
        // whitespace would otherwise produce a body-less vault file
        // even though the segmenter saw real content.
        let seg0 = "useful fallback";
        assert_eq!(pick_vault_body(Some("   \n\t \n"), seg0), seg0);
    }

    #[test]
    fn pick_vault_body_round_trips_markdown_bullets_verbatim() {
        // The serializer trims trailing newlines but preserves the
        // body content -- so the body we pass MUST be the source of
        // truth, not a lossy summary.
        let cleaned = "- one\n- two\n- three\n";
        let body = pick_vault_body(Some(cleaned), "one");
        assert!(body.contains("- one"));
        assert!(body.contains("- two"));
        assert!(body.contains("- three"));
    }

    // ──────────────────────────────────────────────────────────
    // Hotfix mb-wzcj (LESSONS PINNED P15) -- vocabulary bridge
    //
    // The classifier emits the legacy 5-variant set
    // {Task, Research, Idea, Note, Reference}; the vault writer
    // expects the canonical 9-shape set per ADR 0054 §G. Until
    // `mb-qw7n` realigns the classifier prompt, the worker bridges
    // them via `kg_entry_type_to_vault`. The two legacy values the
    // reverse-watcher parser ALSO tolerates (`task`, `idea`) must
    // round-trip with an observable `Some(legacy_wire)` sidecar so
    // the call site can emit a `tracing::warn!` -- writer-side
    // permissiveness must match parser tolerance during the
    // realignment window.
    // ──────────────────────────────────────────────────────────

    #[test]
    fn kg_entry_type_to_vault_downgrades_task_to_note_with_legacy_label() {
        // The user-observable bug from the 2026-06-06 KG filing
        // failure: classifier labels content as `task`; pre-hotfix
        // the worker silently mapped Task->Note with no warn;
        // post-hotfix the sidecar `Some("task")` lights up the warn
        // path at the call site (`maybe_commit_to_vault`).
        let (target, legacy) = kg_entry_type_to_vault(KgEntryType::Task);
        assert_eq!(
            target,
            VaultEntryType::Note,
            "task downgrades to note (parser symmetry)"
        );
        assert_eq!(
            legacy,
            Some("task"),
            "task downgrade must surface a Some(_) sidecar so the call site can warn"
        );
    }

    #[test]
    fn kg_entry_type_to_vault_downgrades_idea_to_observation_with_legacy_label() {
        // `idea` is in the parser's legacy-tolerance set alongside
        // `task`. ADR 0054 §G maps it to Observation (the inchoate
        // pattern-noticing shape that the chat-LLM Lint pass can
        // crystallize into a Concept page later).
        let (target, legacy) = kg_entry_type_to_vault(KgEntryType::Idea);
        assert_eq!(target, VaultEntryType::Observation);
        assert_eq!(legacy, Some("idea"));
    }

    #[test]
    fn kg_entry_type_to_vault_passes_canonical_shapes_through_silently() {
        // Note, Reference -> identical canonical shape, no warn.
        // Research is mapped (not in parser's legacy set) but
        // structurally cross-cutting, so also silent.
        let (target, legacy) = kg_entry_type_to_vault(KgEntryType::Note);
        assert_eq!(target, VaultEntryType::Note);
        assert_eq!(legacy, None, "canonical Note must not trigger warn");

        let (target, legacy) = kg_entry_type_to_vault(KgEntryType::Reference);
        assert_eq!(target, VaultEntryType::Reference);
        assert_eq!(legacy, None, "canonical Reference must not trigger warn");

        let (target, legacy) = kg_entry_type_to_vault(KgEntryType::Research);
        assert_eq!(target, VaultEntryType::Reference);
        assert_eq!(
            legacy, None,
            "Research is a structural cross-cutting mapping (not in parser legacy set); silent"
        );
    }

    #[test]
    fn kg_entry_type_to_vault_is_total_for_every_classifier_variant() {
        // Defense-in-depth: if a future classifier-side variant is
        // added without updating the bridge, this test catches it
        // (the match would no longer compile -- the assertion is
        // structural, not value-based).
        for v in [
            KgEntryType::Task,
            KgEntryType::Research,
            KgEntryType::Idea,
            KgEntryType::Note,
            KgEntryType::Reference,
        ] {
            // Just exercising the function; the per-variant
            // assertions live in the tests above. The purpose here
            // is the exhaustive list -- if a 6th variant lands and
            // the bridge isn't extended, the new variant won't
            // appear here and downstream behavior will silently
            // fall back to the default arm (which we keep as a
            // compile error by NOT having a wildcard `_ =>` arm in
            // `kg_entry_type_to_vault`).
            let _ = kg_entry_type_to_vault(v);
        }
    }
}
