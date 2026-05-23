//! Vault manifest per ADR 0046 §1.
//!
//! The manifest is the single source of truth for "what's currently
//! projected into the vault". It lives at
//! `<vault>/.mockingbird/manifest.json` and maps each canonical
//! record's UUID to:
//!
//! - the relative path of its `history/*.md` file
//! - the SHA-256 hex of that file's bytes (for §5 byte-identical-
//!   on-re-export reconciliation)
//! - the ISO-8601 timestamp of the last export
//! - whether the record is a dictation or a meeting
//!
//! The export reconciliation job in [`super::export_job`] (Phase C)
//! consults this map to decide whether a record needs (re-)writing,
//! and to detect stale entries that should be archived.
//!
//! ## Single-writer assumption (ADR 0046 §16)
//!
//! v1 assumes one desktop writes a given vault. A future iter will
//! stamp `machine_fingerprint` with `sha256(hostname || install_dir)`
//! and refuse to run if it disagrees with the manifest on disk; the
//! field is reserved as `Option<String>` here and always `None` in
//! Iter 2 -- the load path tolerates it being missing in the JSON.
//!
//! ## On-disk format
//!
//! ```json
//! {
//!   "schema_version": 1,
//!   "machine_fingerprint": null,
//!   "records": {
//!     "<uuid-1>": {
//!       "path": "history/2026-05-27-1408__a4f7c2d3.md",
//!       "content_sha256": "deadbeef...",
//!       "last_exported_iso": "2026-05-27T14:08:42Z",
//!       "record_type": "dictation"
//!     },
//!     "<uuid-2>": { ... }
//!   }
//! }
//! ```
//!
//! `records` is a [`BTreeMap`] so the serialized output is
//! UUID-sorted. Deterministic on-disk byte layout makes the manifest
//! diffable across runs and avoids spurious "manifest changed but
//! content didn't" sync events from Obsidian Sync.
//!
//! ## Atomicity
//!
//! [`Manifest::save_atomic`] writes `manifest.json.tmp` first, then
//! renames over `manifest.json`. The concurrency model in ADR
//! §14 allows a live dictation `complete()` to fire while an
//! export pass is in flight; the rename gives the reader a
//! consistent view -- it sees either the old manifest or the new
//! one, never a half-written file.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// Current manifest schema version. Bumped on every breaking
/// front-matter or layout change per ADR 0046 §15. The boot-time
/// migration trigger (Phase C / `export_job.rs`) compares this to
/// the on-disk `schema_version` and forces a full vault rebuild
/// when the on-disk value is lower.
pub const MOCKINGBIRD_EXPORT_VERSION: u32 = 1;

/// What kind of canonical record this manifest entry projects.
///
/// Threaded into the front-matter `type` field and used by the
/// reconciliation job's stale-detection branch (a record that's
/// out of scope per `VaultSyncRecordTypes` gets archived, not
/// re-exported).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordType {
    /// One row from `sessions` (PTT, in-app, desktop-import,
    /// mobile-inbox).
    Dictation,
    /// One row from `meeting_sessions`.
    Meeting,
}

/// One entry in the manifest's `records` map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestRecord {
    /// Path of the projected file, relative to vault root,
    /// forward-slash separated. Forward slashes are canonical for
    /// cross-platform stability (Obsidian normalizes either way,
    /// but the manifest is also a diffable artifact -- we don't
    /// want spurious changes when the manifest is written on Mac
    /// vs Windows).
    pub path: String,

    /// Lowercase hex SHA-256 over the file's full byte contents
    /// (front-matter + body). The reconciliation engine compares
    /// this to the projection's freshly-computed SHA to decide
    /// whether to write at all.
    pub content_sha256: String,

    /// ISO-8601 UTC timestamp of the most recent successful export
    /// of this record. Read by the Connection Health card in the
    /// Mobile tab (Iter 4) and by the stale-record sweep.
    pub last_exported_iso: String,

    /// Whether this entry came from `sessions` or `meeting_sessions`.
    pub record_type: RecordType,
}

/// The whole manifest.
///
/// Stays ergonomic to construct in tests via struct literal --
/// nothing fancy in `Default` because the empty-records case is
/// the natural new-manifest shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Bumped per ADR §15 whenever the on-disk schema changes.
    pub schema_version: u32,

    /// Reserved for ADR §16 (multi-desktop guard). Always `None`
    /// in Iter 2; an Iter 4 follow-up wires the hostname-based
    /// fingerprint generation and refuse-to-run check.
    ///
    /// Serde-default-skipped so an old manifest written without
    /// the field still loads cleanly (forward-compat).
    #[serde(default)]
    pub machine_fingerprint: Option<String>,

    /// UUID -> entry. `BTreeMap` for deterministic
    /// (lexicographically-sorted) on-disk ordering. The Phase B
    /// projection is byte-identical for unchanged records; the
    /// manifest needs the same discipline so its file hash
    /// doesn't churn either.
    pub records: BTreeMap<String, ManifestRecord>,
}

impl Manifest {
    /// Empty fresh manifest at the current schema version. Useful
    /// for first-run / restored-from-deletion paths.
    pub fn fresh() -> Self {
        Self {
            schema_version: MOCKINGBIRD_EXPORT_VERSION,
            machine_fingerprint: None,
            records: BTreeMap::new(),
        }
    }

    /// Load from disk, returning a fresh manifest if the file
    /// doesn't exist. Errors only on parse / IO failure of an
    /// existing file -- never on absence, since absence is the
    /// expected first-run state and the reconciliation job
    /// recovers from it cleanly by treating every in-scope record
    /// as "needs export".
    pub fn load(path: &Path) -> AppResult<Self> {
        if !path.exists() {
            return Ok(Self::fresh());
        }
        let text = std::fs::read_to_string(path).map_err(|e| {
            AppError::Vault(format!("manifest load: read {} -- {}", path.display(), e))
        })?;
        let m: Manifest = serde_json::from_str(&text).map_err(|e| {
            AppError::Vault(format!("manifest load: parse {} -- {}", path.display(), e))
        })?;
        Ok(m)
    }

    /// Save atomically: write to `<path>.tmp`, then rename.
    ///
    /// Why not fsync the tmp file: `std::fs::File` doesn't expose
    /// a sync without holding the handle, and `std::fs::write`
    /// closes its handle before we can grab it. ADR §14's
    /// concurrency model accepts the resulting tiny window
    /// (process crash between write and rename leaves the old
    /// manifest in place -- which is the safe-by-default failure
    /// mode). v1 acceptable; can revisit if a Wave 5 fixture
    /// exposes a real durability gap.
    pub fn save_atomic(&self, path: &Path) -> AppResult<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                AppError::Vault(format!(
                    "manifest save: ensure parent {} -- {}",
                    parent.display(),
                    e
                ))
            })?;
        }
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| AppError::Vault(format!("manifest save: serialize -- {e}")))?;
        std::fs::write(&tmp, &json).map_err(|e| {
            AppError::Vault(format!(
                "manifest save: write tmp {} -- {}",
                tmp.display(),
                e
            ))
        })?;
        std::fs::rename(&tmp, path).map_err(|e| {
            AppError::Vault(format!(
                "manifest save: rename {} -> {} -- {}",
                tmp.display(),
                path.display(),
                e
            ))
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_record(path: &str, sha: &str, t: RecordType) -> ManifestRecord {
        ManifestRecord {
            path: path.into(),
            content_sha256: sha.into(),
            last_exported_iso: "2026-05-27T14:08:42Z".into(),
            record_type: t,
        }
    }

    #[test]
    fn fresh_manifest_has_current_version_and_no_records() {
        let m = Manifest::fresh();
        assert_eq!(m.schema_version, MOCKINGBIRD_EXPORT_VERSION);
        assert!(m.machine_fingerprint.is_none());
        assert!(m.records.is_empty());
    }

    #[test]
    fn load_missing_file_returns_fresh() {
        let td = TempDir::new().unwrap();
        let p = td.path().join("manifest.json");
        let m = Manifest::load(&p).unwrap();
        assert_eq!(m, Manifest::fresh());
    }

    #[test]
    fn round_trip_through_disk_preserves_records() {
        let td = TempDir::new().unwrap();
        let p = td.path().join(".mockingbird").join("manifest.json");
        let mut m = Manifest::fresh();
        m.records.insert(
            "ccccccc1-0000-0000-0000-000000000000".into(),
            sample_record("history/c.md", "cafebabe", RecordType::Dictation),
        );
        m.records.insert(
            "aaaaaaa1-0000-0000-0000-000000000000".into(),
            sample_record("history/a.md", "deadbeef", RecordType::Dictation),
        );
        m.records.insert(
            "bbbbbbb1-0000-0000-0000-000000000000".into(),
            sample_record("history/b.md", "feedface", RecordType::Meeting),
        );

        m.save_atomic(&p).unwrap();

        let m2 = Manifest::load(&p).unwrap();
        assert_eq!(m, m2);
    }

    #[test]
    fn save_atomic_emits_uuid_sorted_records() {
        let td = TempDir::new().unwrap();
        let p = td.path().join(".mockingbird").join("manifest.json");
        let mut m = Manifest::fresh();
        // Insert in reverse order to prove the BTreeMap sorts.
        for prefix in ["c", "b", "a"] {
            let uuid = format!("{}1111111-0000-0000-0000-000000000000", prefix);
            m.records.insert(
                uuid.clone(),
                sample_record(&format!("history/{prefix}.md"), "00", RecordType::Dictation),
            );
        }
        m.save_atomic(&p).unwrap();

        let raw = std::fs::read_to_string(&p).unwrap();
        let pos_a = raw.find("\"a1111111").unwrap();
        let pos_b = raw.find("\"b1111111").unwrap();
        let pos_c = raw.find("\"c1111111").unwrap();
        assert!(pos_a < pos_b, "a must precede b in serialized order");
        assert!(pos_b < pos_c, "b must precede c in serialized order");
    }

    #[test]
    fn save_atomic_does_not_leave_tmp_behind_on_success() {
        let td = TempDir::new().unwrap();
        let p = td.path().join(".mockingbird").join("manifest.json");
        let m = Manifest::fresh();
        m.save_atomic(&p).unwrap();
        let tmp = p.with_extension("json.tmp");
        assert!(p.exists(), "final manifest must be present");
        assert!(!tmp.exists(), "tmp must be renamed away on success");
    }

    #[test]
    fn save_atomic_creates_missing_parent_dir() {
        let td = TempDir::new().unwrap();
        // Parent dir intentionally not pre-created.
        let p = td.path().join(".mockingbird").join("manifest.json");
        assert!(!p.parent().unwrap().exists());
        let m = Manifest::fresh();
        m.save_atomic(&p).unwrap();
        assert!(p.exists());
    }

    #[test]
    fn load_rejects_garbage_json() {
        let td = TempDir::new().unwrap();
        let p = td.path().join("manifest.json");
        std::fs::write(&p, b"this is not json").unwrap();
        let err = Manifest::load(&p).unwrap_err();
        match err {
            AppError::Vault(msg) => assert!(msg.contains("parse")),
            other => panic!("expected Vault parse error, got {other:?}"),
        }
    }

    #[test]
    fn machine_fingerprint_missing_in_json_loads_as_none() {
        let td = TempDir::new().unwrap();
        let p = td.path().join("manifest.json");
        // Hand-write a manifest WITHOUT the machine_fingerprint key
        // to prove `#[serde(default)]` covers forward-compat with
        // pre-Iter-4 manifests.
        let json = r#"{"schema_version":1,"records":{}}"#;
        std::fs::write(&p, json).unwrap();
        let m = Manifest::load(&p).unwrap();
        assert!(m.machine_fingerprint.is_none());
        assert_eq!(m.schema_version, 1);
    }

    #[test]
    fn record_type_serializes_lowercase() {
        let r = sample_record("history/x.md", "00", RecordType::Meeting);
        let json = serde_json::to_string(&r).unwrap();
        // Matches the rename_all = "lowercase" on RecordType.
        assert!(json.contains("\"record_type\":\"meeting\""), "got {json}");
    }
}
