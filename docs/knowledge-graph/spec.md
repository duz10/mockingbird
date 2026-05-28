# Mockingbird Knowledge Graph — Phase 0 Validation & v1 Product Spec

**Document type:** Product specification (experience-level, technology-agnostic)
**Audience:** Codebase implementation agent + product owner
**Author intent:** Define *what* to build and *why*, and the standard the work must meet. Technical implementation decisions are delegated to the codebase agent.
**Status:** Ready for implementation, starting at Phase 0.

---

## 0. How to read this document

This spec is deliberately split into two parts:

1. **Phase 0 — Pipeline Validation (a prerequisite gate).** Before any production feature is built, the agent builds an *isolated test harness* that proves the extraction pipeline works on realistic, messy human input. Phase 0 produces a go/no-go signal and a set of tuned prompts. It is not a feature; it is the evidence that unlocks the real build and tells us how ambitious v1 can safely be.

2. **v1 — The Knowledge Graph feature.** The actual user-facing capability: drop a voice memo, get back clean, tagged, status-tracked, actionable entries inside an Obsidian vault, synced to mobile.

The agent should **complete Phase 0 first**, report results against the thresholds defined here, and only then proceed to v1 — scoping v1 (lighter vs. fuller) based on what Phase 0 demonstrates the local models can reliably do.

A guiding principle throughout: **this is a tool for a general, average person**, not for a power user with a custom tagging discipline. Every design choice should be tested against "would this work for someone who just talks into their phone with zero system knowledge?"

---

## 1. Background & what already exists

Mockingbird is a local-first, desktop-hub audio-processing application. A proven capability already exists and must be **reused, not rebuilt**:

- A folder-watcher monitors a synced Obsidian vault directory on the desktop hub.
- When an audio file (e.g. `.m4a` voice memo) appears, Mockingbird processes it, transcribes it, and writes a dictation record back into the vault.
- Obsidian's own sync propagates the result back to mobile, where the user sees the finished dictation.
- Processing is **deferred**: if the hub is offline, audio queues and is processed when the hub comes back online.
- The team has adopted a **multi-pass approach** for local small models — breaking complex work into small, single-purpose LLM passes so each stays focused and accurate within the limits of small local models.

The Knowledge Graph feature is a **bolt-on that reuses this exact backbone** (watch → queue → multi-pass process → write back → sync). It adds a second *processing mode* on top of the watcher that already works. It does not replace standard dictation.

---

## 2. The core experience promise

> A user talks into their phone with zero discipline — no spoken category labels, no tag syntax, no structure — drops the memo, and trusts that it comes back as clean, findable, status-tracked, actionable entries inside Obsidian.

The system does the structuring. The human just talks. The moment a user must remember syntax *while dictating*, the value is lost. Everything below serves this promise.

A second, equally important promise governs the deferred nature of the system:

> Dropping the memo is "done" from the user's side. The structuring happens whenever the hub is next online. The user must never feel the lag or be left unsure whether something was captured.

This matters because the well-documented failure mode of self-hosted, not-always-on processing systems is precisely that they work when the machine is on and silently fail to capture the rest of the time — which is exactly when most worthwhile thoughts occur. Mockingbird's queue-and-process design is the correct answer to this, but it places a hard requirement on the experience: **capture (instant, mobile) and processing (deferred, hub) must be cleanly decoupled and the system's state must always be glanceable.**

---

## 3. Research context that shaped this spec

The design is grounded in current, validated patterns. The agent does not need to re-derive these, but should understand the "why":

- **The LLM-Wiki / "second brain" pattern (2026).** The dominant approach treats the LLM as a compiler that turns raw input into structured, interlinked markdown — "Obsidian is the IDE, the LLM is the programmer, the wiki is the codebase." Notably, leading implementations use **no vector database and no embeddings** — just human-readable markdown files on disk, browsable in Obsidian's graph view even with no LLM running. We adopt this philosophy: plain markdown, human-readable, no hidden datastore.
- **Compounding value.** The reason the pattern beats an ordinary note pile is cross-referencing: each new entry woven against existing ones, so the corpus gets *more* useful over time rather than becoming a junk drawer. (This is the prize we are sequencing toward — see §9.)
- **The Obsidian Tasks plugin (free, open-source, de facto standard)** uses an inline markdown format for tasks with emoji-encoded metadata (priority, due date, recurrence, project tag). Writing tasks in this format means the entire ecosystem works for free — desktop queries via the Tasks plugin, and mobile widgets/notifications/kanban via the separate **TaskForge** native app, which reads/writes the same format. *Note for the owner: "TaskForge" is a commercial native mobile app, not an open-source plugin; the open-source layer is the Tasks plugin. We do not build either — we emit their format and inherit the ecosystem.*
- **Task dependencies are NOT natively supported** by the Tasks plugin (long-standing open request). Therefore dependency-based / "don't surface B until A is done" flows are explicitly a **v2+ concern**, not something we inherit or build in v1.
- **Hybrid tagging is the established best practice.** Information-science literature is consistent: controlled vocabularies give clarity but demand discipline; open "folksonomy" tags are flexible but suffer ambiguity and synonym fragmentation; the strongest systems combine a controlled top layer with an open bottom layer, plus a normalization mechanism that produces "terminological consensus." Our three-layer tag system (§7) is exactly this hybrid, with the LLM's normalization pass automating the consensus that crowds normally produce slowly.

---

# PART A — PHASE 0: PIPELINE VALIDATION (PREREQUISITE GATE)

## 4. Purpose of Phase 0

Turn the "can the local models actually do this well?" question from a guess into a measurement. Phase 0 builds a sandboxed harness, generates a realistic test corpus *with an independent answer key*, runs the multi-pass extraction pipeline against it, scores the output against pre-set thresholds, and checks run-to-run stability. 

**Phase 0 deliverables:**

1. A **simulated test corpus** (raw dictations) with a **ground-truth answer key**, authored together.
2. The **multi-pass extraction prompts** for the local models.
3. A **scored validation report** against the thresholds in §8.4.
4. A **two-run stability comparison**.
5. A **recommendation**: which v1 scope (lighter vs. fuller — see §9) the results support.

Passing the gate unlocks Part B.

## 5. Isolation discipline (non-negotiable engineering constraints)

These rules exist so the experiment can never muddy the working production app, and so the owner always knows where production ends and experiment begins.

- **5.1 — Sandbox folder.** All Phase 0 work (corpus, answer keys, harness, scoring scripts, run reports, experimental prompts) lives under a single, clearly named directory (e.g. `knowledge-graph-phase0/` or `experimental/kg-validation/`). Deleting that folder must leave the production dictation app completely untouched. This is the blast-radius container.
- **5.2 — Reuse by reference, never by copy.** When Phase 0 needs an existing production capability (transcription, the folder-watcher, etc.), it **imports / calls the existing production module as-is**. It must not copy production source code into the sandbox. Duplication is the specific thing that causes "which copy is real?" confusion later. The mental model: *Phase 0 is a test harness wrapped **around** the existing engine, not a fork **of** it. New scaffolding, old engine.*
- **5.3 — Do not modify production files.** Phase 0 may read from, import, and call production modules, but writes only inside its own sandbox folder. If the agent finds it genuinely needs to change a production module to make the test work, it must **stop and flag this to the owner** rather than silently editing. This single rule guarantees the owner can always return to known-good primary functionality, because primary functionality was never altered. Any ambiguity becomes a conversation, not a hidden change.
- **5.4 — Deliberate duplication is allowed only for features under active change**, and only as a clearly-marked new file inside the sandbox that never overwrites the original.

## 6. The test corpus

### 6.1 The critical methodology rule: independent ground truth

The single most important instruction in Phase 0:

> The corpus must be built in **two parts authored together**: (a) the raw rambling dictation text a human would actually speak, and (b) a hand-specified **answer key** for that dictation — written at authoring time, *not* generated by the model under test.

If the same model both produces the structured output and grades it, nothing is learned — the grader just agrees with the processor. The answer key is the independent standard that makes the grade real. This mirrors the established "gold standard set of 30–50 labeled examples" calibration practice for evaluating LLM pipelines.

