#!/usr/bin/env python3
"""aggregate_fixture.py — capture the Wave 0.5.4 seed-42 KG parity fixture.

Aggregates the sandbox's iter-1-7b-fix (4-pass pipeline) + run-7b-entities-seed42
(entity probe) artifacts into two deterministic JSON files consumed by Chunk 3's
production `kg_parity` probe:

    docs/knowledge-graph/parity/wave-0.5.4-seed-42.json
        — per-dictation pipeline_result + entities aggregate (sorted by dictation_id).

    docs/knowledge-graph/parity/wave-0.5.4-seed-42-canned-responses.json
        — per-(dictation, pass, segment_idx) JSON strings to feed MockOllama.

Re-running this against the same source dirs is a content-stable no-op. Uses
Python's stdlib json (sort_keys=True everywhere) so diffs are byte-stable
across re-runs and across machines.

Sources:
    experimental/kg-validation/runs/iter-1-7b-fix/structured/<id>.json
    experimental/kg-validation/runs/iter-1-7b-fix/raw/<id>/{segment,classify-N,extract-N}.json
    experimental/kg-validation/runs/run-7b-entities-seed42/entities/<id>.json
    experimental/kg-validation/corpus/dictations/<id>.md

Why Python and not PowerShell: PS 5.1's ConvertTo-Json wraps Object[] values
nested in ordered hashtables as {value, Count} instead of bare arrays, which
breaks bit-identical parity. Python json.dumps preserves list shape verbatim.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO_ROOT = HERE.parents[2]
SANDBOX = REPO_ROOT / "experimental" / "kg-validation"
PIPELINE_RUN = SANDBOX / "runs" / "iter-1-7b-fix"
ENTITY_RUN = SANDBOX / "runs" / "run-7b-entities-seed42"
CORPUS_DICTS = SANDBOX / "corpus" / "dictations"

CAPTURED_ISO = "2026-06-14T08:00:00Z"
MODEL = "qwen2.5:7b-instruct-q4_K_M"
PROFILE = "mid-confident"
SEED = 42


def _load_json(path: Path):
    with path.open("r", encoding="utf-8") as f:
        return json.load(f)


def _load_text(path: Path) -> str:
    return path.read_text(encoding="utf-8").strip()


def _numeric_suffix(name: str, prefix: str) -> int:
    m = re.match(rf"{re.escape(prefix)}-(\d+)\.json$", name)
    if not m:
        raise ValueError(f"unexpected per-pass artifact name: {name}")
    return int(m.group(1))


def _compact(obj) -> str:
    # Compact JSON, sorted keys — the form a deterministic LLM mock should
    # return AND the form most likely to round-trip through the production
    # passes::{segment,classify,extract,extract_entities} parsers.
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def _canned_response_for(artifact: dict) -> str | None:
    """Pick the canned string to feed MockOllama for a given per-pass
    artifact.

    Preference order:
      1. `parsed` (compact JSON re-serialization) — the happy path.
      2. `raw_model_output` verbatim — when the sandbox model returned
         something the parser rejected (e.g. an invalid enum variant).
         Replaying the raw output is the only way production can
         reproduce the sandbox's parse error byte-for-byte; without it,
         production's dispatcher returns an `OllamaError::Mock` instead
         of `PassError::Parse`, and `pipeline_result.per_pass_errors`
         diverges from the fixture.
      3. `None` — only when both fields are absent / empty. The probe
         will surface this as an unmatched canned response.

    Added in the Phase 1A Wave 3 parity-gate fix (mb-cskk follow-up):
    the original capture script ignored `raw_model_output`, which
    silently flattened the corpus's only classify parse-failure case
    (persona-06-case-05) into a no-op dispatcher error.
    """
    parsed = artifact.get("parsed")
    if parsed is not None:
        return _compact(parsed)
    raw = artifact.get("raw_model_output")
    if isinstance(raw, str) and raw:
        return raw
    return None


def _collect_per_pass_errors(
    segment_artifact: dict | None,
    classify_artifacts: list,
    extract_artifacts: list,
) -> list:
    """Reconstruct `pipeline_result.per_pass_errors` from sandbox
    artifacts.

    Production's `run_pipeline` pushes `(format!("{stage}[{idx}]"),
    PassError)` tuples — the probe re-serializes these as `[tag,
    error_string]` JSON arrays via `pipeline_result_to_value`. To match
    that shape from the sandbox side, walk each per-pass artifact and
    pick up any non-null `error` field. The sandbox's classify /
    extract / extract_entities errors are already formatted with the
    same `PassError::Parse` template production uses
    ("JSON parse failed in {pass} pass: {error}\nRaw output:\n{raw}"),
    so the strings round-trip byte-identical.

    Added with `_canned_response_for` (Phase 1A Wave 3 parity fix).
    """
    errors: list = []
    if segment_artifact and segment_artifact.get("error"):
        errors.append([f"segment[{0}]", segment_artifact["error"]])
    for idx, art in enumerate(classify_artifacts):
        err = art.get("error") if art else None
        if err:
            errors.append([f"classify[{idx}]", err])
    for idx, art in enumerate(extract_artifacts):
        err = art.get("error") if art else None
        if err:
            errors.append([f"extract[{idx}]", err])
    return errors


def main() -> int:
    for required in (PIPELINE_RUN, ENTITY_RUN, CORPUS_DICTS):
        if not required.exists():
            print(f"missing required source dir: {required}", file=sys.stderr)
            return 1

    structured_dir = PIPELINE_RUN / "structured"
    raw_dir_root = PIPELINE_RUN / "raw"
    entity_dir = ENTITY_RUN / "entities"

    dict_ids = sorted(
        p.stem for p in structured_dir.glob("*.json")
    )
    print(f"Aggregating {len(dict_ids)} dictations from {PIPELINE_RUN}")

    dictations = []
    canned_per_dict: dict[str, dict] = {}

    for dict_id in dict_ids:
        entries = _load_json(structured_dir / f"{dict_id}.json")
        if not isinstance(entries, list):
            raise TypeError(f"structured/{dict_id}.json is not a list")

        dict_md = CORPUS_DICTS / f"{dict_id}.md"
        dict_text = _load_text(dict_md) if dict_md.exists() else ""

        # ── Per-pass artifacts ───────────────────────────────────────
        raw_dir = raw_dir_root / dict_id
        segment_artifact = None
        classify_artifacts: list = []
        extract_artifacts: list = []
        if raw_dir.is_dir():
            seg = raw_dir / "segment.json"
            if seg.exists():
                segment_artifact = _load_json(seg)

            classify_files = sorted(
                raw_dir.glob("classify-*.json"),
                key=lambda p: _numeric_suffix(p.name, "classify"),
            )
            classify_artifacts = [_load_json(p) for p in classify_files]

            extract_files = sorted(
                raw_dir.glob("extract-*.json"),
                key=lambda p: _numeric_suffix(p.name, "extract"),
            )
            extract_artifacts = [_load_json(p) for p in extract_files]

        # ── Entity-pass output (dictation-aggregate) ─────────────────
        entity_path = entity_dir / f"{dict_id}.json"
        entity_artifact = _load_json(entity_path) if entity_path.exists() else None

        # ── Fixture row ──────────────────────────────────────────────
        row = {
            "dictation_id": dict_id,
            "dictation_text": dict_text,
            "captured_iso": CAPTURED_ISO,
            "pipeline_result": {
                # The PipelineResult.entries shape kg::run_pipeline must
                # reproduce bit-identically (open-vocab run; no closed-vocab
                # validator was wired in iter-1-7b-fix).
                "entries": entries,
                # per_pass_errors reconstructed from sandbox artifacts'
                # `error` fields — see _collect_per_pass_errors. Was
                # hardcoded [] in the original capture; that lie matched
                # reality for 31/32 fixtures but broke persona-06-case-05
                # (the corpus's only classify parse-failure case).
                "per_pass_errors": _collect_per_pass_errors(
                    segment_artifact, classify_artifacts, extract_artifacts
                ),
                "new_tag_requests": [],
            },
            "entities": (
                {
                    "entities": entity_artifact.get("entities", []),
                    "segment_count": entity_artifact.get("segment_count", 0),
                    "segment_failures": entity_artifact.get("segment_failures", []),
                }
                if entity_artifact
                else None
            ),
        }
        dictations.append(row)

        # ── Canned MockOllama responses ──────────────────────────────
        # segment: array of segment strings.
        segment_response = (
            _compact(segment_artifact["parsed_segments"])
            if segment_artifact and segment_artifact.get("parsed_segments") is not None
            else None
        )
        # Use _canned_response_for so parse-failure cases re-feed their
        # raw model output to production (lets PassError::Parse fire on
        # the production side with the same message the sandbox logged).
        classify_responses = [_canned_response_for(c) for c in classify_artifacts]
        extract_responses = [_canned_response_for(x) for x in extract_artifacts]
        # extract_entities: per-dictation aggregate (see README §3).
        entity_response = (
            _compact({"entities": entity_artifact["entities"]})
            if entity_artifact is not None
            else None
        )

        canned_per_dict[dict_id] = {
            "segment": segment_response,
            "classify": classify_responses,
            "extract": extract_responses,
            "extract_entities": entity_response,
        }

    fixture = {
        "fixture_version": "1.0",
        "source_run_pipeline": "experimental/kg-validation/runs/iter-1-7b-fix/",
        "source_run_entities": "experimental/kg-validation/runs/run-7b-entities-seed42/",
        "model": MODEL,
        "profile": PROFILE,
        "seed": SEED,
        "captured_iso": CAPTURED_ISO,
        "dictation_count": len(dictations),
        "schema_revision": "phase-0.5-wave-4",
        "pipeline_passes": ["segment", "classify", "extract", "extract_entities", "normalize"],
        "notes": (
            "Wave 0.5.4 sealed scorecard fixture; consumed by Chunk 3's "
            "src-tauri/eval/kg_parity probe to gate the production graduation."
        ),
        "dictations": dictations,
    }

    canned = {
        "fixture_version": "1.0",
        "source_fixture": "wave-0.5.4-seed-42.json",
        "matcher_strategy": "prompt-substring",
        "notes": (
            "Per-pass canned responses. classify/extract are per-segment ordered "
            "arrays (index = segment_idx in the production pipeline). "
            "extract_entities is the per-dictation aggregate from the Wave 0.5.4 "
            "probe; per-segment provenance was not preserved by the sandbox "
            "harness. Chunk 3's MockOllama loader picks either (a) feed once "
            "per dictation if the production extract_entities pass is wired "
            "per-dictation, or (b) re-run the sandbox entity pass with per-segment "
            "artifact capture if the production pass is per-segment. See README."
        ),
        "per_dictation": canned_per_dict,
    }

    out1 = HERE / "wave-0.5.4-seed-42.json"
    out2 = HERE / "wave-0.5.4-seed-42-canned-responses.json"

    # indent=2, sort_keys=False — top-level key order is intentional;
    # within each dictation row we let insertion order win for readability.
    # Lists are preserved verbatim (no PowerShell wrapper bug here).
    out1.write_text(json.dumps(fixture, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    out2.write_text(json.dumps(canned, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")

    print(f"Wrote {out1} ({out1.stat().st_size} bytes)")
    print(f"Wrote {out2} ({out2.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
