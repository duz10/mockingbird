# v1 Public Launch Readiness - Static Security Audit (LR.0.A)

- **Audit date:** 2026-06-04
- **Auditor:** code-puppy (Bernard) on duz10's Windows + CUDA dev box
- **Scope:** Mockingbird repo at HEAD `dd5b69c` (working tree clean at audit start)
- **Charter:** ADR-0055 "v1 public launch readiness" (this audit is wave LR.0.A)
- **Out of scope this dispatch:** Remediation of any finding. This document
  reports state and recommends actions for the LR.1 through LR.6 waves.

> **No em dashes in this report's user-visible summary.** The report body
> stays plain ASCII so any quoted excerpts can flow into external docs in
> LR.2 without rewriting.

---

## Top-line summary

The repo is **clean for a public flip from a secrets standpoint.** Zero
real credentials in git history, zero tracked binaries, zero flagged-supply-chain
packages in the JS dependency tree, zero Rust vulnerabilities, and `.env*`
files have never landed in any commit.

Two production-relevant supply-chain findings need a small amount of
attention before LR.6:

1. **react-router 6.7.0 to 6.30.3 GHSA-2j2x-hqr9-3h42** (moderate). Open-redirect
   via protocol-relative URL in `same-origin` redirects. Reachability in a
   Tauri shell is much lower than in a browser, but the fix is a one-line
   bump. Recommended for LR.2 or LR.3.
2. **cargo audit was not installed** on the dev box at audit start. Installed
   during the audit (`cargo install cargo-audit --locked`, v0.22.1). Add it
   to the LR.3 CI pipeline so this gate is mechanically enforced.

Everything else is either inert (Linux-only Tauri paths flagged by
gtk-rs unmaintenance), unactionable until upstream Tauri moves
(transitive `unic-*` warnings), or planned-for-rewrite work already
scheduled in LR.2 (the Wispr Flow reference in `tauri.conf.json`).

**Recommendation:** Proceed to LR.0.B (Unsplash DPAPI migration) immediately.
None of the findings below block subsequent waves. They all become bead
work folded into the existing LR.2 / LR.3 sub-beads.

---

## Check 1: Git history secret scan

**Status:** PASS.

Method: dumped full history via `git log --all -p --no-color` (17.6 MB,
317,293 lines covering every commit on every branch) then ran the
following regex patterns against the dump via PowerShell `Select-String`:

| Pattern | Regex | Matches | Disposition |
|---|---|---|---|
| Anthropic / OpenAI SK | `sk-[A-Za-z0-9_\-]{20,}` | 4 | All 4 are synthetic test fixtures in `src-tauri/src/logging.rs` tests and one in a fixture in the scrubber test corpus. Strings are `sk-ant-realistic-looking-key-xxx...` and `sk-ant-abcdefghij0123456789KLMN` (sentinel data, not real keys). CLEAN. |
| AWS Access Key ID | `AKIA[0-9A-Z]{16}` | 0 | CLEAN. |
| GitHub PAT | `gh[ps]_[A-Za-z0-9]{36}` | 0 | CLEAN. |
| Slack token | `xox[bpsa]-[A-Za-z0-9-]{10,}` | 0 | CLEAN. |
| Generic Bearer | `Bearer\s+[A-Za-z0-9\-._~+/]{20,}=*` | 0 | CLEAN. |
| Quoted password assign | `(?i)password\s*[:=]\s*['"][^'"]{6,}` | 0 | CLEAN. |
| 40-char hex (Unsplash shape) | `\b[a-fA-F0-9]{40}\b` | 20+ | All hits are Git SHA-1 commit hashes in the `commit XXXX...` headers of the dump itself. Zero hits in patch bodies. CLEAN. |

**Recommendation:** None. Public flip is safe from a history standpoint.
`git-filter-repo` is NOT required.

## Check 2: .env history audit

**Status:** PASS.

`git log --all --oneline -- .env .env.local .env.production` returns empty.
No env file has ever been committed on any branch.

`.gitignore` ships `.env` and `.env.*` with a single exception for
`.env.example` (re-allowed via `!.env.example`). Verified at lines 17-19
of `.gitignore`.

