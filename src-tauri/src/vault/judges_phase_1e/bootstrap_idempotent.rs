//! J3 - `kg-subtree-bootstrap-idempotent` (Wave 1E.9 / `mb-kazi`).
//!
//! Asserts that the KG subtree + root-file bootstrap helpers are
//! idempotent across four state cells:
//!
//! 1. **Missing** -- no vault dir at all. `bootstrap_kg_subtree`
//!    creates six folders; `bootstrap_kg_root_files` writes three
//!    files. Second call returns `AlreadyExists` for both and
//!    touches nothing.
//! 2. **Empty** -- vault root exists but `Knowledge Graph/` is
//!    absent. Same expectation as cell (1) for the bootstrap pair.
//! 3. **Populated** -- the full nine-item shape (6 folders + 3
//!    files) already exists with user content. Bootstrap must
//!    return `AlreadyExists` and not overwrite any file.
//! 4. **Partial** -- subtree present, but one of the three root
//!    files is missing. Bootstrap fills the gap and reports
//!    `Created` ; the other two root files are NOT touched
//!    (write-once semantics per ADR 0053).
//!
//! The "second call" check is the load-bearing idempotency
//! property: every successful bootstrap must be safe to re-run.
//! This protects against the activation-loop where the user
//! re-toggles the KG feature mid-session.
//!
//! Spec: `docs/phases/phase-1e.md` §"Wave 1E.9" + §FRAMING-UPDATED
//! ("extended to cover the six-folder + three-file shape from
//! Wave 1E.7"). README seed is not yet shipped (Wave 1E.7
//! framing-amendment is docs-only); this probe covers the actual
//! committed shape and will need extension if README ever lands.

use std::path::{Path, PathBuf};

use tempfile::TempDir;

use crate::error::{AppError, AppResult};
use crate::vault::kg_layout::{
    bootstrap_kg_root_files, bootstrap_kg_subtree, kg_root_file_paths, kg_subtree_paths,
    BootstrapReport, RootFilesReport,
};

/// Run J3 - subtree bootstrap idempotency probe.
///
/// Returns `0` on green (all four cells converge to the same
/// on-disk shape; back-to-back bootstrap calls touch nothing),
/// `1` on any assertion failure.
pub fn run_subtree_bootstrap_idempotent_probe() -> i32 {
    println!("J3 - kg-subtree-bootstrap-idempotent (Wave 1E.9 / mb-kazi)");

    match run_inner() {
        Ok(()) => {
            println!();
            println!(
                "J3 GREEN: 4 cells (missing / empty / populated / partial) all idempotent across the 6-folder + 3-root-file shape"
            );
            0
        }
        Err(e) => {
            eprintln!();
            eprintln!("J3 FAILED");
            eprintln!("    reason: {e}");
            1
        }
    }
}

fn run_inner() -> AppResult<()> {
    cell_missing()?;
    cell_empty()?;
    cell_populated()?;
    cell_partial()?;
    Ok(())
}

// ----------------------------------------------------------------
// Cell 1 -- missing vault
// ----------------------------------------------------------------

fn cell_missing() -> AppResult<()> {
    // tempfile gives us a parent we own; pretend the "vault" dir
    // INSIDE it doesn't yet exist. bootstrap_kg_subtree must
    // create it transparently.
    let td = TempDir::new().map_err(io_err)?;
    let vault = td.path().join("not_yet_created_vault");

    let r1 = bootstrap_kg_subtree(&vault)?;
    assert_report(r1, BootstrapReport::Created, "missing/subtree first call")?;

    let f1 = bootstrap_kg_root_files(&vault)?;
    assert_root_report(
        f1,
        RootFilesReport::Created,
        "missing/root-files first call",
    )?;

    assert_full_shape_present(&vault, "missing/after first bootstrap")?;

    let r2 = bootstrap_kg_subtree(&vault)?;
    assert_report(
        r2,
        BootstrapReport::AlreadyExists,
        "missing/subtree second call",
    )?;

    let f2 = bootstrap_kg_root_files(&vault)?;
    assert_root_report(
        f2,
        RootFilesReport::AlreadyExists,
        "missing/root-files second call",
    )?;

    println!("    cell 1 (missing): first call Created + second call AlreadyExists");
    Ok(())
}

