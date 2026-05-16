---
description: Merge .code_puppy/judges-template.json into ~/.code_puppy/judges.json (idempotent).
---

Run:

```
pwsh scripts/seed-judges.ps1
```

Re-run safely — existing judges with the same `id` are skipped.
Use `-Force` to overwrite existing judges with template versions:

```
pwsh scripts/seed-judges.ps1 -Force
```

The script preserves any local-only judges not in the project template.
Run after pulling new commits that may have touched
`.code_puppy/judges-template.json`.
