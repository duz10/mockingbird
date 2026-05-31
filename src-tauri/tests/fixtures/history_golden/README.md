# History sidecar golden fixtures (Wave 1E.4, mb-i14b)

Locks the canonical JSON form of the per-session history sidecar
defined in `src-tauri/src/vault/history.rs` (`serialize_sidecar`)
per ADR 0053 §D7.

## Fixtures

| File | Shape |
|---|---|
| `kg_note_with_audio.json` | Audio capture (`capture_kind = kg-note`) with full transcripts + UUID hash. The JSON itself doesn't reference the audio path — the discriminator is `capture_kind`. |
| `kg_note_text.json` | Text-only capture (`capture_kind = kg-note-text`). Raw + cleaned transcripts identical (the text path writes the user's typed text into both stages — see `kg::ingest_text`). |
| `kg_note_sparse.json` | Audio capture with an empty `cleaned_transcript` (simulates cleanup-pass failure). Locks the contract that empty strings are emitted, not omitted. |

## Regeneration workflow

To intentionally update after a canonical-form change:

```bash
MOCKINGBIRD_UPDATE_GOLDENS=1 cargo test -p mockingbird golden_kg_note
```

The test resolves the fixtures path via `file!()` (not `CARGO_MANIFEST_DIR`)
so the LESSONS P2 throwaway-crate harness writes back to the real tree.
Re-run without the env var to verify the new bytes lock cleanly.
