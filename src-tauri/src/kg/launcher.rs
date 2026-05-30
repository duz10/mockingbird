//! Phase 1D Wave 1D.5 (`mb-navi`, ADR 0052) -- Obsidian launcher.
//!
//! Owns the "launch the user's Obsidian vault from the app" side
//! effect. Used by two call sites:
//!
//! 1. Settings -> Knowledge Graph tab (the per-tab affordance).
//! 2. The KG dashboard's `Actions` band (mirror affordance so the
//!    user doesn't have to bounce through Settings).
//!
//! Both invoke `kg_launch_obsidian` (in `commands::kg`); this module
//! is the pure-Rust side effect they wrap.
//!
//! ## Why the `obsidian://` URI scheme (not a shell-open of the
//! folder)
//!
//! Per spec §15.4: the canonical handoff is the URI scheme.
//! Shelling out to the bare filesystem path would open the folder
//! in Explorer / Finder, NOT in Obsidian -- a worse UX (you'd see
//! the markdown files as text in a file manager) AND it would
//! couple us to a particular vault-folder layout. The scheme lets
//! Obsidian resolve the registered vault by name and apply the
//! user's plugins, themes, sync state, etc.
//!
//! Obsidian's URI handler accepts either `?vault=<name>` (resolves
//! against the registered-vaults list) or `?path=<absolute path>`
//! (opens an arbitrary folder as a vault). We use `?vault=<name>`
//! because (a) it's the documented happy path, and (b) on Windows
//! the absolute paths Mockingbird stores tend to contain backslashes
//! that have historically tripped Obsidian's URL parser. The
//! "vault name" is the final path component of the configured
//! vault path -- this matches Obsidian's own naming convention
//! (the vault name in the open-recent menu IS the leaf folder
//! name).
//!
//! ## Cross-platform discipline (Principle 5)
//!
//! Windows ships now; macOS gets a stub that returns an explicit
//! error. The stub keeps the `cfg`-gated arms in sync so a macOS
//! build doesn't fail to compile when someone tries the IPC -- it
//! fails at runtime with a clear "not yet supported" message.
//!
//! ## Why we don't use Tauri's `shell` plugin
//!
//! The shell plugin would work for `cmd /c start <url>` on Windows
//! but pulls a permissions allowlist into `capabilities/default.json`
//! that's broader than this one use case warrants (the plugin's
//! `open` permission would whitelist arbitrary URL/path opens from
//! the frontend; we'd rather keep the surface to one Rust-side IPC
//! we can audit). `std::process::Command` is the smaller hammer.

use std::path::Path;

use crate::error::{AppError, AppResult};

/// Launch the user's Obsidian vault. `vault_path` is the absolute
/// path to the vault's root folder (as stored in the Mobile Sync
/// settings); the leaf component is extracted and used as the
/// vault name handed to the `obsidian://` URI scheme.
///
/// Errors:
/// - [`AppError::Other`] if `vault_path` has no resolvable leaf
///   name (e.g. root paths like `C:\` or `/`).
/// - [`AppError::Io`] if `std::process::Command::spawn` fails
///   (Obsidian not installed, or the user's URL handler isn't
///   wired; the Win32 shell will fail with `ERROR_NO_ASSOCIATION`).
/// - [`AppError::Other`] on non-Windows platforms (macOS impl is a
///   stub until Phase 9).
pub fn launch_obsidian_vault(vault_path: &Path) -> AppResult<()> {
    let vault_name = vault_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            AppError::Other(format!(
                "vault path {vault_path:?} has no leaf-name component"
            ))
        })?;
    let uri = format!("obsidian://open?vault={}", encode_vault_name(vault_name));
    spawn_uri(&uri)
}

/// Minimal application/x-www-form-urlencoded encoder, scoped to the
/// characters a Windows folder name can plausibly contain. Spaces,
/// `&`, `=`, `+`, `?`, `#`, and `%` are percent-encoded; everything
/// else (letters, digits, hyphen, underscore, dot, parens) is left
/// alone -- those are unreserved per RFC 3986 and Obsidian accepts
/// them verbatim in vault names.
///
/// Lives here as a private helper rather than pulling in the
/// `url` or `percent-encoding` crates: this is the only encoding
/// call site in the entire crate that targets the `obsidian://`
/// scheme, and the input alphabet is narrow enough that a 10-line
/// helper is honestly simpler than a dependency edit + 200 KB of
/// generic encoding tables. If a second call site ever materializes,
/// promote to a crate.
fn encode_vault_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        match ch {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '(' | ')' => {
                out.push(ch);
            }
            _ => {
                // Percent-encode every byte of the char's UTF-8.
                let mut buf = [0u8; 4];
                for &byte in ch.encode_utf8(&mut buf).as_bytes() {
                    out.push_str(&format!("%{byte:02X}"));
                }
            }
        }
    }
    out
}

#[cfg(target_os = "windows")]
fn spawn_uri(uri: &str) -> AppResult<()> {
    // `cmd /c start "" <url>` -- the empty `""` argument is the
    // window title that `start` would otherwise consume; without it
    // `start` interprets the URL as the title and silently does
    // nothing. We deliberately do NOT wait for the spawned process
    // -- `start` returns immediately after handing off to the shell.
    std::process::Command::new("cmd")
        .args(["/c", "start", "", uri])
        .spawn()
        .map_err(AppError::Io)?;
    Ok(())
}

#[cfg(target_os = "macos")]
#[allow(dead_code)]
fn spawn_uri(_uri: &str) -> AppResult<()> {
    // Phase 9 will wire `open <uri>` here. Until then we fail loud
    // so a misclick on macOS surfaces as a toast rather than a
    // silent no-op. The structure mirrors the Windows arm so the
    // future implementation is a one-line swap.
    Err(AppError::Other(
        "launching Obsidian from Mockingbird is not yet supported on macOS \
         (Phase 9 work)"
            .into(),
    ))
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
#[allow(dead_code)]
fn spawn_uri(_uri: &str) -> AppResult<()> {
    // Linux / other -- not a v1 target, but keep the arm so the
    // crate compiles on a tinkerer's box.
    Err(AppError::Other(
        "launching Obsidian from Mockingbird is only supported on Windows in v1".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn encode_passes_through_unreserved_chars() {
        assert_eq!(encode_vault_name("MyVault"), "MyVault");
        assert_eq!(encode_vault_name("my-vault_v2.0"), "my-vault_v2.0");
        assert_eq!(
            encode_vault_name("vault(backup)"),
            "vault(backup)",
            "RFC 3986 sub-delims subset we whitelist must pass through",
        );
    }

    #[test]
    fn encode_percent_encodes_space() {
        // The single most common case: spaces in vault folder
        // names. Must be `%20` (the URL form), NOT `+` (that's
        // application/x-www-form-urlencoded body-form, which
        // Obsidian's URI parser does not normalize back to space).
        assert_eq!(encode_vault_name("My Notes"), "My%20Notes");
    }

    #[test]
    fn encode_percent_encodes_ampersand_and_query_delimiters() {
        // `&`, `=`, `?`, `#`, `+`, `%` would break the URL structure
        // if they leaked through. Pin them so a future "whitelist
        // tweak" can't silently break the URI scheme.
        assert_eq!(encode_vault_name("A&B"), "A%26B");
        assert_eq!(encode_vault_name("k=v"), "k%3Dv");
        assert_eq!(encode_vault_name("a?b"), "a%3Fb");
        assert_eq!(encode_vault_name("x#y"), "x%23y");
        assert_eq!(encode_vault_name("p+q"), "p%2Bq");
        assert_eq!(encode_vault_name("100%"), "100%25");
    }

    #[test]
    fn encode_handles_multibyte_utf8() {
        // Each byte of the UTF-8 encoding gets its own %XX.
        // "é" is U+00E9 = 0xC3 0xA9 in UTF-8 -> "%C3%A9".
        assert_eq!(encode_vault_name("café"), "caf%C3%A9");
        // Emoji are 4 bytes in UTF-8.
        assert_eq!(encode_vault_name("📓"), "%F0%9F%93%93");
    }

    #[test]
    fn launch_errors_when_path_has_no_leaf() {
        // Root-only paths have no `file_name()`. Should error out
        // before even attempting to spawn, regardless of platform
        // (the error arm is shared across all `cfg`s).
        #[cfg(target_os = "windows")]
        let root = PathBuf::from(r"C:\");
        #[cfg(not(target_os = "windows"))]
        let root = PathBuf::from("/");

        let err = launch_obsidian_vault(&root).expect_err("root path should fail");
        let msg = err.to_string();
        assert!(
            msg.contains("leaf-name") || msg.contains("file name"),
            "error should mention the missing leaf name, got: {msg}"
        );
    }
}
