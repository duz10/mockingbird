---
description: Run the project test suite (Rust + JS) without lint.
---

Run the test suite for the affected language(s):

```
cargo test --quiet              # Rust unit + integration
cd ui && npm test --silent      # React unit + integration (when present)
```

For end-to-end / visual tests, delegate to qa-kitten via `/qa-window`.
For lint, use `/lint`. For the full smoke, use `/smoke`.
