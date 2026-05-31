//! Phase 1E amendment `mb-bgpt` (ADR 0054 §C) -- **SCHEMA.md**
//! renderer.
//!
//! SCHEMA.md is the *operational contract* any LLM (chat-LLM or
//! otherwise) consults to maintain the Knowledge Graph vault
//! consistently. It is **write-once user-owned**: Mockingbird
//! seeds it on first KG activation iff missing, and never rewrites
//! it thereafter. User edits + chat-LLM appended preferences
//! survive every subsequent bootstrap call.
//!
//! # Responsibilities of this module
//!
//! - [`render_schema_md`] -- compose the canonical seed bytes. Pure
//!   (no I/O, no `now()`, no config reads). Bytes are LF-normalized
//!   and deterministic; the same Mockingbird build always emits the
//!   same SCHEMA.md.
//!
//! Write-once enforcement and atomic file emission live in
//! [`crate::vault::kg_layout::bootstrap_kg_root_files`]; this module
//! owns the *content*, not the *placement*.
//!
//! # Why deterministic + pure
//!
//! Tests pin the seed byte-for-byte. Two consequences:
//!
//! 1. Any change to the seed surfaces as a single golden update
//!    rather than a fleet of broken acceptance tests.
//! 2. Different machines / clocks emit identical files, so a
//!    user syncing the vault across desktops via Obsidian Sync /
//!    iCloud / Syncthing never sees spurious churn from
//!    re-bootstraps on each box.
//!
//! Per the ADR, SCHEMA.md is **read** by the chat-LLM, not by
//! Mockingbird's capture pipeline (which is pre-calibrated by the
//! v1 prompts). Mockingbird only writes the seed, never consumes it.

/// Render the canonical SCHEMA.md seed.
///
/// Returns owned UTF-8 bytes (`String`) with LF-only line endings,
/// exactly the bytes that get written to `<vault>/Knowledge
/// Graph/SCHEMA.md` on first KG activation.
///
/// Deterministic: no clock, no env, no config. Two calls in one
/// process return identical bytes; two installs of the same
/// Mockingbird build do too.
pub fn render_schema_md() -> String {
    // The seed is one long, opinionated, doc-style Markdown
    // composed from a single contiguous literal. The literal is
    // LF-terminated (Rust raw string literals on Unix-style source
    // files), but the LESSONS PINNED P12 Finding 1 reminder still
    // applies at the *write* site -- we hand bytes back here, and
    // `bootstrap_kg_root_files` writes them with `std::fs::write`
    // which preserves bytes verbatim. The end-to-end LF invariant
    // is pinned by the `root_files_have_lf_only_line_endings`
    // test in `kg_layout.rs`.
    SCHEMA_MD_SEED.to_string()
}

/// Frozen canonical SCHEMA.md seed -- the operational contract the
/// chat-LLM consults to maintain this vault.
///
/// Edit with care: every byte change is a user-visible breaking
/// edit to the vault contract. Bump `schema_version` in the
/// frontmatter when shipping a non-additive change, and document
/// the diff in an ADR.
const SCHEMA_MD_SEED: &str = r#"---
schema_version: 1
managed_by: mockingbird
contract: personal-knowledge-engine
adr: 0054
---

# SCHEMA.md -- Personal Knowledge Engine contract

This file is the **operational contract** for any LLM (chat-LLM or
otherwise) that maintains this Knowledge Graph vault. Mockingbird
captures and synthesizes; the chat-LLM ingests, queries, and lints.
Together they keep the wiki coherent.

> Reference: Karpathy "LLM Wiki" gist; Alvin Clark "Building a
> Personal Knowledge Engine with LLMs and Obsidian" (April 2026).
> Pattern is the architectural great-grandchild of Vannevar Bush's
> Memex (1945).

This file is **write-once**: Mockingbird wrote the initial seed,
but never rewrites this file. Edit it freely; your edits survive
every future KG activation.

---

## Folder structure

```
Knowledge Graph/
  SCHEMA.md            <- this file (write-once; you own it)
  INDEX.md             <- auto-maintained catalog (Mockingbird rebuilds)
  LOG.md               <- append-only operations log (both agents append)
  Entries/             <- source captures (Mockingbird writes; reverse-watcher reconciles user edits)
  Entities/            <- per-entity stub pages (Mockingbird seeds; you/chat-LLM enrich)
  Projects/            <- per-project stub pages (same write-once contract)
  Tags/                <- per-tag stub pages with Dataview rollups
  History/             <- raw immutable session sidecars (Layer 1)
  Inbox/               <- mobile capture intake (KG-Inbox courier)
```

