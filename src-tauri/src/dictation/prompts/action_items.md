You are extracting action items from a personal voice memo.

The text that follows was dictated by ONE person — typically the user
talking to themselves, to a future self, or thinking out loud about
work. It is NOT a multi-person meeting. Do not look for assigned
owners; the implicit owner of every action is the speaker ("you" /
"the user") unless they name someone else.

Treat ALL of the following as action items, in order of preference:
  1. Explicit todos ("I need to send the proposal", "let's rename
     this feature", "remember to update the doc").
  2. Decisions the speaker makes ("we should call it Dictations",
     "we are going to use Whisper.cpp"). A decision IS an action: the
     next step is to implement / propagate it.
  3. Things the speaker resolves to do next ("first I'll wire the
     IPC, then I'll add the panel").
  4. Open questions the speaker flags for follow-up ("not sure if we
     should also do X — investigate later").

Output ONLY a bullet list. Each bullet starts with `- ` and is one
action, phrased as an imperative the user can act on. Like this
(but DO NOT include the surrounding fences — render the bullets
raw, with no wrapping at all):

- Rename the History tab to Dictations across the UI.
- Decide whether to keep the old /history URL as a redirect.
- Investigate whether other surfaces need the same rename.

Your response MUST start with the character `-` and MUST NOT start
with three backticks. Do not prefix with "Sure, here are…" or any
other preamble.

If — and only if — the memo contains literally NO actionable content
(e.g. it's a pure observation like "the weather is nice today"),
output exactly:

```
No action items found in this transcript.
```

DO NOT:
  - Wrap the output in ```backticks``` or any other code fence.
  - Add a heading like "## Action items" — just the bullets.
  - Require explicit deadlines. Voice memos rarely have them.
  - Require named owners. The speaker IS the owner.
  - Treat decisions and intents as non-actionable just because they
    aren't phrased as "I will X by Friday".

Voice memo:
