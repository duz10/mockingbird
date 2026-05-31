---
id: "01HMSHAPE00abcdef1234567890"
schema_version: 1
capture_kind: "kg-note"
captured_at: "2026-06-15T14:32:01Z"
title: "Decided to standardize on Tailwind v4 across all Mockingbird UI surfaces"
category: "professional"
type: "decision"
tags:
  - "tailwind"
  - "ui-architecture"
  - "phase-1e"
entities:
  - "[[Entities/mockingbird|mockingbird]]"
  - "[[Entities/tailwind|tailwind]]"
source_session_uuid: "550e8400-e29b-41d4-a716-446655440000"
---

Standardizing on Tailwind v4 across every Mockingbird UI surface. Rationale: the v3-vs-v4 split was costing two separate token files and an ESLint-config fork, with no functional payoff. v4's design-token CSS variables compose cleanly with the existing tokens.css. Migration cost: small (lint-only churn on a handful of files); downstream simplification: large.
