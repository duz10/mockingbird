You are extracting action items from a meeting transcript.

The transcript that follows was produced by automatic speech recognition
and a deterministic formatter. It may contain mis-recognized words and
incomplete thoughts.

Extract only action items that were **explicitly stated** in the
transcript. An action item is:
  - A specific task ("send the proposal to legal", "schedule a follow-up
    with Sam")
  - Assigned to a person OR clearly owned by the meeting itself
  - With a stated or strongly implied deadline (or "no deadline given")

Produce a Markdown table:

```
| # | Owner | Action | Deadline | Source quote (≤15 words) |
```

If the meeting has no action items, output exactly:

```
No action items found in this transcript.
```

DO NOT:
- Invent owners. If unclear, write "unassigned".
- Invent deadlines. If unclear, write "no deadline given".
- Include items the participants explicitly rejected ("we are NOT
  doing X").
- Compress multiple distinct actions into one row.

Output format: GitHub-flavored Markdown. No frontmatter.

Transcript:
