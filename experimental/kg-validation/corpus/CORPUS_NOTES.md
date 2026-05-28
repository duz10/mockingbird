# Corpus notes — Phase 0 KG validation (mb-t7w5)

This directory holds the hand-authored (dictation, answer-key) pairs that
constitute the Phase 0 measurement set per ADR 0048. The dictations are
authored by Bernard (planning-agent) and reviewed pair-by-pair by Dustin
before landing here. The answer keys are written AT THE SAME TIME as the
dictations — spec §6.1's independent-ground-truth rule. No model under
test ever generates its own answer key.

## Single corpus-wide capture anchor

All dictations are treated as captured at:

    2026-06-14T08:00:00Z   (Sunday morning)

The harness threads this into pipeline prompts as the "current time"
context so relative-date answer keys are deterministic.

Reference calendar from the anchor:
- Mon = 2026-06-15
- Tue = 2026-06-16
- Wed = 2026-06-17
- Thu = 2026-06-18
- Fri = 2026-06-19
- Sat = 2026-06-20
- next Mon = 2026-06-22
- next Tue = 2026-06-23
- next Fri = 2026-06-26
- "next month" / "in July" = 2026-07-01..31
- "August" = 2026-08-01..31

## Persona index (matches spec §6.2 order)

| ID | Persona | Voice notes |
|---|---|---|
| 01 | Working-class hourly earner | Shift schedules, car repairs, utility bills, kids' school logistics. Mid-thought disfluencies ("uh," "okay so"), informal register. |
| 02 | Lower-middle-class tradesperson / service worker | Job leads, tool/supply purchases, client follow-ups interleaved with family. Practical, concrete. |
| 03 | Salaried middle-class professional | Work projects bleeding into evenings, home improvement, vacation planning. Slightly more structured speech. |
| 04 | Aspiring-middle-class side-hustler | Personal/professional boundaries genuinely blurred. The messiest, highest-value persona for ambiguous-category cases. |
| 05 | Caregiver / parent running a household | Appointments, school forms, groceries, stray personal goals. Often mid-task while dictating. |
| 06 | Recent grad / early-career renter | Job apps, budgeting, social plans. Self-aware, sometimes hedged ("I should probably," "I keep meaning to"). |

## Filename convention

- Dictations: `dictations/persona-NN-case-MM.md` — plain text, no frontmatter.
- Answer keys: `answer-keys/persona-NN-case-MM.json` — serialized `AnswerKey` per `src/schema.rs`.

`NN` is the persona ID (01-06). `MM` is the sequence within that persona (01, 02, ...).

## Distribution target (from spec §6.2)

| Difficulty | Target |
|---|---|
| Clean single-item | ~8 |
| Multi-item rambler | ~10 |
| Ambiguous category | ~6 |
| No date mentioned | ~4 |
| Near-empty / junk | ~2 |
| Total | ~30 |

Each pair's difficulty type is documented in the corresponding answer-key
file's leading comment-style notes (NOT in the AnswerKey struct itself,
which stays strictly typed).

## Batch ledger

| Batch | Pairs | Status |
|---|---|---|
| 1 | 5 pairs: persona-{01,02,03,05,06}-case-01 | Landed (Wave 1 Batch 1). |
| 2 | 10 pairs: persona-01-case-{02,03}, persona-02-case-02, persona-03-case-{02,03}, persona-04-case-{01,02,03}, persona-05-case-02, persona-06-case-02 | Landed (Wave 1 Batch 2). Adds side-hustler (persona 04) debut + multi-item ramblers + ambiguous-category cases. |
| 3 | 15 pairs: persona-01-case-{04,05}, persona-02-case-{03,04}, persona-03-case-{04,05,06}, persona-04-case-{04,05}, persona-05-case-{03,04,05}, persona-06-case-{03,04,05} | Landed (Wave 1 Batch 3). Adds 2 junk cases, 4 no-date hard-gate, 3 `objective` category, 1 `reference` type, and the 5-item peak-hard segmentation case (persona-05-case-03). |
| Addendum | 2 pairs: persona-01-case-06 (personal note — gate code FYI), persona-03-case-07 (professional note — witnessed launch-slip announcement) | Landed (Wave 1 addendum). Closes the `EntryType::Note` taxonomy gap inline rather than deferring to v2. Tests the `note` vs. `task` boundary (no action implied, just a fact to file) and the `note` vs. `reference` boundary (firsthand witnessed fact vs. info-from-elsewhere). `corpus_exercises_full_taxonomy` now asserts all 5 `EntryType` variants present. |

**Batch totals:** 32 total = 5 + 10 + 15 + 2 addendum.

## Final distribution (32 pairs)

| Persona | Cases |
|---|---|
| 01 (working-class)        | 6 |
| 02 (tradesperson)         | 4 |
| 03 (salaried professional)| 7 |
| 04 (side-hustler)         | 5 |
| 05 (caregiver)            | 5 |
| 06 (recent grad)          | 5 |

Difficulty rough breakdown: 13 clean single-item · 13 multi-item rambler
(incl. 1 five-item peak-hard) · 2 junk · 4 dedicated no-date hard-gate ·
8+ ambiguous-category (incl. 3 `objective` tests) · 1 `reference` type ·
2 `note` type (Wave 1 addendum).

Taxonomy coverage: `Category` Personal / Professional / Objective all
exercised; `EntryType` Task / Idea / Research / Reference / **Note** all
exercised. The Wave 1 addendum closed the previous Note gap inline (see
`mb-901u`), so the Phase 0 fixture set now spans the full taxonomy. The
`corpus_exercises_full_taxonomy` test in `src/schema.rs` mechanically
enforces this from 20+ keys onward.
