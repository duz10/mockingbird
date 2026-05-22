//! High-level orchestrators for the Wave-3 summarization pipeline +
//! export surfaces. The IPC commands in `commands/activity.rs` are
//! thin shims over these functions.
//!
//! ## What "regenerate the summary" actually does
//!
//! 1. Load the session detail (`persist::get_session_detail`).
//! 2. Run [`segmenter::normalize`] → [`blocker::to_blocks`].
//! 3. For each Block, look up an existing `activity_blocks` row by
//!    `started_at` proximity:
//!       - If a `user_edited = 1` row matches → keep it as-is
//!         (Principle: respect user edits across re-runs).
//!       - Otherwise → run the abstractor.
//! 4. Replace the session's Block rows with the freshly-generated
//!    set (preserving user edits per above).
//! 5. Run [`assembler::assemble`] → write to
//!    `activity_sessions.summary_markdown` + `prompt_set_sha`.
//!
//! ## Export targets
//!
//! - `export_to_file` writes the markdown to a path (the Tauri save
//!   dialog feeds it).
//! - `copy_to_clipboard` reuses the `meetings::clipboard` Win32
//!   helper (already production-tested across two phases).

use std::path::Path;

use rusqlite::Connection;

use crate::activity::{
    abstractor::{
        abstract_block_audio_aware_with_provider, abstract_block_with_provider,
        current_prompt_set_sha, current_prompt_set_sha_audio, AudioExcerpt, BlockAbstract,
        BlockAudioContext,
    },
    assembler::{assemble, assemble_work_report, AbstractedBlock},
    block_audio_stitcher::{stitch, BlockAudioBundle, StitchBlock},
    blocker::{to_blocks, Block},
    blocks_persist::{list_blocks, update_session_summary, ActivityBlockRow},
    persist::get_session_detail,
    segmenter::normalize,
    segments_persist::list_segments,
};
use crate::cleanup::ollama::OllamaProvider;
use crate::cleanup::provider::CleanupProvider;
use crate::error::{AppError, AppResult};
use crate::meetings::clipboard::copy_text_one_shot;

/// Regenerate the summary for one session. Uses the default
/// [`OllamaProvider`]; the `_with_provider` variant lets tests
/// inject a stub.
pub fn regenerate_summary(conn: &mut Connection, session_id: &str) -> AppResult<String> {
    let provider = OllamaProvider::new();
    regenerate_summary_with_provider(conn, session_id, &provider)
}

/// Test seam: regenerate with a caller-supplied provider.
pub fn regenerate_summary_with_provider(
    conn: &mut Connection,
    session_id: &str,
    provider: &dyn CleanupProvider,
) -> AppResult<String> {
    // 1. Load.
    let detail = get_session_detail(conn, session_id)?.ok_or_else(|| {
        AppError::ActivityPersist(format!("no such activity session: {session_id}"))
    })?;
    let session_ended_at = detail.session.ended_at;
    let session_started_at = detail.session.started_at;

    // 2. Pipeline stages 1 + 2.
    let normalized = normalize(&detail.events);
    let blocks = to_blocks(&normalized, session_ended_at);

    // 3. Abstract per Block, respecting prior user edits.
    let existing = list_blocks(conn, session_id)?;
    let prompt_set_sha = current_prompt_set_sha();
    let audio_prompt_sha = current_prompt_set_sha_audio();
    let now_ms = session_ended_at.unwrap_or(session_started_at);

    // Wave 4: load transcript segments (if any), stitch onto Blocks
    // via the midpoint rule (ADR 0041), and project to per-Block
    // audio context. Sessions with `audio_enabled = 0` produce an
    // empty segment list — the stitcher returns empty bundles —
    // and every Block routes through the v1 (no-audio) path.
    let segments = list_segments(conn, session_id)?;
    let stitcher_input: Vec<_> = segments
        .iter()
        .filter_map(|r| r.to_stitcher_input())
        .collect();
    let stitch_blocks: Vec<StitchBlock> = blocks
        .iter()
        .enumerate()
        .map(|(i, b)| StitchBlock {
            // The stitcher's `id` is only echoed back to us so a
            // simple index-based id is fine; we re-correlate via
            // the parallel Vec ordering below.
            id: i.to_string(),
            started_at: b.started_at,
            ended_at: b.ended_at,
        })
        .collect();
    let bundles = stitch(&stitch_blocks, &stitcher_input);
    let audio_contexts: Vec<BlockAudioContext> = bundles
        .iter()
        .zip(blocks.iter())
        .map(|(bundle, block)| bundle_to_context(bundle, block.started_at))
        .collect();

    let abstracted = abstract_blocks_respecting_user_edits(
        &blocks,
        &existing,
        &audio_contexts,
        provider,
        &prompt_set_sha,
        &audio_prompt_sha,
    );

    // 4. Replace block rows. Trade-off: we delete + re-insert rather
    //    than try to UPDATE in place. Reason: the blocker can change
    //    the *number* of blocks across re-runs (heuristic tuning,
    //    new events), so the row-identity-preserving path is more
    //    code for no real benefit at this stage. User-edited rows
    //    are preserved via [`abstract_blocks_respecting_user_edits`].
    //    Wave 4+ can revisit if we need stable Block ids for
    //    cross-run cross-referencing.
    let tx = conn.transaction()?;
    for row in &existing {
        // Within-transaction direct delete — `delete_block` would also
        // open a fresh statement; we just inline it.
        tx.execute(
            "DELETE FROM activity_blocks WHERE id = ?1",
            rusqlite::params![row.id],
        )?;
    }
    let mut total_duration_ms = 0_i64;
    for ab in &abstracted {
        let source_ids_json = serde_json::to_string(&ab.block.source_event_ids)
            .map_err(|e| AppError::ActivityPersist(format!("source_event_ids encode: {e}")))?;
        total_duration_ms += ab.block.duration_ms();
        // Inline insert + label/user_edited preservation when a prior
        // user edit matched.
        let new_id = crate::activity::ids::new_event_id();
        tx.execute(
            "INSERT INTO activity_blocks \
             (id, session_id, started_at, ended_at, primary_app, primary_title, \
              label, generated_abstract, user_edited, source_event_ids, \
              prompt_version_sha, created_at, updated_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
            rusqlite::params![
                new_id,
                session_id,
                ab.block.started_at,
                ab.block.ended_at,
                ab.block.primary_app,
                ab.block.primary_title,
                ab.label,
                ab.abstract_text,
                if ab.label.is_some() || ab.preserved_edit {
                    1
                } else {
                    0
                },
                source_ids_json,
                ab.prompt_version_sha,
                now_ms,
            ],
        )?;
    }
    tx.commit()?;

    // 5. Assemble + write summary.
    let session_total = session_ended_at.unwrap_or(session_started_at) - session_started_at;
    let title = format_session_title_ms(session_started_at);
    let _ = total_duration_ms; // available for debugging; not used in current title.
    let markdown = assemble(
        &title,
        session_total.max(0),
        &to_assembler_blocks(&abstracted),
    );
    update_session_summary(conn, session_id, &markdown, &prompt_set_sha, now_ms)?;
    Ok(markdown)
}

/// Build the work-report variant on demand (does NOT regenerate the
/// stored summary — caller has already called `regenerate_summary`
/// or has an existing one).
pub fn render_work_report(conn: &Connection, session_id: &str) -> AppResult<String> {
    let detail = get_session_detail(conn, session_id)?.ok_or_else(|| {
        AppError::ActivityPersist(format!("no such activity session: {session_id}"))
    })?;
    let rows = list_blocks(conn, session_id)?;
    let blocks_for_assembler: Vec<AbstractedBlock> =
        rows.iter().map(block_row_to_assembler_block).collect();
    let session_total =
        detail.session.ended_at.unwrap_or(detail.session.started_at) - detail.session.started_at;
    let title = format_session_title_ms(detail.session.started_at);
    Ok(assemble_work_report(
        &title,
        session_total.max(0),
        &blocks_for_assembler,
    ))
}

/// Write the stored `summary_markdown` to a destination path.
pub fn export_to_file(conn: &Connection, session_id: &str, dest: &Path) -> AppResult<()> {
    let body = load_summary_or_err(conn, session_id)?;
    std::fs::write(dest, body)?;
    Ok(())
}