// ----------------------------------------------------------------
// Cell 2 -- empty vault (dir exists, KG subtree does not)
// ----------------------------------------------------------------

fn cell_empty() -> AppResult<()> {
    let td = TempDir::new().map_err(io_err)?;
    let vault = td.path().to_path_buf();

    let r1 = bootstrap_kg_subtree(&vault)?;
    assert_report(r1, BootstrapReport::Created, "empty/subtree first call")?;

    let f1 = bootstrap_kg_root_files(&vault)?;
    assert_root_report(f1, RootFilesReport::Created, "empty/root-files first call")?;

    assert_full_shape_present(&vault, "empty/after first bootstrap")?;

    let r2 = bootstrap_kg_subtree(&vault)?;
    assert_report(
        r2,
        BootstrapReport::AlreadyExists,
        "empty/subtree second call",
    )?;

    let f2 = bootstrap_kg_root_files(&vault)?;
    assert_root_report(
        f2,
        RootFilesReport::AlreadyExists,
        "empty/root-files second call",
    )?;

    println!("    cell 2 (empty): first call Created + second call AlreadyExists");
    Ok(())
}

// ----------------------------------------------------------------
// Cell 3 -- populated vault (user has edited the root files)
// ----------------------------------------------------------------

fn cell_populated() -> AppResult<()> {
    let td = TempDir::new().map_err(io_err)?;
    let vault = td.path().to_path_buf();

    bootstrap_kg_subtree(&vault)?;
    bootstrap_kg_root_files(&vault)?;

    // Simulate the user editing each root file. Write-once
    // semantics mean these bytes must survive a re-bootstrap.
    let root = kg_root_file_paths(&vault);
    let user_schema = "# SCHEMA\n\nuser-owned content\n";
    let user_index = "# INDEX\n\nuser-edited links\n";
    let user_log = "# LOG\n\nuser appended line\n";
    std::fs::write(&root.schema_md, user_schema).map_err(io_err)?;
    std::fs::write(&root.index_md, user_index).map_err(io_err)?;
    std::fs::write(&root.log_md, user_log).map_err(io_err)?;

    let r = bootstrap_kg_subtree(&vault)?;
    assert_report(
        r,
        BootstrapReport::AlreadyExists,
        "populated/subtree re-run",
    )?;
    let f = bootstrap_kg_root_files(&vault)?;
    assert_root_report(
        f,
        RootFilesReport::AlreadyExists,
        "populated/root-files re-run",
    )?;

    assert_file_bytes_eq(&root.schema_md, user_schema, "populated/SCHEMA.md")?;
    assert_file_bytes_eq(&root.index_md, user_index, "populated/INDEX.md")?;
    assert_file_bytes_eq(&root.log_md, user_log, "populated/LOG.md")?;

    println!(
        "    cell 3 (populated): re-bootstrap is no-op + user-edited bytes preserved verbatim"
    );
    Ok(())
}

// ----------------------------------------------------------------
// Cell 4 -- partial (subtree present, one root file missing)
// ----------------------------------------------------------------

fn cell_partial() -> AppResult<()> {
    let td = TempDir::new().map_err(io_err)?;
    let vault = td.path().to_path_buf();

    bootstrap_kg_subtree(&vault)?;
    bootstrap_kg_root_files(&vault)?;

    let root = kg_root_file_paths(&vault);

    // Capture the originals so we can prove the other two
    // weren't touched.
    let schema_before = std::fs::read(&root.schema_md).map_err(io_err)?;
    let log_before = std::fs::read(&root.log_md).map_err(io_err)?;

    // Simulate the user deleting only INDEX.md.
    std::fs::remove_file(&root.index_md).map_err(io_err)?;

    let r = bootstrap_kg_subtree(&vault)?;
    assert_report(r, BootstrapReport::AlreadyExists, "partial/subtree no-op")?;
    let f = bootstrap_kg_root_files(&vault)?;
    assert_root_report(f, RootFilesReport::Created, "partial/INDEX.md refilled")?;

    if !root.index_md.exists() {
        return Err(other("partial: INDEX.md still missing after re-bootstrap"));
    }
    let schema_after = std::fs::read(&root.schema_md).map_err(io_err)?;
    let log_after = std::fs::read(&root.log_md).map_err(io_err)?;
    if schema_before != schema_after {
        return Err(other(
            "partial: SCHEMA.md was mutated by re-bootstrap (must be write-once)",
        ));
    }
    if log_before != log_after {
        return Err(other(
            "partial: LOG.md was mutated by re-bootstrap (must be write-once)",
        ));
    }

    // Third call -- the gap-fill must now be idempotent in turn.
    let f2 = bootstrap_kg_root_files(&vault)?;
    assert_root_report(
        f2,
        RootFilesReport::AlreadyExists,
        "partial/third call after gap-fill",
    )?;

    println!(
        "    cell 4 (partial): single-file gap filled + neighbors untouched + then idempotent"
    );
    Ok(())
}

// ----------------------------------------------------------------
// Shape + assertion helpers
// ----------------------------------------------------------------

/// The KG subtree's canonical shape: 6 folders + 3 root files.
/// Listed as `(label, path-resolver)` so failure messages name the
/// missing item.
fn nine_required_paths(vault: &Path) -> Vec<(&'static str, PathBuf)> {
    let s = kg_subtree_paths(vault);
    let r = kg_root_file_paths(vault);
    vec![
        ("Knowledge Graph/Inbox", s.inbox),
        ("Knowledge Graph/Entries", s.entries),
        ("Knowledge Graph/History", s.history),
        ("Knowledge Graph/Entities", s.entities),
        ("Knowledge Graph/Projects", s.projects),
        ("Knowledge Graph/Tags", s.tags),
        ("Knowledge Graph/SCHEMA.md", r.schema_md),
        ("Knowledge Graph/INDEX.md", r.index_md),
        ("Knowledge Graph/LOG.md", r.log_md),
    ]
}

fn assert_full_shape_present(vault: &Path, label: &str) -> AppResult<()> {
    for (name, path) in nine_required_paths(vault) {
        if !path.exists() {
            return Err(other(&format!(
                "{label}: required path missing -- {name} ({})",
                path.display()
            )));
        }
    }
    Ok(())
}

fn assert_report(got: BootstrapReport, want: BootstrapReport, label: &str) -> AppResult<()> {
    if got != want {
        return Err(other(&format!(
            "{label}: BootstrapReport mismatch -- got {got:?}, wanted {want:?}"
        )));
    }
    Ok(())
}

fn assert_root_report(got: RootFilesReport, want: RootFilesReport, label: &str) -> AppResult<()> {
    if got != want {
        return Err(other(&format!(
            "{label}: RootFilesReport mismatch -- got {got:?}, wanted {want:?}"
        )));
    }
    Ok(())
}

fn assert_file_bytes_eq(path: &Path, want: &str, label: &str) -> AppResult<()> {
    let got = std::fs::read_to_string(path).map_err(io_err)?;
    if got != want {
        return Err(other(&format!(
            "{label}: bytes diverged from user-edited content (len got={}, want={})",
            got.len(),
            want.len()
        )));
    }
    Ok(())
}

fn io_err<E: std::fmt::Display>(e: E) -> AppError {
    AppError::Other(format!("io: {e}"))
}

fn other(msg: &str) -> AppError {
    AppError::Other(msg.to_string())
}
