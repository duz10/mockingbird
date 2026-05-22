<!--
  PLACEHOLDER for Phase 10 Wave 4.

  Wave 3 lands this file empty (with this comment) so the
  prompt_set_sha hashed at session-summarization time stays
  stable across the Wave-3 → Wave-4 boundary. Once Wave 4 ships
  Layer-2 audio capture + chunked Whisper transcription, the
  abstractor will pick THIS prompt instead of `abstract_block.md`
  whenever a Block has overlapping `activity_transcript_segments`
  rows, and this file will be fleshed with the per-Block + audio
  bundle instructions.

  Wave 3 NEVER reads this file. It is included in the
  `prompt_set_sha` calculation only — the SHA covers the directory
  contents so a future content change here updates the SHA
  automatically (provenance is total; Principle 2).
-->
