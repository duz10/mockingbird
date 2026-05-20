//! Markdown export for a persisted meeting.
//!
//! Serializes a meeting + its formatted transcripts into a single
//! markdown document with YAML frontmatter (title, started_at,
//! duration, source) and a speaker-tagged body. Optional trailing
//! "## LLM pass output" section if the caller supplies an in-memory
//! LLM-pass result (see [`super::llm_pass`] — note that LLM output is
//! NEVER persisted to the DB).
//!
//! Wave 1 scaffold — types + `todo!()`. Wave 4 ships the impl.

use crate::error::AppResult;

/// What to embed in the export.
#[derive(Debug, Clone)]
pub struct ExportRequest<'a> {
    pub meeting_uuid: &'a str,
    /// If `Some`, append the LLM pass output as a trailing section.
    /// Note: the LLM-pass output is held in [`super::runtime`]'s
    /// in-memory `HashMap<Uuid, String>` keyed cache, NOT in the DB.
    pub llm_pass_text: Option<&'a str>,
}

/// Render to a UTF-8 markdown string.
///
/// Wave 1: `todo!()` — Wave 4 ships the implementation against the
/// `mc-two-channel-merged` judge.
pub fn render_markdown(_req: &ExportRequest<'_>) -> AppResult<String> {
    todo!("Wave 4: implement markdown export with frontmatter + speaker-tagged body")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_request_constructs() {
        let req = ExportRequest {
            meeting_uuid: "uuid",
            llm_pass_text: None,
        };
        assert_eq!(req.meeting_uuid, "uuid");
        assert!(req.llm_pass_text.is_none());
    }
}
