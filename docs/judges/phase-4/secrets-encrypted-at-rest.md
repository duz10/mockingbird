# Judge: secrets-encrypted-at-rest (Phase 4)

**Target:** `src-tauri/src/secrets/windows.rs` (DPAPI store)

**Question:** When a secret is put into the DPAPI store, do the bytes on disk contain the plaintext anywhere?

**Pass criteria:**

`pwsh scripts/cargo-with-cuda.ps1 test --release --lib -- secrets::windows::tests::cipher_on_disk_does_not_contain_plaintext --exact` → `1 passed`.

Test plants a sentinel string into the store, reads the on-disk cipher file back as bytes, and asserts the sentinel is **not** present anywhere in the byte stream.

**On failure:** block `phase-4-complete` tag. This is non-negotiable — a regression here means user API keys would be readable by any process that can access the file.

**Last run:** 2026-05-18 — GREEN.