/// Copy the stored `summary_markdown` to the system clipboard.
/// Reuses the meetings clipboard helper (no DRY violation — that
/// module's contract is "one-shot save/restore"; activity is the
/// second caller, which is precisely what `meetings::clipboard`
/// expected when the Wave 1A note said "this stays outside the
/// dictation injection module so other features can use it").
pub fn copy_to_clipboard(conn: &Connection, session_id: &str) -> AppResult<()> {
    let body = load_summary_or_err(conn, session_id)?;
    copy_text_one_shot(&body)
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

/// Decorated `AbstractedBlock` carrying an extra
/// `preserved_edit` flag so the writer knows whether to keep the
/// user_edited = 1 marker.
struct AbstractedBlockWithProvenance {
    block: Block,
    abstract_text: Option<String>,
    label: Option<String>,
    prompt_version_sha: String,
    preserved_edit: bool,
}

fn abstract_blocks_respecting_user_edits(
    blocks: &[Block],
    existing: &[ActivityBlockRow],
    audio_contexts: &[BlockAudioContext],
    provider: &dyn CleanupProvider,
    prompt_set_sha: &str,
    audio_prompt_sha: &str,
) -> Vec<AbstractedBlockWithProvenance> {
    debug_assert_eq!(
        blocks.len(),
        audio_contexts.len(),
        "audio_contexts must zip 1:1 with blocks; got {} blocks vs {} contexts",
        blocks.len(),
        audio_contexts.len()
    );
    let empty_audio = BlockAudioContext::default();
    let mut out = Vec::with_capacity(blocks.len());
    for (idx, b) in blocks.iter().enumerate() {
        // Match by `started_at` proximity (≤ 1s tolerance). This is
        // loose enough to survive heuristic-tuning re-runs that
        // shift Block boundaries by a few hundred ms.
        let prior_match = existing
            .iter()
            .find(|r| r.user_edited && (r.started_at - b.started_at).abs() <= 1_000);
        if let Some(prior) = prior_match {
            out.push(AbstractedBlockWithProvenance {
                block: b.clone(),
                abstract_text: prior.generated_abstract.clone(),
                label: prior.label.clone(),
                prompt_version_sha: prior.prompt_version_sha.clone(),
                preserved_edit: true,
            });
            continue;
        }
        // Wave 4: route through the audio-aware abstractor. The
        // implementation itself falls back to the v1 path when the
        // bundle is empty — so v1 sessions naturally re-acquire the
        // v1 fingerprint even though they're calling the audio-aware
        // entry point.
        let audio = audio_contexts.get(idx).unwrap_or(&empty_audio);
        let res: BlockAbstract = match abstract_block_audio_aware_with_provider(
            b,
            audio,
            provider,
            audio_prompt_sha,
            prompt_set_sha,
        ) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(
                    target: "activity::export",
                    error = %e,
                    "abstractor errored at the outer call boundary"
                );
                BlockAbstract {
                    text: String::new(),
                    prompt_version_sha: prompt_set_sha.to_string(),
                    used_llm: false,
                }
            }
        };
        out.push(AbstractedBlockWithProvenance {
            block: b.clone(),
            abstract_text: if res.text.trim().is_empty() {
                None
            } else {
                Some(res.text)
            },
            label: None,
            prompt_version_sha: res.prompt_version_sha,
            preserved_edit: false,
        });
    }
    out
}

/// Project a stitcher bundle to the abstractor's audio context.
/// Translates session-relative ms timestamps to Block-relative
/// seconds so the prompt's `[t+SS]` markers read naturally.
fn bundle_to_context(bundle: &BlockAudioBundle, block_started_at_ms: i64) -> BlockAudioContext {
    BlockAudioContext {
        mic_excerpts: bundle
            .mic_segments
            .iter()
            .map(|s| AudioExcerpt {
                offset_seconds: ((s.started_at - block_started_at_ms).max(0)) / 1_000,
                text: s.text.clone(),
            })
            .collect(),
        sys_excerpts: bundle
            .sys_segments
            .iter()
            .map(|s| AudioExcerpt {
                offset_seconds: ((s.started_at - block_started_at_ms).max(0)) / 1_000,
                text: s.text.clone(),
            })
            .collect(),
    }
}

