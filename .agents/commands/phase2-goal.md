---
description: Execute one iteration of Phase 2 (Audio capture & STT). See docs/phases/phase2.md.
---

Phase 2: Audio capture & STT. Read `docs/phases/phase2.md`
(planning-agent writes it after Phase 1 closes). Standard
required-reading + iteration-mandate + definition-of-done apply
from `.code_puppy/AGENTS.md`.

**Prerequisite check**: Phase 2 needs cmake + nvcc + ollama installed
(see STATUS.md "Blocked-on"). Confirm with `pwsh scripts/verify-environment.ps1 -Strict`
before invoking `/goal` on Phase 2.
