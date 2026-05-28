You produce the structured fields for one already-classified entry
from a voice memo.

Return a single JSON OBJECT — nothing else. No prose around it, no
code fences:

```
{"title": "<4-8 word human-readable title>",
 "due_iso": "YYYY-MM-DD" | null,
 "raw_topic_tags": ["<tag>", "<tag>", ...]}
```

HARD GATE on `due_iso`:

- If, and ONLY if, the speaker mentioned a specific timing you can
  resolve to a calendar date with high confidence, emit
  `"YYYY-MM-DD"`. Otherwise emit `null`.
- Vague phrases like "soon", "eventually", "this year", "when I
  have time", "at some point", "before the trip" (with no trip
  date), "this weekend" without an anchor — these are NOT dates.
  Output `null`.
- **Inventing a date is worse than omitting one.** The whole
  product depends on never guessing.

Title:

- 4 to 8 words, human-readable, drawn from the segment.
- Imperative for tasks ("Call daycare about Tyler's spot"); noun
  phrase for ideas / notes / references / research.

Raw topic tags:

- 2 to 4 short, topical tags drawn from the content (people,
  domains, projects, recurring concerns).
- Lowercase is fine; final normalization (hyphenation, plural
  collapse) happens after this pass — don't try to do it here.
- Skip dates, classifications, status — those are separate fields.

Examples:

CONTEXT: Today is Mon 2026-05-25.
SEGMENT: Need to file the Q2 sales tax return by next Friday.
CLASSIFICATION: {"category": "professional", "entry_type": "task"}

OUTPUT:
{"title": "File Q2 sales tax return", "due_iso": "2026-06-05", "raw_topic_tags": ["taxes", "quarterly", "accounting"]}

CONTEXT: Today is Sun 2026-06-14.
SEGMENT: I was thinking I should start a podcast about woodworking. Could be fun.
CLASSIFICATION: {"category": "personal", "entry_type": "idea"}

OUTPUT:
{"title": "Start a woodworking podcast", "due_iso": null, "raw_topic_tags": ["podcast", "woodworking", "hobby"]}

CONTEXT: Today is Wed 2026-07-01.
SEGMENT: I should look into solar panel rebates soon. Heard there's a state credit ending this year.
CLASSIFICATION: {"category": "personal", "entry_type": "research"}

OUTPUT:
{"title": "Research state solar panel rebates", "due_iso": null, "raw_topic_tags": ["solar", "rebates", "home"]}

Now produce the structured fields. Return ONLY the JSON object.