#[allow(dead_code)] // killed by abstract_block_with_provider going through audio-aware path
fn _silence_unused_v1_abstract_path(
    b: &Block,
    provider: &dyn CleanupProvider,
    sha: &str,
) -> AppResult<BlockAbstract> {
    // Kept around as a single-call escape hatch in case a future
    // caller needs the no-audio path explicitly. Wave 4 routes
    // everything through the audio-aware path.
    abstract_block_with_provider(b, provider, sha)
}

fn to_assembler_blocks(src: &[AbstractedBlockWithProvenance]) -> Vec<AbstractedBlock> {
    src.iter()
        .map(|x| AbstractedBlock {
            block: x.block.clone(),
            abstract_text: x.abstract_text.clone(),
            label: x.label.clone(),
        })
        .collect()
}

fn block_row_to_assembler_block(row: &ActivityBlockRow) -> AbstractedBlock {
    // Reconstruct a thin `Block` from the row. The assembler only
    // reads started_at / ended_at / primary_app / primary_title / duration —
    // it doesn't need focus_events for the rendered markdown.
    AbstractedBlock {
        block: Block {
            started_at: row.started_at,
            ended_at: row.ended_at,
            primary_app: row.primary_app.clone(),
            primary_title: row.primary_title.clone(),
            source_event_ids: Vec::new(),
            focus_events: Vec::new(),
            idle_ms_within: 0,
        },
        abstract_text: row.generated_abstract.clone(),
        label: row.label.clone(),
    }
}

fn load_summary_or_err(conn: &Connection, session_id: &str) -> AppResult<String> {
    crate::activity::blocks_persist::get_session_summary(conn, session_id)?.ok_or_else(|| {
        AppError::ActivityPersist(format!(
            "session {session_id} has no summary yet — call regenerate_summary first"
        ))
    })
}

/// Cheap session-title format: ISO date-time without re-pulling
/// `chrono`. Same approach as the assembler's `format_clock`.
/// "YYYY-MM-DD HH:MM" by way of unix-epoch math.
fn format_session_title_ms(ts_ms: i64) -> String {
    // Reuse the days→Y/M/D helper that already lives in
    // `dictation::format_secs_as_iso` (Howard Hinnant). Pulling it
    // up to a shared util is a follow-up bead — for now, a
    // minimal local impl.
    let secs = ts_ms.max(0) / 1000;
    let days = secs / 86_400;
    let s_in_day = secs % 86_400;
    let hh = s_in_day / 3600;
    let mm = (s_in_day % 3600) / 60;
    let (y, m, d) = days_to_ymd(days);
    format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}")
}

