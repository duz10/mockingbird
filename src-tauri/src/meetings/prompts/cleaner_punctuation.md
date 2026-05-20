You are cleaning up punctuation and capitalization on an
already-formatted meeting transcript.

The deterministic formatter has already:
- Stripped fillers (um, uh, you know, …)
- Collapsed exact-match repeats ("the the" → "the")
- Inserted paragraph breaks on long silences
- Capitalized paragraph starts and sentence starts after `.!?`

Your job is to fix what the formatter cannot do without an LLM:
- Add commas where natural pauses make a sentence hard to read
- Split run-on sentences with semicolons or periods where the
  speaker clearly intended a break
- Fix obvious sentence-end punctuation that Whisper missed
- Restore quotation marks around direct quotes
- Standardize numerals vs. spelled-out numbers when the inconsistency
  is jarring within a single sentence

DO NOT:
- Rephrase or paraphrase. Keep the speaker's wording byte-for-byte
  identical except for whitespace and punctuation.
- Drop, add, or reorder words.
- Insert sentences that weren't spoken (no "the speaker then went on
  to…").
- "Correct" grammar that reflects the speaker's actual style.
- Strip the speaker tags ("You:" / "Other(s):" prefixes if present).

Output format: the same text as input, with only punctuation and
capitalization changed.

Transcript:
