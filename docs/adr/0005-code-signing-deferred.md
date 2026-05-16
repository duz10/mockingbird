# ADR-0005: Code-signing certificate deferred to Phase 7

- **Status:** Deferred (Phase 7)
- **Date:** 2026-05-15
- **Deciders:** Dustin

## Context

PLAN §−1 item 4 lists "code-signing cert" as a pre-flight decision.
Tauri updater requires signing for distribution, but signed Windows
binaries also need a Microsoft-trusted certificate (EV preferred)
which is a procurement task (cost, identity validation, hardware
token logistics).

## Decision

Defer the certificate procurement and acquisition to Phase 7
("Polish"). Phase 0–6 ship locally for dev-loop testing only.
Phase 7 will:

1. Choose a CA (DigiCert vs SSL.com vs others)
2. Procure the cert + hardware token
3. Wire signing into the Tauri release pipeline
4. Update `tauri.conf.json`
5. Write a follow-up ADR (probably 0011 or similar) documenting the
   choice and the renewal calendar.

Phase 0–6 binaries are unsigned; SmartScreen warnings are expected
during dev-loop installs and accepted.

## Consequences

- **Positive:** no procurement blocker for Phases 0–6.
- **Negative:** updater integration tests in Phase 7 may surface
  late issues that would have been cheaper to find earlier.
  Mitigated by Phase 7's polish budget.
- **Neutral:** Tauri updater key pair (signing the *update payload*,
  not the binary itself) is generated in bootstrap and exists.

## Alternatives considered

- **Acquire cert during bootstrap:** stalls coding on procurement.
- **Self-signed cert:** users have to explicitly trust, terrible UX
  for an app the user installs from a non-store URL.

## Cross-references

- PLAN §−1 item 4
- STATUS.md "Section −1 resolution" table
- (future) ADR 0011-or-later: code-signing CA choice