/// Days since unix epoch → (year, month, day). Adapted from Howard
/// Hinnant's algorithm (used in `dictation::*` already). Valid for
/// any unix-epoch-positive timestamp.
fn days_to_ymd(days_since_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = z.div_euclid(146_097);
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::persist::{finalize_session, insert_event, insert_session, SessionStatus};
    use crate::cleanup::provider::{CleanupRequest, CleanupResult};
    use crate::db::migrations;

    struct AlwaysOkProvider;
    impl CleanupProvider for AlwaysOkProvider {
        fn cleanup(&self, _req: CleanupRequest<'_>) -> AppResult<CleanupResult> {
            Ok(CleanupResult {
                text: "Stub-generated summary.".into(),
                model_used: "stub".into(),
                latency_ms: 0,
                input_tokens: None,
                output_tokens: None,
            })
        }
        fn provider_name(&self) -> &'static str {
            "stub"
        }
        fn supports_model(&self, _: &str) -> bool {
            true
        }
    }

    fn fresh_db() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        migrations::apply_all(&c).unwrap();
        c
    }

    fn seed_session_with_two_blocks(conn: &mut Connection) -> String {
        let sid = insert_session(conn, 1_000_000).unwrap();
        // Block 1: code.exe for 10s
        insert_event(
            conn,
            &sid,
            1_000_000,
            "app_switch",
            Some("code.exe"),
            Some("main.rs"),
            None,
        )
        .unwrap();
        insert_event(
            conn,
            &sid,
            1_010_000,
            "app_switch",
            Some("chrome.exe"),
            Some("github"),
            None,
        )
        .unwrap();
        finalize_session(conn, &sid, 1_030_000, SessionStatus::Completed).unwrap();
        sid
    }

    #[test]
    fn regenerate_summary_produces_markdown_with_top_apps_section() {
        let mut c = fresh_db();
        let sid = seed_session_with_two_blocks(&mut c);
        let md = regenerate_summary_with_provider(&mut c, &sid, &AlwaysOkProvider).unwrap();
        assert!(md.contains("## Top apps"));
        assert!(md.contains("code.exe") || md.contains("chrome.exe"));
        // The session row should now have the summary cached.
        let stored = crate::activity::blocks_persist::get_session_summary(&c, &sid).unwrap();
        assert_eq!(stored.as_deref(), Some(md.as_str()));
    }

    #[test]
    fn regenerate_summary_preserves_user_edited_blocks() {
        let mut c = fresh_db();
        let sid = seed_session_with_two_blocks(&mut c);
        regenerate_summary_with_provider(&mut c, &sid, &AlwaysOkProvider).unwrap();

        // User renames the first block. Mark user_edited via the
        // rename_block API.
        let rows = list_blocks(&c, &sid).unwrap();
        let first = &rows[0];
        crate::activity::blocks_persist::rename_block(
            &c,
            &first.id,
            Some("My Renamed Block"),
            2_000_000,
        )
        .unwrap();

        // Regenerate again with a DIFFERENT stub that would output
        // different text — the user-edited row's label + abstract
        // should survive.
        struct OtherProvider;
        impl CleanupProvider for OtherProvider {
            fn cleanup(&self, _req: CleanupRequest<'_>) -> AppResult<CleanupResult> {
                Ok(CleanupResult {
                    text: "DIFFERENT SUMMARY".into(),
                    model_used: "stub2".into(),
                    latency_ms: 0,
                    input_tokens: None,
                    output_tokens: None,
                })
            }
            fn provider_name(&self) -> &'static str {
                "stub2"
            }
            fn supports_model(&self, _: &str) -> bool {
                true
            }
        }
        regenerate_summary_with_provider(&mut c, &sid, &OtherProvider).unwrap();

        let rows2 = list_blocks(&c, &sid).unwrap();
        let preserved = rows2
            .iter()
            .find(|r| r.label.as_deref() == Some("My Renamed Block"))
            .expect("user-edited block should survive regenerate");
        assert!(preserved.user_edited);
    }

    #[test]
    fn export_to_file_requires_a_prior_summary() {
        let mut c = fresh_db();
        let sid = seed_session_with_two_blocks(&mut c);
        let tmp = std::env::temp_dir().join("mb_activity_export_test.md");
        let _ = std::fs::remove_file(&tmp);
        // No regenerate yet — export should error.
        assert!(export_to_file(&c, &sid, &tmp).is_err());
        // After regenerate, it works.
        regenerate_summary_with_provider(&mut c, &sid, &AlwaysOkProvider).unwrap();
        export_to_file(&c, &sid, &tmp).unwrap();
        let body = std::fs::read_to_string(&tmp).unwrap();
        assert!(body.contains("## Top apps"));
        let _ = std::fs::remove_file(&tmp);
    }

    #[test]
    fn days_to_ymd_matches_known_epoch_dates() {
        // 0 days since epoch == 1970-01-01.
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
        // 2026-05-25 is day 20_598 (computed externally).
        let (y, m, d) = days_to_ymd(20_598);
        assert_eq!((y, m, d), (2026, 5, 25));
    }

    #[test]
    fn render_work_report_returns_bullets() {
        let mut c = fresh_db();
        let sid = seed_session_with_two_blocks(&mut c);
        regenerate_summary_with_provider(&mut c, &sid, &AlwaysOkProvider).unwrap();
        let report = render_work_report(&c, &sid).unwrap();
        assert!(report.contains("## Highlights"));
        assert!(report.contains("- Stub-generated summary."));
    }
}
