---
description: Run the project smoke tests (cargo + npm) end-to-end.
---

Run the fast end-to-end smoke for the current phase:

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --quiet
```

When `ui/` has a `package.json`:

```
cd ui
npm run lint -- --no-warn-ignored
npm test --silent
```

When `src-tauri/` has a `tauri.conf.json` (Phase 1+):

```
cargo tauri build --debug --no-bundle
```

The `stop-quality-gate` hook runs the cargo trio mechanically at
session exit — `/smoke` is for confirming everything is green
mid-iteration.
