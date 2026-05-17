# ADR 0021 — Cleanup provider trait is sync (not `async_trait`)

**Status:** Accepted — 2026-05-18
**Phase:** 4
**Supersedes:** PLAN §8 `async_trait CleanupProvider` signature

## Context

PLAN §8 originally specified:

```rust
#[async_trait]
pub trait CleanupProvider: Send + Sync {
    async fn cleanup(&self, prompt: &str, raw_transcript: &str, ...) -> Result<...>;
}
```

The PLAN was authored before the dictation orchestrator solidified. As of Wave 4, the orchestrator (`src-tauri/src/dictation.rs`) and everything it touches is **synchronous**:

- `audio.drain` / `vad.process_frame` / `stt.transcribe` / `cleaner.clean` / `injector.inject` — all `&mut self` sync calls.
- The dictation thread is a plain `std::thread::spawn` — no tokio runtime in scope.
- `tokio` is in deps but only with the `rt` + `macros` + `sync` features (single-threaded scheduler for Tauri commands). No `rt-multi-thread`.

Adding async to `CleanupProvider` would require either:

1. Starting a `tokio::runtime::Runtime` inside the dictation thread on every call — overhead + complexity for one HTTP request per dictation.
2. Refactoring the entire orchestrator to async — substantial churn touching every trait and the integration tests built in Wave 5.

## Decision

`CleanupProvider` is **synchronous**:

```rust
pub trait CleanupProvider: Send + Sync {
    fn cleanup(&self, request: CleanupRequest<'_>) -> AppResult<CleanupResult>;
    fn provider_name(&self) -> &'static str;
    fn supports_model(&self, model_id: &str) -> bool;
}
```

HTTP is performed via [`ureq`](https://crates.io/crates/ureq) — a small sync HTTP client with `rustls-tls` support. ~25 transitive deps vs ~150 for `reqwest`.

## Consequences

**Positive:**
- Zero architectural change to the orchestrator or to the Wave 5 integration tests. A stub `CleanupProvider` slots in next to the existing stub `Cleaner`.
- One HTTP request per dictation is fundamentally a *blocking workload from the user's perspective* (we cannot inject text until cleanup returns). No async benefit.
- `ureq` has a smaller dep tree, simpler error surface, and works fine inside a non-tokio thread.

**Negative / mitigations:**
- **Streaming responses** require a custom reader loop instead of `async fn next`. Acceptable — we log streamed tokens for debug but only inject after the response completes. The Ollama provider exposes `cleanup_streaming(callback: impl FnMut(&str))` for the case where Phase 5's recording window wants to show partial output mid-flight.
- **Multiple concurrent dictations** are not supported anyway (single dictation thread; ADR 0015 + state machine §6.1). So the lost concurrency from sync isn't a real loss.
- **Tests** must avoid `#[tokio::test]` — already the case.

## Implementation notes

- All providers (`OllamaProvider`, `ClaudeProvider`) take their HTTP client at construction time so tests can inject a `Box<dyn HttpClient>` mock instead of hitting a real socket.
- Errors lift into `AppError::Cleanup(String)` — a new variant added alongside this ADR.
- A `StubCleanupProvider` (test-only, returns a deterministic transformation) is the analog of `PassthroughCleaner` from Wave 4 and lets Phase 4's orchestrator integration test prove the LLM is actually in the loop without needing a live Ollama.

## Reversibility

If Phase 5+ surfaces a real need for async (e.g. concurrent multi-mode hot-swapping), the migration is mechanical:

1. Add `async_trait` to deps.
2. Wrap every sync trait method body in `tokio::task::block_in_place` initially.
3. Refactor providers one at a time.

This is straightforward — sync is the conservative starting point, not a dead-end.
