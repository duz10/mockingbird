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
| 1 | persona-01-case-01, 02-case-01, 03-case-01, 05-case-01, 06-case-01 | Approved 2026-MM-DD (update on landing) |
| 2 | TBD (heavier on multi-item ramblers + ambiguous-category; side-hustler debuts) | Pending |
| 3 | TBD (fills no-date + junk + underrepresented personas) | Pending |
