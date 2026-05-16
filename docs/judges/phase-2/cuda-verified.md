# Judge: cuda-verified (Phase 2)

**Target:** `src-tauri/src/stt/whisper.rs`, `Cargo.toml` (workspace), `docs/adr/0011-whisper-rs-cuda-build.md`

**Question:** When the STT pipeline runs on this device, does Whisper actually use the GPU (cuBLAS / CUDA), or did it silently fall back to CPU?

**Rationale:** PLAN line 1362 is unambiguous: *"CUDA path verified on RTX 2060."* Without this verification, the user-facing latency promise (< 1 s for 10 s audio) is unmet. The GPU-first/CPU-fallback design (ADR 0011) means the code can quietly run on CPU forever without anyone noticing — until a user complains about 10× slower transcription. This judge gates the seal tag against exactly that drift.

**Pass criteria — ALL of:**

1. `Cargo.toml` workspace section has `whisper-rs = { version = "0.16", features = ["cuda"] }` (cuda feature ON, NOT commented out).
2. `cargo build --release --bin stt_test` completes without CUDA build errors.
3. Running the binary against any speech WAV produces stderr/stdout lines matching `CUDA|cuBLAS|gpu_device|cudart`:

   ```powershell
   $env:WHISPER_MODEL_PATH = "$env:USERPROFILE\mockingbird_models\whisper-large-v3-turbo-q5_0.bin"
   $out = & target\release\stt_test.exe src-tauri\tests\fixtures\audio\hello.wav 2>&1
   $out | Select-String -Pattern 'CUDA|cuBLAS|gpu_device|cudart'
   ```

   At least one match must appear. The `whisper_init_with_params_no_state: use gpu = 0` line is a **FAIL** signal — that means GPU was requested but Whisper ran CPU anyway.

4. `--json` output's `"gpu_used"` field is `true`:

   ```powershell
   $j = & target\release\stt_test.exe src-tauri\tests\fixtures\audio\hello.wav --json | ConvertFrom-Json
   if (-not $j.gpu_used) { throw "gpu_used=false — CPU fallback ran" }
   ```

**Pass criteria — CURRENT STATE (Wave 5 baseline):**

🚨 **NOT YET MET.** Wave 4 ships with `cuda` feature **OFF** because:

- Chocolatey only publishes `cuda 13.2.1`.
- ggml hard-codes CUDA architectures `52;61;70;75` which are deprecated in CUDA 13.
- MSBuild's `CudaToolkitDir` integration comes up empty for CUDA 13 → C++ compile aborts.

ADR 0011's runtime CPU fallback covers this gracefully (no crash, no telemetry, no surprise), but the judge stays **RED** until CUDA 12.x is installed side-by-side and the feature is flipped back on. See `bd mb-ltq` for the re-enable checklist.

**On failure:**

- **Block the `phase-2-complete` tag.** GPU verification is PLAN-mandated.
- If `use gpu = 0` appears in stderr: GPU init failed at runtime. Check `nvidia-smi` works, driver is present, and `CUDA_PATH` env var resolves.
- If the build itself fails: see ADR 0011 + LESSONS entries `2026-05-16 [phase-2] CUDA 13 + whisper-rs 0.16's bundled ggml = chasm` and follow the CUDA 12.x install path.

**Last run:** _Wave 5 — **RED**. cuda feature off; runtime confirmed `whisper_init_with_params_no_state: use gpu = 0` + `whisper_backend_init_gpu: no GPU found`. CPU fallback works (model loads, compute buffers allocate, transcribe runs). Verification blocked on CUDA 12.x install (bd mb-ltq, P0)._
