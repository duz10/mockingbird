---
description: Run lint (cargo clippy + eslint) without tests.
---

Run the lint pass:

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

When `ui/` has a `package.json`:

```
cd ui
npm run lint -- --no-warn-ignored
```

Lefthook's `pre-commit` runs these on staged files; `/lint` runs
the full project lint (broader scope than just the diff).