`.env.local` exists in the working tree but is correctly gitignored
(verified via `git check-ignore -v`).

## Check 3: Tauri signing privkey location

**Status:** PASS.

`git ls-files | findstr` for `*.key`, `*.pfx`, `*.pem`, `*.cer` returns
**zero** tracked files of any of those types. `.gitignore` lines 9-13 ship
`*.key`, `*.key.pub`, `*.pfx`, `*.pem` as belt-and-suspenders.

`.env.example` documents `TAURI_SIGNING_PRIVATE_KEY_PATH=%USERPROFILE%\.tauri\mockingbird.key`
which is outside the working tree. Convention matches ADR 0005 (code-signing
deferred).

## Check 4: cargo audit (Rust supply-chain)

**Status:** PASS with documented exceptions.

| Metric | Count |
|---|---|
| Vulnerabilities | **0** |
| Unmaintained warnings | 16 |
| Unsound warnings | 1 |

`cargo install cargo-audit --locked` installed v0.22.1 during this audit.
**Action for LR.3:** add `cargo audit` to CI. **Action right now: none**;
this is documented in the LR.3 sub-bead.

### Documented exceptions (no action required for v0.2.0-beta.1 launch)

| Advisory | Crate / version | Kind | Reachable on Windows release build? | Disposition |
|---|---|---|---|---|
| RUSTSEC-2024-0413 | atk 0.18.2 | unmaintained | No (Linux GTK only) | Inert. Upstream Tauri swap-out tracked under Tauri 2.x roadmap. |
| RUSTSEC-2024-0416 | atk-sys 0.18.2 | unmaintained | No | Inert. |
| RUSTSEC-2024-0412 | gdk 0.18.2 | unmaintained | No | Inert. |
| RUSTSEC-2024-0418 | gdk-sys 0.18.2 | unmaintained | No | Inert. |
| RUSTSEC-2024-0411 | gdkwayland-sys 0.18.2 | unmaintained | No | Inert. |
| RUSTSEC-2024-0417 | gdkx11 0.18.2 | unmaintained | No | Inert. |
| RUSTSEC-2024-0414 | gdkx11-sys 0.18.2 | unmaintained | No | Inert. |
| RUSTSEC-2024-0415 | gtk 0.18.2 | unmaintained | No | Inert. |
| RUSTSEC-2024-0420 | gtk-sys 0.18.2 | unmaintained | No | Inert. |
| RUSTSEC-2024-0419 | gtk3-macros 0.18.2 | unmaintained | No | Inert. |
| RUSTSEC-2024-0429 | glib 0.18.5 | unsound (VariantStrIter) | No (webview2 on Windows; webkit2gtk path is dead) | Inert on Windows release. Becomes live when macOS support lands in Phase 9. Re-evaluate then. |
| RUSTSEC-2024-0370 | proc-macro-error 1.0.4 | unmaintained | Build time only | Compile dep of `darling_macro`, `tao-macros`. No runtime exposure. Will fall out when upstream proc-macro2-error transition completes. |
| RUSTSEC-2025-0081 | unic-char-property 0.9.0 | unmaintained | Yes (urlpattern path) | Real but inert; transitive through `tauri-utils -> urlpattern`. Not directly used by Mockingbird code. Tracked under upstream Tauri. |
| RUSTSEC-2025-0075 | unic-char-range 0.9.0 | unmaintained | Yes | Same as above. |
| RUSTSEC-2025-0080 | unic-common 0.9.0 | unmaintained | Yes | Same. |
| RUSTSEC-2025-0100 | unic-ucd-ident 0.9.0 | unmaintained | Yes | Same. |
| RUSTSEC-2025-0098 | unic-ucd-version 0.9.0 | unmaintained | Yes | Same. |

The five `unic-*` warnings share a single transitive root and will all
clear in a single `tauri-utils` bump. No standalone action needed.

## Check 5: npm audit --omit=dev (JS supply-chain)

**Status:** WARN. Two moderate advisories, both transitive through
`react-router-dom`. Both fixable via `npm audit fix --ignore-scripts`.

| Advisory | Crate / version | Severity | Description |
|---|---|---|---|
| GHSA-2j2x-hqr9-3h42 | react-router 6.7.0 - 6.30.3 | moderate | Same-origin redirect with path starting `//` causes open redirect via protocol-relative URL reinterpretation. |
| (transitive) | react-router-dom 6.6.3-pre.0 - 6.30.3 | moderate | Depends on the above. |

Reachability in the Mockingbird Tauri shell: low. The app does not accept
arbitrary URLs from external sources; routing is fully internal and the
single external launch path (`https://...` from the model-download script
and the Unsplash request URL) is constructed in Rust on a fixed allowlist.
Still, the fix is a single dependency bump.

**Recommendation:** Fold into LR.2 (where docs are rewritten anyway and
the `package.json` is in flux for the Unsplash DPAPI migration's UI side)
or LR.3 (where CI is wired). Run with `--ignore-scripts` per ADR 0006.

## Check 6: Mini Shai-Hulud IOC scan

**Status:** PASS.

Scanned `ui/package.json` and `ui/package-lock.json` for the flagged
namespaces called out in PLAN Appendix D and the `supply-chain` skill:

| Namespace | package.json | package-lock.json |
|---|---|---|
| `@tanstack/` | 0 | 0 |
| `@mistralai/` | 0 | 0 |
| `@uipath/` | 0 | 0 |
| `@squawk/` | 0 | 0 |
| `@draftlab/` | 0 | 0 |

Plus the project-level hook `block-tanstack` mechanically refuses
re-introduction.

## Check 7: Logging scrubber regex extension

**Status:** DONE in this dispatch.

`src-tauri/src/logging.rs` `ScrubberSet` extended from 2 patterns
(`sk-...`, email) to 5 patterns:

| Pattern | Regex | Placeholder |
|---|---|---|
| `api_key` | `sk-[A-Za-z0-9_\-]{20,}` | `sk-<REDACTED>` |
| `github_pat` | `gh[ps]_[A-Za-z0-9]{36}` | `gh<REDACTED>` |
| `bearer` | `Bearer\s+[A-Za-z0-9\-._~+/]{20,}=*` | `Bearer <REDACTED>` |
| `hex40` | `(?i)\b[a-f0-9]{40}\b` | `<HEX40_REDACTED>` |
| `email` | (existing) | `<EMAIL>` |

Ordering matters: `api_key` runs before `hex40` so an `sk-ant-...` body
can never be misclassified as hex, and `bearer` runs before `hex40` so a
`Bearer <40-hex>` token gets the more-informative bearer placeholder.

Three new unit tests cover the new patterns (`scrubber_redacts_github_pat`,
`scrubber_redacts_bearer_tokens`, `scrubber_redacts_hex40_blobs`) plus
boundary cases (39-char and 41-char hex must NOT trigger; short PATs
must NOT trigger; case-insensitive hex; tab whitespace tolerance).

The existing `scrubber_redacts_api_keys` test is unchanged and still
asserts the same behavior. `cargo fmt --check` and
`cargo clippy --release -- -D warnings` both GREEN against the
extended module.

## Check 8: Internal-doc exposure map (input list for LR.1 gitignore)

**Status:** mapped. This becomes the gitignore patch in LR.1.

### Top-level files to gitignore (internal maintainer artifacts)

| Path | Why it is internal | Action |
|---|---|---|
| `PLAN-mockingbird-v2.md` | Full v2 plan, references internal methodology | gitignore in LR.1 |
| `STATUS.md` | Session anchor, references phases/beads/judges | gitignore in LR.1 |
| `mockingbird-activity-capture-plan.md` | Internal planning doc | gitignore in LR.1 |
| `mockingbird-logo-motion.html` | Dev experiment, not user-facing | gitignore in LR.1 |
| `verify_iter1_schema.py` | One-off verification script | gitignore in LR.1 |
| `.cargo-test-output.log` | Build artifact (already covered by `*.log`) | already covered |
| `build_phaseB_test.log`, `build_phaseC_release.log`, `final_test_norun.log`, `launch_phaseC.log`, `test-llm-prompts.log`, `test-output.log`, `test_link.log`, `test_out.log` | Build/test stdout dumps | already covered by `*.log` |
| `Code-Puppy-Capabilities-Insight.docx` | Methodology reference | already explicitly gitignored |
| `.env.local` | Dev secrets file | already covered by `.env.*` |

### Top-level files to KEEP as user-facing (rewrite or polish during LR.2)

| Path | Action in LR.2 |
|---|---|
| `README.md` | Rewrite to drop `.code_puppy/`, `beads`, "Code Puppy" methodology framing (lines 61, 105, 139, 142, 157). Drop the Dustin methodology line at 140 (keep copyright line at 162). |
| `CHANGELOG.md` | Rewrite line 187 to drop "beads against this release". Otherwise preserve. |
| `CONTRIBUTING.md` | Rewrite lines 6, 15, 20, 47, 49 to drop `.code_puppy/AGENTS.md` and `bd (beads)` references; route contributors through plain GitHub issues. |
| `LICENSE` | Keep. Copyright attribution is public. |
| `.gitignore`, `.gitattributes`, `.npmrc`, `.rustfmt.toml`, `.env.example` | Keep. |
| `Cargo.toml`, `Cargo.lock`, `lefthook.yml` | Keep. |

### New top-level files to author in LR.2

`INSTALL.md`, `PREREQS.md`, `SECURITY.md`, `PRIVACY.md`, `CODE_OF_CONDUCT.md`,
`ARCHITECTURE.md` (per the LR.2 brief in the planning conversation).
`ARCHITECTURE.md` currently does not exist.

### `docs/` subtree disposition

| Path | Status | LR.1 action |
|---|---|---|
| `docs/LESSONS.md` | Internal | gitignore |
| `docs/PRODUCT-STATE.md` | Internal | gitignore |
| `docs/DATA_MODEL.md` | Internal | gitignore |
| `docs/SETTINGS.md` | Internal | gitignore |
| `docs/CONTRIBUTING.md` | Internal duplicate of top-level | gitignore |
| `docs/adr/` | Internal (54 ADRs) | gitignore directory |
| `docs/phases/` | Internal | gitignore directory |
| `docs/judges/` | Internal | gitignore directory |
| `docs/knowledge-graph/` | Internal | gitignore directory |
| `docs/historical/` | Internal | gitignore directory |
| `docs/archive/` | Internal | gitignore directory |
| `docs/audits/` | Internal (this file lives here) | gitignore directory |
| `docs/reviews/` | Internal | gitignore directory |
| `docs/spikes/` | Internal | gitignore directory |
| `docs/cleanup/` | Internal | gitignore directory |
| `docs/design/` | Internal | gitignore directory |
| `docs/mobile/` | **User-facing** (iOS Shortcut recipe) | KEEP |
| `docs/research/` | Does not exist yet. LR.2 will create `docs/research/macos-implementation-notes.md` as a user-facing note. | N/A |

### `.code_puppy/` directory

| Path | Action |
|---|---|
| `.code_puppy/` (entire directory) | gitignore in LR.1 per locked decision #3 |
| `AGENTS.md` (top-level) | gitignore in LR.1 |

### External references that name internal concepts (LR.2 rewrite targets)

Captured here so the LR.2 sub-bead has a precise input list.

| File | Line | Reference | Replacement strategy |
|---|---|---|---|
| `src-tauri/tauri.conf.json` | 93 | "replacement for Wispr Flow" in `longDescription` | Rewrite to describe Mockingbird positively without comparator (LR.2). |
| `README.md` | 61 | "bd (beads)" in toolchain list | Drop the parenthetical. |
| `README.md` | 105 | `.code_puppy/` in repo layout | Remove from layout listing. |
| `README.md` | 139 | "built by Code Puppy" attribution | Replace with the one-line "Built solo with AI coding assistance" sentence permitted by locked decision #10. |
| `README.md` | 140 | "Iterations end green... (Dustin)" | Drop the methodology framing sentence. |
| `README.md` | 142 | "`.code_puppy/settings.json` enforces..." | Drop. |
| `README.md` | 157 | "`.code_puppy/AGENTS.md`" | Drop. |
| `README.md` | 162 | "(c) 2026 Dustin Boyd" | Keep (public copyright attribution). |
| `CHANGELOG.md` | 187 | "beads against this release (see `bd ready`)" | Rewrite to "open issues against this release". |
| `CONTRIBUTING.md` | 6 | `.code_puppy/AGENTS.md` link | Drop. |
| `CONTRIBUTING.md` | 15 | "initializes beads if needed" | Drop. |
| `CONTRIBUTING.md` | 20 | "uses `bd` (beads) for live task tracking" | Drop. |
| `CONTRIBUTING.md` | 47-49 | "Issues live in `bd`..." paragraph | Replace with "Issues live on GitHub". |

## Check 9: Tauri config audit

**Status:** PASS with one flagged rewrite for LR.2.

| Field | Value | Disposition |
|---|---|---|
| `bundle.active` | `true` (line 88) | Correct. |
| `bundle.longDescription` | "Local-first... replacement for Wispr Flow." (line 93) | **Flagged for LR.2 rewrite.** Locked decision #2: no Wispr Flow positioning. |
| `plugins.updater.active` | `false` (line 104) | Correct. No phone-home (Principle 4 binding). Per the `_phase7_note` at line 105, this flips to `true` only when the auto-updater ships behind ADR 0005 successor. |
| `plugins.updater.pubkey` | present (line 106) | Correct. Public keys are meant to be public; the private key lives at `%USERPROFILE%\.tauri\mockingbird.key` per `.env.example`. |
| `plugins.updater.endpoints` | `[]` (line 107) | Correct. Empty endpoint list aligns with `active=false`. |

**Recommendation:** Rewrite line 93 in LR.2. Suggested copy: "Local-first,
system-wide voice dictation and meeting capture for Windows. Privacy
respecting, fully on device, zero telemetry." This is em-dash-free and
matches the positioning Dustin chose in the planning conversation.

## Check 10: Tracked-binary scan

**Status:** PASS.

`git ls-files | findstr /R /I "\.exe$ \.dll$ \.so$ \.dylib$ \.pfx$ \.cer$ \.key$ \.pem$"`
returns **zero** matches.

The only repo-tracked binary-ish file is `Code-Puppy-Capabilities-Insight.docx`
which is already gitignored explicitly (line 55 of `.gitignore`) and present
in the working tree only because it predates that ignore rule. It will not
re-enter on future commits.

`target/`, `src-tauri/target/`, `node_modules/`, `dist/`, `build/` are
all correctly gitignored.

---

## Top 3 findings by severity

1. **react-router 6.7.0 - 6.30.3 GHSA-2j2x-hqr9-3h42 (moderate).**
   Open-redirect via protocol-relative URL. Production-relevant on paper,
   low reachability in the Tauri shell. Fix is a one-line `npm audit fix
   --ignore-scripts`. **Owner:** LR.2 sub-bead. **Blocker for LR.6 public
   flip:** soft (advisable to fix before public flip; not strictly required).

2. **cargo audit not installed on dev box at audit start.** Resolved
   in-band via `cargo install cargo-audit --locked`. **Owner:** LR.3
   sub-bead (add to CI). **Blocker for LR.6 public flip:** soft.

3. **`tauri.conf.json` line 93 references "Wispr Flow".** Already
   planned for LR.2 rewrite per locked decision #2. **Owner:** LR.2
   sub-bead. **Blocker for LR.6 public flip:** **hard.** Public flip
   without this rewrite would ship the positioning Dustin chose to drop.

## Recommendation to proceed

**Proceed to LR.0.B (Unsplash DPAPI migration) without human review.**

None of the findings require remediation before the next dispatch. All
three top findings are already scheduled into a later wave (LR.2 for the
two react-router and Wispr Flow items, LR.3 for the cargo-audit-in-CI
item). The hard blocker for the LR.6 public flip is mechanically tracked
via the LR.2 sub-bead's existence.

If any of the following had been true, this audit would have stopped to
ask the human first:

- Real secrets found in git history (would require `git-filter-repo`
  before any push)
- Tracked private key, certificate, or signing material
- Flagged `@tanstack/*` or other compromised-namespace package in the
  dependency tree
- High-severity (CVSS >= 7) vulnerability in a production dependency
- Updater `active=true` without phone-home review

None of those conditions hold. Audit complete.
