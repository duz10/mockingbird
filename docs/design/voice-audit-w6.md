# W6 Voice & Tone Audit

Reviewed every user-facing string in `ui/src/i18n/en.json` against the four
voice principles from Design Language v1 §12.

## Principles (recap)

1. **Say what it does, not how it makes you feel.** No SaaS marketing copy.
2. **Privacy as fact, not feature.** State what's true; don't perform.
3. **Errors don't apologize.** No "Oops!" or "Something went wrong" — state
   the problem and the next action.
4. **Numbers in mono.** Typography concern, handled by `--font-mono` already.

## Violations found and fixed

| Key                | Before                       | After                                                        | Principle |
| ------------------ | ---------------------------- | ------------------------------------------------------------ | --------- |
| `common.error`     | "Something went wrong"       | "Couldn't complete that action — check logs for details."    | 3         |

That was the only string in the entire i18n file that broke a principle
hard enough to warrant a change. Mockingbird's copy was already mostly
on-voice because we wrote it that way from the start; the audit confirms
it.

## Notes for the future

- `recording.idle` / `recording.recording` / `recording.processing` /
  `recording.ok` / `recording.error` look like legacy keys superseded by
  the `recording.state.*` family. A future cleanup wave can grep call sites
  and delete the unused ones — out of scope for the design cutover.
- The Settings ModelsPanel `window.prompt("Paste your Claude API key:")`
  fallback is a Phase-0 stub; the real flow is a modal in Phase 6 polish
  and that's where it gets a real microcopy pass.
- `window.alert(...)` strings in Settings are dev-stub placeholders for
  Phase-7 work and aren't shipped to users.

— Recorded as part of DLW6 (mb-ubf).