For each dictation, the answer key specifies:
- How many distinct entries it **should** split into.
- The correct **category** (one of the controlled set, §7).
- The correct **type** (§7).
- The dates that **should** be extracted — *and explicitly, the ones that should be left empty*.
- A reasonable set of **topic tags** (allowing for acceptable variation — see §8.3).
- Whether it is a "junk / no real content" case that should produce no entry (or a flagged-for-review entry).

### 6.2 Corpus construction: personas × difficulty (two-axis design)

The corpus must represent a **general, average American** across class and life circumstance — explicitly NOT optimized for the product owner's personal domains. The agent generates dictations by crossing a set of personas with a set of difficulty types. This two-axis design is what produces genuine coverage instead of fifty variations of one voice.

**Personas (each generates several dictations).** Suggested set the agent should build from — adjust as reasonable, but keep the class/life-stage spread:

- A **working-class hourly earner** — shift schedules, car repairs, utility bills, kids' school logistics.
- A **lower-middle-class tradesperson / service worker** — job leads, tool purchases, client follow-ups interleaved with family matters.
- A **salaried middle-class professional** — work projects bleeding into evenings, home improvement, planning a vacation.
- An **aspiring-middle-class side-hustler** — the messiest, most valuable case: personal/professional boundaries genuinely blurred.
- A **caregiver / parent running a household** — appointments, school forms, groceries, a stray personal goal.
- A **recent grad / early-career renter** — job applications, budgeting, social plans.

Content should feel mundane, varied, and emotionally normal: a car repair, a kid's permission slip, a vague work idea, a recipe to try, a bill to dispute, a gift to buy, a half-formed business thought, a doctor's appointment to make.

**Difficulty types (the corpus must span all of these):**

| Type | What it stresses | Suggested share of ~30 |
|---|---|---|
| Clean single-item | Baseline. If these fail, stop. | ~8 |
| Multi-item rambler | Segmentation (the riskiest pass) | ~10 |
| Ambiguous category | The pick-one classifier (e.g. side-hustle = personal or professional?) | ~6 |
| No date mentioned | Tests that the model leaves due dates **empty** rather than inventing them | ~4 |
| Near-empty / junk | Non-content handling ("uh, the thing… never mind") | ~2 |

### 6.3 Corpus size

**Minimum viable: 30 dictations**, weighted toward the hard cases (the easy ones teach little). 50 is better if generation is cheap (and since the agent authors them, it likely is) — but **30 well-designed beats 50 lazy ones**. Each dictation should vary in length and complexity, consistent with how real people actually ramble.

## 7. The structuring standard the pipeline must produce

This is the schema the extraction must emit. It is also the v1 data model, so Phase 0 validates against the real target. Output is **YAML frontmatter + markdown body** — human-readable, no hidden datastore.

### 7.1 Folder structure (lifecycle, not meaning)

Under `Knowledge Graph/`, exactly three top-level folders:

- **`Inbox/`** — raw audio lands here; after processing, audio is moved out. Inbox should be near-empty; an empty inbox is the signal "everything is processed."
- **`Entries/`** — every structured note lives here, **flat** (not nested by category). Nesting by category would imprison cross-cutting notes and make re-categorization a file move. Folders are for *lifecycle*; meaning is carried by tags.
- **`History/`** — processed audio + the raw transcript, kept untouched as the re-processing safety net (and the source of truth that allows backfilling later — see §9).

### 7.2 The three-layer tag system (controlled top, open bottom, normalized)

This is the heart of the design and the resolution of the standards-vs-flexibility tension.

- **Layer 1 — Category (controlled, pick exactly one).** `personal`, `professional`, `objective`. Closed set; the coarse filter the user leans on most. This is the only truly rigid field, and that rigidity is a feature. *(Phase 0 may surface that these three are insufficient or confusing for the general population — that is a valuable finding to report, not a constraint to silently break.)*
- **Layer 2 — Type (controlled, small stable set).** `task`, `research`, `idea`, `note`, `reference`. Drives behavior: a `task` gets a status and possibly a due date and a checkbox; a `note` just sits there. Kept short and stable because the future task/visualization layer keys off it.
- **Layer 3 — Topic tags (open, LLM-generated).** The flexibility valve. Free-form topical tags extracted from content (`#mockingbird`, `#taxes`, `#reading-list`). No ceiling. The discipline that keeps this from fragmenting is **not** a fixed list — it is a **normalization pass**: lowercase, hyphenated, singular, so "Mockingbird," "mockingbirds," and "the Mockingbird app" all collapse to `#mockingbird`. This single rule is what keeps an open vocabulary searchable. It is the automated equivalent of the "terminological consensus" that crowd folksonomies reach slowly.

### 7.3 Per-entry fields (all inferred, never required of the user)

- **Title** — short, human-readable, generated from content.
- **Category / Type / Topic tags** — the three layers above.
- **Status** — for `task` type: `todo` / `doing` / `done` (default `todo`). Omit for non-tasks.
- **Date captured** — automatic.
- **Due / target date** — **only if the user actually mentioned timing** ("by Friday," "before the trip"). Never invented. An empty due date is honest; a guessed one erodes trust.
- **Body** — the cleaned transcript, plus a link back to the raw audio/transcript in `History/`.

### 7.4 Tasks use the Obsidian Tasks format (inherit the ecosystem)

When an entry's type is `task`, the pipeline writes a native Obsidian Tasks checkbox line in the body, with that plugin's metadata format (due date, priority, tags), in addition to the frontmatter record. Frontmatter describes the entry-as-record; the checkbox line makes it actionable. This makes tasks work in desktop Obsidian (Tasks plugin) and on mobile (TaskForge) with zero additional build on our side. **Dependencies between tasks are out of scope for v1** (not natively supported).

### 7.5 One-memo-many-entries: SPLIT

A single rambling memo often contains several to-dos and a stray idea. The pipeline must **segment one audio file into multiple entries** when content clearly contains distinct items. One-memo-one-note produces unsearchable blobs that defeat the purpose. The multi-pass approach makes this natural (a segmentation pass, then a structuring pass per segment). This is the riskiest pass and is weighted heavily in the corpus accordingly.

## 8. The validation harness, scoring, and gate

### 8.1 The multi-pass pipeline under test

A focused, single-purpose pass sequence suited to small local models (the agent may refine the exact decomposition, but this is the intended shape):

1. **Transcribe** — raw text (reuses existing production capability).
2. **Segment** — split into distinct candidate entries.
3. **Classify** — assign Layer 1 category + Layer 2 type per entry.
4. **Extract** — pull dates (or none), generate title, propose Layer 3 topic tags.
5. **Normalize + write** — normalize tags, write frontmatter + body + task checkbox.

### 8.2 Scoring against the answer key

For each dictation, compare pipeline output to the hand-authored answer key on: segmentation count correctness, category correctness, type correctness, date correctness (including correct *absence* of dates), and tag normalization correctness.

### 8.3 Where an LLM is used to judge "correctness," guard it

Topic-tag correctness is partly subjective (is `#car-repair` an acceptable match for an expected `#auto-maintenance`?). If an LLM is used to judge semantic tag equivalence:
- It must give **reasoning before its verdict** (chain-of-thought), which measurably improves judge reliability.
- Its judgments must be **spot-checked by the owner on a sample** — an LLM judge is a well-informed suggestion, not absolute ground truth, and should be validated rather than blindly trusted.
- Provide the judge the **answer key as reference** so it compares against the intended answer rather than its own internal opinion.

### 8.4 Pre-set thresholds (chosen BEFORE seeing results)

Thresholds are fixed in advance so outcomes aren't rationalized after the fact. Tune the numbers to taste before running, but commit before running:

| Metric | Threshold | Type |
|---|---|---|
| Clean single-item handled correctly | ~100% | Hard floor — if these fail, halt |
| Segmentation correct on multi-item cases | ≥ 85% | Gate |
| Category correct | ≥ 90% | Gate |
| Type correct | ≥ 85% | Gate |
| Invented dates across the no-date set | **0** | **Hard gate** (trust-critical) |
| Tag-variant collapse correct | ≥ 80% | Gate |

### 8.5 Two-run stability check

Run the entire corpus through the pipeline **twice** and compare the two runs *against each other*, not only against the answer key. Small local models are non-deterministic; if the same dictation tags or splits differently across runs, that instability is itself a finding and means prompts need tightening before any accuracy number is trustworthy. Cheap to add, highly revealing.

### 8.6 The report

A single human-readable report: per-metric scores vs. thresholds, the stability findings, notable failure examples, any signal that the controlled vocabularies (categories/types) are wrong for the general population, and a clear **go / no-go** plus a **scope recommendation** for v1.

---

# PART B — v1: THE KNOWLEDGE GRAPH FEATURE

## 9. v1 scope: lighter, built on graph-ready bones

**Recommended scope (pending Phase 0 results):** ship the **lighter v1** — reliable capture → segmented, three-layer-tagged, status-tracked entries, with native Obsidian Tasks checkboxes — **but design the schema as if the fuller graph were coming next.**

The reasoning:
- The whole system only works if the user trusts it. Lighter v1 proves the loop on verifiable output (transcribe, split, tag, write, checkbox), so errors are caught at a glance and trust is earned fast.
- It respects the small-local-model constraint: segmentation + classification + normalization is already several focused passes — a realistic load.
- "Lighter" must not mean "throwaway." Because v1 entries already carry clean frontmatter, normalized tags, and a stable `type`, the later entity/concept cross-linking layer (v1.5) becomes *a new pass over well-formed data*, not a migration.
- Because the raw transcript is retained in `History/` as source of truth, deferring the graph costs nothing permanent: when entity extraction arrives, the back catalogue can be **reprocessed and retroactively woven into the mesh.** Deferred ≠ lost history.

**Flip to fuller v1** (entity/concept cross-linking from day one) only if Phase 0 demonstrates the local models already handle entity resolution reliably — i.e. consistently recognizing that "the Mockingbird app," "Mockingbird," and "MB" are one entity. Noisy entity extraction is worse than none, because a polluted graph looks authoritative while being wrong.

## 10. v1 deferred-processing UX (the decoupling requirement)

Because the hub isn't always on, the experience must make the two-promise model (§2) tangible:

- **Capture is instant and local.** Dropping the memo on mobile is the user's entire job and is "done" immediately.
- **Inbox state is glanceable.** The `Inbox/` folder should visibly reflect processing state — audio present = queued; audio gone + entry appeared in `Entries/` = processed. A lightweight status convention (e.g. a `_processing` / `_done` marker or a filename change) lets a single glance answer "did it work / am I caught up?" This is the single biggest UX risk of a deferred system and is pure experience design, not a technical detail.
- **Dual-write.** Every knowledge-graph capture is also saved as a standard dictation record (the existing behavior), so the raw transcript always exists independently of the structured entry.

## 11. What "done" means for v1 (day-one usefulness)

The day-one win is simple and complete on its own: **talk → segmented, tagged, status-tracked entries appear in Obsidian; tasks are actionable as native checkboxes on desktop and mobile; pre-built Kanban and dashboard views (§14) let the user observe and manage projects with edits syncing back; an empty inbox tells you you're caught up.**

Explicitly **not** in v1: entity/concept cross-linking (v1.5), dependency-based task flows and dependency-aware boards (v2+), any vector database or embeddings (not part of this architecture at all). Note that *non-dependency* boards and dashboards **are** in v1 — only the dependency-awareness on top of them is deferred. These deferred items are layers built *on top of* the well-formed data v1 produces — not prerequisites for it.

## 12. The single biggest risk to watch

It is not missing features — it is **inconsistent tagging eroding trust.** If the same concept receives three different tags across three memos, search breaks and the user stops feeding the system. The Layer 3 normalization pass is therefore not a nice-to-have; it is the mechanism that determines whether this feature lives or dies. Phase 0's tag-normalization threshold and stability check exist specifically to de-risk this before a single production entry is written.

---

## 13. Ecosystem expectations: what the experience is on each surface

A core principle of this spec: **the user experience differs by where in the ecosystem the user is, and the plan must set the right expectation for each surface rather than pretending they are identical.** The underlying data is one set of plain markdown files; what changes is what each surface is good at. Designing as if mobile and desktop offer the same thing would set the user up for disappointment on a phone and underuse of the desktop.

### 13.1 The boundary of what Mockingbird builds

Mockingbird's responsibility **ends at writing clean, standard-format markdown into the vault** — capture, local processing, structured write-back, sync. Everything past that (boards, widgets, notifications, graph visuals) is provided by Obsidian and its open-source plugin ecosystem rendering *our* files. We do not build a task app, a board app, or a mobile app of our own.

Mockingbird's desktop app does, however, provide a **control and overview surface** for the feature (settings + a read-only dashboard + a launch-into-Obsidian handoff — see §15). This is deliberately *not* an editing or board-building surface in v1: all editing and all spatial board/graph work happen in Obsidian. Mockingbird **observes** the data and **controls** the pipeline; Obsidian is where the data is **worked**. (This is the "Option A — Mockingbird as control surface" decision; richer in-app rendering is revisited only if the Obsidian experience proves insufficient.)

### 13.2 No third-party dependency (the open-format guarantee)

Mockingbird emits the **open Obsidian Tasks plugin format** (plain-text checkbox syntax with standard metadata) and the open frontmatter/tag schema in §7. It depends on **no third-party task application or its servers.** Any such app (e.g. the commercial TaskForge native apps, or the open-source Tasks/TaskNotes/Kanban plugins) is an **optional, user-chosen viewer** of files that already live in the user's own vault. This means:

- We never route user data through anyone else's servers, and we never inherit a third party's privacy posture. Our value lives in the open files, so the user is locked neither to us nor to any viewer.
- The same files render in multiple tools with no migration, which *is* the portability benefit — the user can change or stack viewers without changing their data.
- If a user personally chooses to install a paid viewer for extra mobile polish, that is their choice on their device, entirely separate from what Mockingbird does or requires.

### 13.3 Desktop = the workshop

Desktop Obsidian is where the spatial, overview-heavy work happens and where the experience is fullest:

- The **graph view** (relationships across the whole corpus) is usable and valuable on a large screen.
- **Kanban-style drag-and-drop boards** and **query-driven tables/dashboards** (via standard plugins reading our files) give the "observe all the projects and their relationships over time" PM surface.
- This is the place for planning, rearranging, and seeing the web of connections.

### 13.4 Mobile = capture, triage, and management (not the canvas)

Mobile Obsidian runs the same core plugins, including graph view, but with real ergonomic limits that the plan must respect rather than paper over:

- **Strong on mobile:** capture (the whole point of the phone), search, tags, backlinks, viewing entries, and **full editing that syncs back** — checking off a task, renaming, changing due dates, editing notes, adding tags. The query-driven dashboard/board notes (§14) render as clean, tappable lists that read well on a phone.
- **Weak on mobile:** the *global graph* becomes a hairball on a small screen at any real scale, and the *visual drag-around-the-canvas board* is cramped and awkward to manage. Navigating out of the graph into a note is clunky.
- **Therefore:** on mobile, surface connections as **lists and filters**, not as the graph visual. Mobile is for *managing* board items (edit, complete, re-tag, re-date — all syncing back) and *viewing* projects readably; it is not the place for spatial board rearrangement. A 6-inch screen was never going to do canvas planning well regardless of tooling, so almost nothing functional is lost — only the comfortable drag-canvas, which stays a desktop activity.

### 13.5 Mockingbird desktop app = control + read-only overview

The Mockingbird app itself is a third surface, and its job is narrow and clear (full detail in §15):

- **Capture:** start a new dictation from the app's microphone, identical to a regular dictation but routed through the knowledge-graph pipeline.
- **Control:** the settings/admin that govern how the feature behaves (vault/folder, vocabularies, processing/queue, dual-write).
- **Read-only overview:** a high-level dashboard of the current state read from the vault — counts and light insights, not an editable board.
- **Handoff:** a launch button that opens the connected vault in Obsidian for the full spatial experience.

It is explicitly **not** where editing or board management happens in v1. That keeps Mockingbird from duplicating Obsidian's editing UI and avoids any second source of truth.

### 13.6 The expectation to set with the user

> Capture and manage anywhere; plan spatially on desktop. Whatever you change on any surface is the same underlying file, so it shows up everywhere.

## 14. Output & board experience (bidirectional by default)

This section answers the "I want a Monday-style board to observe and manage projects, and I want my edits to flow back" requirement. The important point: **the bidirectional sync is the native behavior of building on plain files — it is not something we engineer.**

### 14.1 The file is the database

