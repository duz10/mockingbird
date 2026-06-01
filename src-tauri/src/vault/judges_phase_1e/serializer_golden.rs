//! J4 — `kg-serializer-golden-roundtrip` (Wave 1E.9 / `mb-kazi`).
//!
//! Asserts: `parse_entry(serialize_entry(e))` -> `e` for every
//! shipped golden fixture, AND `serialize_entry(parse_entry(disk_bytes).entry)`
//! is byte-identical to `disk_bytes` for the same set.
//!
//! Why both directions:
//!
//! - The first direction (serialize -> parse -> compare) is what the
//!   existing `proptest` tests cover in `markdown_serializer_tests.rs`
//!   under `cargo test --release` -- which is blocked on this box
//!   (LESSONS P2). The probe form runs at seal time without the
//!   test runner.
//! - The second direction (parse -> serialize -> bytes) is what the
//!   reverse-watcher (Wave 1E.5) relies on: when a user edits a file
//!   in Obsidian and the file's YAML structurally matches our output,
//!   re-projecting it MUST produce the same bytes -- otherwise the
//!   pre-recorded `vault_file_hash` won't loop-prevent and we'd
//!   ping-pong forever. J1 catches the in-process behavior; J4
//!   catches the underlying serializer drift that would make J1 fail.
//!
//! Goldens covered (all seven shipped under
//! `src-tauri/tests/fixtures/markdown_golden/`):
//!
//! - `minimal.md` -- bare-minimum required frontmatter only.
//! - `full_task.md` -- task with status + due_date.
//! - `doing_task.md` -- in-progress task.
//! - `done_task.md` -- completed task.
//! - `special_chars.md` -- frontmatter scalar quoting / escaping.
//! - `wiki_linked_entities.md` -- pipe-aliased `[[Entities/x|x]]`.
//! - `knowledge_shape_diversity.md` -- nine-knowledge-shape vocab
//!   (ADR 0054 §G). The extended vocab amendment from Wave 1E.7.
//!
//! Spec: `docs/phases/phase-1e.md` §"Wave 1E.9" + §FRAMING-UPDATED.

use crate::vault::markdown_parser::parse_entry;
use crate::vault::markdown_serializer::serialize_entry;

/// One golden fixture: a stable name (for failure reporting) +
/// the bytes loaded via `include_str!` at compile time so the
/// probe binary doesn't need to know where the workspace is at
/// runtime.
struct Golden {
    name: &'static str,
    bytes: &'static str,
}

const GOLDENS: &[Golden] = &[
    Golden {
        name: "minimal",
        bytes: include_str!("../../../tests/fixtures/markdown_golden/minimal.md"),
    },
    Golden {
        name: "full_task",
        bytes: include_str!("../../../tests/fixtures/markdown_golden/full_task.md"),
    },
    Golden {
        name: "doing_task",
        bytes: include_str!("../../../tests/fixtures/markdown_golden/doing_task.md"),
    },
    Golden {
        name: "done_task",
        bytes: include_str!("../../../tests/fixtures/markdown_golden/done_task.md"),
    },
    Golden {
        name: "special_chars",
        bytes: include_str!("../../../tests/fixtures/markdown_golden/special_chars.md"),
    },
    Golden {
        name: "wiki_linked_entities",
        bytes: include_str!("../../../tests/fixtures/markdown_golden/wiki_linked_entities.md"),
    },
    Golden {
        name: "knowledge_shape_diversity",
        bytes: include_str!("../../../tests/fixtures/markdown_golden/knowledge_shape_diversity.md"),
    },
];

/// Run J4 — Markdown serializer golden round-trip probe.
///
/// Returns `0` on green (every golden parses cleanly AND
/// re-serializes byte-identically), `1` on any failure.
pub fn run_serializer_golden_roundtrip_probe() -> i32 {
    println!(" J4 — kg-serializer-golden-roundtrip (Wave 1E.9 / mb-kazi)");
    println!(
        "    Asserting: parse(disk) -> serialize -> bytes-equal(disk) over {} fixtures",
        GOLDENS.len()
    );

    let mut failures: Vec<String> = Vec::new();

    for golden in GOLDENS {
        match check_one(golden) {
            Ok(()) => {
                println!("     {}: parse + re-serialize byte-identical", golden.name);
            }
            Err(msg) => {
                eprintln!("     {}: {msg}", golden.name);
                failures.push(format!("{}: {msg}", golden.name));
            }
        }
    }

    if failures.is_empty() {
        println!();
        println!(
            " J4 GREEN: all {} golden fixtures round-trip byte-identically",
            GOLDENS.len()
        );
        0
    } else {
        eprintln!();
        eprintln!(
            " J4 FAILED: {}/{} fixtures broke round-trip",
            failures.len(),
            GOLDENS.len()
        );
        for f in &failures {
            eprintln!("    - {f}");
        }
        1
    }
}

fn check_one(golden: &Golden) -> Result<(), String> {
    // Disk fixtures are committed with LF line endings; the parser
    // tolerates CRLF but our serializer always emits LF. If git's
    // autocrlf bit a fixture, we want a clear failure here, not a
    // confusing byte-diff later.
    if golden.bytes.contains('\r') {
        return Err("fixture on disk contains CR -- check .gitattributes / autocrlf".to_string());
    }

    // Direction 1: parse -> serialize -> bytes.
    let parsed = parse_entry(golden.bytes).map_err(|e| format!("parse_entry failed: {e}"))?;
    let reserialized = serialize_entry(&parsed.entry);

    if reserialized != golden.bytes {
        return Err(format!(
            "byte mismatch on re-serialize ({} bytes on disk, {} bytes after round-trip); \
             first divergence at offset {}",
            golden.bytes.len(),
            reserialized.len(),
            first_diff_offset(golden.bytes, &reserialized)
        ));
    }

    // Direction 2: re-parse the re-serialized output and confirm
    // the entry value compares equal to the first parse. This
    // catches a class of bug where parse/serialize are MUTUALLY
    // consistent but both wrong relative to disk -- the bytes-eq
    // check above covers disk, this one covers structural sanity.
    let reparsed =
        parse_entry(&reserialized).map_err(|e| format!("second parse_entry failed: {e}"))?;
    if reparsed.entry != parsed.entry {
        return Err("KgEntry not structurally equal after parse->serialize->parse".to_string());
    }

    Ok(())
}

/// Returns the byte offset of the first divergence between two
/// strings, or `min(a.len(), b.len())` if one is a strict prefix
/// of the other. Used only for the failure message — keeps the
/// probe output actionable without dumping multi-KB diffs.
fn first_diff_offset(a: &str, b: &str) -> usize {
    a.as_bytes()
        .iter()
        .zip(b.as_bytes().iter())
        .position(|(x, y)| x != y)
        .unwrap_or(a.len().min(b.len()))
}