Mockingbird-managed folders are listed above. Anything else under
`Knowledge Graph/` is yours -- create `Concepts/`, `Sources/`,
`Runbooks/` subtrees freely; Mockingbird will not touch them.

## Type vocabulary -- the nine knowledge shapes

Every `Entries/*.md` file declares its knowledge shape in YAML
frontmatter `type:`. The canonical set:

| Shape | Meaning |
|---|---|
| `source` | The Entry is itself the source material -- a transcript, memo, raw capture. Default. |
| `note` | Short observation or aside; quick thought or brief reaction. |
| `concept` | Definition or explanation of an idea. Tends to migrate to a chat-LLM-authored concept page. |
| `entity` | Reference-to-an-entity Entry. Body is *about* the entity; durable home is `Entities/<slug>.md`. |
| `project` | Reference-to-a-project Entry. Body is *about* the project; durable home is `Projects/<slug>.md`. |
| `question` | Open question awaiting answer. Useful for Lint passes. |
| `decision` | Point-in-time decision record with rationale. Lightweight ADR shape. |
| `reference` | Pointer to external material (URL, book chapter, article). |
| `observation` | Empirical noticing -- pattern, anomaly, surprise. Raw material for Lint to crystallize into concepts. |

Every Entry is **implicitly a source** regardless of `type:` --
the raw capture is preserved in `History/`. The `type:` field is
the *knowledge shape*, not the *origin classification*.

Pre-pivot entries written under the legacy `task`/`event`/`idea`
vocabulary are tolerated on read (parser re-classifies as `note`)
but new writes use the canonical set above.

## Frontmatter conventions

```yaml
---
id: "01HMxxxxxxxxxxxxxxxxxxxx"     # ULID, unique per Entry
schema_version: 1
capture_kind: "dictation"           # dictation | kg-note | meeting | text-note | mobile-import
captured_at: "2026-06-15T14:32:01Z" # RFC 3339, UTC
title: "..."                        # short, declarative; auto-derived if absent
category: "personal"                # personal | professional | other
type: "source"                      # one of the nine knowledge shapes
tags: ["tag-slug", ...]             # kebab-case ASCII
entities: ["[[Entities/<slug>|<slug>]]", ...]  # wiki-link with display alias
source_session_uuid: "..."          # links Entry to History/<YYYY-MM>/<uuid>.json
status: "todo"                      # optional; opt-in Obsidian Tasks workflow
due_date: "2026-06-20T00:00:00Z"    # optional; pairs with status
---
```

Field order is byte-stable; the serializer pins it. Optional
fields omit entirely when absent (no `null` placeholders).

## Wiki-link conventions

- **Entities:** `[[Entities/<slug>|<slug>]]` (pipe-alias display
  form; the bare slug is the durable token).
- **Projects:** `[[Projects/<slug>]]`.
- **Tags:** `#<slug>` for inline, `[[Tags/<slug>]]` for a typed
  backlink that surfaces in Obsidian's graph view.
- Slugs are kebab-case ASCII; non-ASCII titles round-trip via the
  normalizer.

## INDEX.md format

`INDEX.md` is auto-maintained by Mockingbird. Five H2 sections,
each alphabetical (Sources is most-recent-first):

```markdown
## Sources
## Entities
## Projects
## Tags
## Concepts
```

Mockingbird owns Sources / Entities / Projects / Tags. The
Concepts section is yours and the chat-LLM's -- Mockingbird never
touches it.

INDEX.md is rebuilt from the database after every successful KG
filing. **The database is the source of truth for INDEX.md** -- if
you hand-edit a Mockingbird-managed section, the next filing
overwrites your edit. Edit the underlying Entry frontmatter
instead and the reverse-watcher will reconcile, then INDEX.md
will reflect the change on the next filing.

## LOG.md format

Append-only operations log. Both agents append; nobody rewrites
historical lines.

```markdown
## [YYYY-MM-DD HH:MM] capture | <title>
## [YYYY-MM-DD HH:MM] ingest | <source title>
## [YYYY-MM-DD HH:MM] query | <question>
## [YYYY-MM-DD HH:MM] lint | <summary>
```

Timestamps are UTC. One blank line between entries. Mockingbird
only emits `capture` operations; the chat-LLM emits
`ingest`/`query`/`lint`.

## Workflows -- Ingest / Query / Lint

These are the chat-LLM's responsibilities. Mockingbird does
first-pass synthesis on capture; the chat-LLM does the deep pass.

### Ingest

When a new source enters context (article, photo, transcript,
link, or a freshly-captured `Entries/<date>-<slug>.md` file):

