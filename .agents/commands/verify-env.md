---
description: Verify local dev environment has every tool Mockingbird needs.
---

Run:

```
pwsh scripts/verify-environment.ps1
```

Add `-Strict` to fail the command when Phase-2/4 prereqs are missing:

```
pwsh scripts/verify-environment.ps1 -Strict
```

The script checks rustc/cargo, node/npm, git, python, cargo-tauri,
`bd`, WebView2, plus the deferred-install set: cmake, nvcc, ollama.
Install URLs are surfaced for anything missing.