There is no separate datastore that a board is a "view" of. **The markdown files *are* the database.** A Kanban board, a checklist, a filtered table, and the graph are all **live renderings of the same files.** When the user drags a card, checks a box, or edits a field in any of those renderings, the tool **writes the change straight back into the markdown.** That is the bidirectional loop:

> Voice in → local LLM distributes it into the knowledge-graph files → those files render as boards/lists/graph wherever viewed → the user edits a rendering → the edit writes back to the file → the file is the single source of truth that every other view re-renders from.

One source, many views, edits flow both ways. This is the payoff of choosing plain files over a proprietary store, and it is why we get the board experience without building a board engine.

### 14.2 What we ship so the user gets boards out of the box

The user must not be handed a blank vault and homework. A concrete v1+ deliverable is a **small set of pre-built board and dashboard notes** so working views exist immediately:

- A **Kanban board** keyed to `type: task` and `status` (e.g. To Do / Doing / Done columns), where cards are our task entries and moving a card updates the file.
- **Query-driven dashboards** keyed to our frontmatter, `type`, tags, and due dates — e.g. "open tasks due this week," "everything in a given project/topic," "recent ideas" — which double as the clean mobile lists from §13.4.

These are configured by us against the schema in §7 so they work on day one; the user just uses them.

### 14.3 What edits are respected, and where

Because every view edits the underlying file, the following all sync back from any surface that supports the edit: completing/checking a task, renaming an item, changing due dates, editing notes/body, changing tags, and (on desktop boards) moving items between columns/statuses. Field-level editing is available on both desktop and mobile; spatial rearrangement of the board canvas is a desktop strength (§13.4).

### 14.4 The limit to keep in view

**Task dependencies** (B cannot start until A is done) are not native to the Obsidian Tasks ecosystem, so dependency-aware boards remain the **v2+** custom layer already flagged. Everything else of a PM board — columns, statuses, due dates, grouping, filtering, and bidirectional editing — is available now over the data v1 produces.

---

## 15. Mockingbird desktop app — the in-app feature surface (Option A: control surface)

This section defines what the Knowledge Graph feature looks like *inside the Mockingbird desktop app itself*, as distinct from what happens in Obsidian. The v1 decision is **Option A: Mockingbird is the capture-and-control surface; Obsidian is the board/editing surface.** This is the lightest build, leans on the Obsidian work that has to happen anyway, and is revisited only if the Obsidian experience proves insufficient for the desired Monday-style experience.

### 15.1 Design principle: observe and control, do not edit

In v1, the Mockingbird app **reads** the knowledge-graph data and **controls** the pipeline. It does **not** edit *existing* entries, render an editable board, or build a graph canvas — all of that is Obsidian's job (§13, §14). Capturing *new* input (audio or text, §15.5) is allowed and is not a contradiction: "read-only" refers to existing graph data, while capture has always been a write action (as on-app audio dictation already is). The benefit of this line: there is no duplicate edit-and-sync logic to maintain and no second source of truth. Obsidian is the truth; Mockingbird's views are a read of that truth, recomputed from the vault rather than stored separately, so they can never drift out of sync with what Obsidian shows.

### 15.2 Settings / admin area

In the existing Mockingbird settings area, near the current Obsidian Sync settings (**the agent should inspect that existing settings implementation and follow its patterns**), add the controls the user needs to govern the feature. At minimum the agent should account for:

- Which vault / which folders the knowledge graph uses (the `Knowledge Graph/` location and its `Inbox`/`Entries`/`History` structure).
- The controlled vocabularies — the Layer 1 categories and Layer 2 types (§7.2) — so the user can review or adjust them, informed by whatever Phase 0 reveals about whether the defaults fit a general user.
- Processing / queue behavior (e.g. how deferred processing and the inbox-state convention behave).
- The dual-write behavior (graph captures also saved as standard dictations).

Exact fields and layout are for the agent to finalize after Phase 0 clarifies the data model; this lists the intent.

### 15.3 The Knowledge Graph screen (left sidebar, read-only dashboard)

Add a **Knowledge Graph** item to the Mockingbird left sidebar — a first-class destination alongside the existing sections. The screen is a **read-only, high-level status dashboard**: a quick, honest overview of the current state of the data, useful at a glance without opening Obsidian. Candidate insights (final set deferred to the agent post–Phase 0, since Phase 0 determines what the data model can reliably support and what is genuinely useful):

- Counts: number of projects/entries, broken down by category, type, and status.
- Activity: recent entries, and the processing-queue / inbox state (what's queued, what's been processed — tying into the glanceable-state principle in §10).
- Light insights: e.g. open tasks, items with upcoming due dates, anything flagged for review.

All figures are **computed by reading the vault**, not maintained in a separate Mockingbird store, so the dashboard always reflects the files Obsidian also reads.

### 15.4 Launch-into-Obsidian handoff

The screen includes a **launch button** that opens the connected vault in Obsidian (the vault configured in §15.2 settings), handing the user off to the full spatial Monday-style experience — boards, editing, graph — that lives there. This is the explicit bridge from "observe in Mockingbird" to "work in Obsidian."

### 15.5 On-app capture — audio note and text note (same pipeline, different entry points)

The Knowledge Graph screen offers two ways to capture **new** input directly from the app. Both feed the **same knowledge-graph pipeline** as audio dropped into the watched mobile folder — same segmentation, classification, tagging, normalization — never a separate processing path. They differ only in where they enter the pipeline and whether they also write to the dictations history.

**Audio note (primary).** New audio capture from the app's microphone, exactly like an existing Mockingbird dictation or meeting capture. It:
- saves the audio to the **regular dictations history** (existing behavior, for normal reference), **and**
- is processed into knowledge-graph entries (the dual pipeline), **and**
- the read-only dashboard updates to reflect the new entries (by re-reading the vault).

**Text note (convenience addition).** A typed/pasted text entry on the Knowledge Graph screen that runs through the **same graph pipeline**, simply entering after the transcription step since it is already text. Scoping difference: the text note is **knowledge-graph-only** — it creates a graph entry but does **not** write to the dictations or meetings history (no audio ever existed, so it would only clutter those sections). This gives the user a non-voice capture option when typing is easier than talking, without polluting the dictation record.

Because both methods share one pipeline, whatever Phase 0 validates covers on-app audio, dropped-in mobile audio, and on-app text at once. (Note: capturing *new* input — by audio or text — is a write action, and is fully consistent with the read-only principle in §15.1, which concerns not *editing existing* entries or rendering an editable board. Capture has always been a write, as the audio path shows.)

### 15.6 Revisit criterion

Option A is the starting point, not a permanent ceiling. Once the Obsidian-based board/graph experience is in real use, evaluate whether it delivers a sufficient Monday-style experience. If it does not, the previously identified richer options (Mockingbird embedding/mirroring an editable board, or a deeper hybrid) can be reconsidered — but only with evidence from real use, and with the cost of duplicating Obsidian's editing UI weighed explicitly.

---

## Appendix — Sequencing summary

1. **Phase 0** (sandboxed, no production changes): build corpus + answer key → build harness reusing the existing engine by reference → run multi-pass pipeline → score vs. pre-set thresholds → two-run stability check → report with go/no-go + scope recommendation.
2. **v1** (lighter, graph-ready bones): folders (`Inbox`/`Entries`/`History`), three-layer tags, Obsidian Tasks format for tasks, split-by-default, dual-write, glanceable inbox state. Ship pre-built Kanban + query-driven dashboard/board notes (§14) so working, bidirectional views exist on day one. Build the Mockingbird in-app surface as a **control surface only (Option A, §15)**: settings/admin, a read-only dashboard, on-app dictation through the shared pipeline, and a launch-into-Obsidian button — no in-app editing or boards. Set per-surface expectations (§13): capture + manage + view everywhere; spatial board/graph planning on desktop; Mockingbird observes and controls.
3. **v1.5**: entity/concept extraction + cross-linking; backfill the v1 catalogue from retained transcripts.
4. **v2+**: task dependencies and dependency-aware boards/visualization layered over the existing `type`/`status`/tag data.

**Cross-cutting guarantees (all phases):** plain markdown is the only datastore (no vector DB, no embeddings, no proprietary store); Mockingbird emits open formats and depends on no third-party app or its servers; every view edits the one underlying file, so sync is bidirectional by default.