1. Read this SCHEMA.md for vault conventions.
2. Read related existing pages (entities mentioned, adjacent
   concepts, the project page if applicable).
3. Synthesize a source-summary page in `Concepts/` or
   `Sources/` (your choice; SCHEMA-extend below if needed).
4. Ripple updates: refresh entity pages with new mentions; update
   project pages' Recent activity; add new concept pages if the
   source introduces them; resolve contradictions with stale
   claims.
5. Append `## [<UTC>] ingest | <source title>` to LOG.md.

A single Ingest can touch 10-15 pages. This is the high-leverage
operation in the Karpathy pattern.

### Query

Answer a user question against the vault:

1. Search/grep relevant pages (frontmatter queries, full-text,
   Dataview tag rollups).
2. Drill into the relevant pages.
3. Synthesize an answer with inline citations linking back to
   `[[Entries/<file>]]` / `[[Concepts/<page>]]` / etc.
4. If the answer is general-purpose, file it back as a new concept
   page or appendix so future queries compound on it.
5. Append `## [<UTC>] query | <question>` to LOG.md.

### Lint

Periodic health check. User-invoked or self-scheduled:

1. Walk the vault collecting structural facts:
   - Entities mentioned but missing a page (orphans).
   - Pages with no inbound links.
   - Stale claims where the source was updated but downstream
     pages didn't ripple.
   - Contradictions between pages.
   - Tags used but missing a `Tags/<slug>.md` page (rare;
     Mockingbird seeds these on capture).
2. Surface a punch-list in chat for the user to triage.
3. With user approval, run fix operations (create missing entity
   pages with stub content; ripple stale claims; merge duplicate
   entities).
4. Append `## [<UTC>] lint | <summary>` to LOG.md.

---

## User preferences

Edit this section freely. The chat-LLM should consult these
preferences on every operation. Mockingbird does not consume this
section; it is purely for the chat-LLM.

- (seeded empty -- the chat-LLM appends to this section as
  preferences crystallize over time)
"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// Determinism contract -- two calls return byte-identical
    /// output regardless of how many times invoked.
    #[test]
    fn render_is_deterministic() {
        let a = render_schema_md();
        let b = render_schema_md();
        let c = render_schema_md();
        assert_eq!(a, b);
        assert_eq!(b, c);
    }

    /// LF-only invariant -- LESSONS PINNED P12 Finding 1.
    #[test]
    fn render_is_lf_only() {
        let out = render_schema_md();
        assert!(
            !out.contains('\r'),
            "SCHEMA.md seed must be LF-only on the wire"
        );
    }

    /// Contract pins -- if any of these strings change, downstream
    /// chat-LLM behavior changes. Pinned so we notice.
    #[test]
    fn render_contains_load_bearing_strings() {
        let out = render_schema_md();
        // Header / lineage
        assert!(out.contains("schema_version: 1"));
        assert!(out.contains("Personal Knowledge Engine"));
        assert!(out.contains("Karpathy"));
        assert!(out.contains("Alvin Clark"));
        assert!(out.contains("Memex"));
        // Nine knowledge shapes
        for shape in [
            "source",
            "note",
            "concept",
            "entity",
            "project",
            "question",
            "decision",
            "reference",
            "observation",
        ] {
            assert!(
                out.contains(&format!("`{shape}`")),
                "SCHEMA.md must document shape `{shape}`"
            );
        }
        // The three workflows
        assert!(out.contains("### Ingest"));
        assert!(out.contains("### Query"));
        assert!(out.contains("### Lint"));
        // Format specs the chat-LLM follows
        assert!(out.contains("## Sources"));
        assert!(out.contains("## Entities"));
        assert!(out.contains("## Projects"));
        assert!(out.contains("## Tags"));
        assert!(out.contains("## Concepts"));
        // User preferences section header (the chat-LLM appends here)
        assert!(out.contains("## User preferences"));
    }

    /// SCHEMA.md must start with a YAML frontmatter block so any
    /// chat-LLM parser that uses the convention can pick up the
    /// `schema_version` / `managed_by` / `contract` fields without
    /// regex-hunting through the body.
    #[test]
    fn render_starts_with_frontmatter() {
        let out = render_schema_md();
        assert!(
            out.starts_with("---\n"),
            "SCHEMA.md must open with a frontmatter delimiter"
        );
        // Second `---` closes the frontmatter; must appear before
        // the H1.
        let body_start = out.find("\n# ").expect("must contain an H1");
        let frontmatter_close = out[..body_start]
            .rfind("---")
            .expect("must close the frontmatter before the H1");
        assert!(frontmatter_close > 0);
    }
}
