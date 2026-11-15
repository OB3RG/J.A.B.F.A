use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::error::{JabfaError, Result};
use crate::index::IndexConfig;
use crate::navigation::get_section;
use crate::parser::parse_markdown;
use crate::writes::{
    WriteResponse, commit_content, patch_section, validate_single_line, validate_write_path,
};

/// A schema-valid llm-brain learning and its registry/log updates.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BrainLearningRequest {
    pub path: PathBuf,
    pub title: String,
    pub finding: String,
    pub context: String,
    pub consequences: String,
    pub origin: String,
    pub created: String,
    pub tags: Vec<String>,
}

/// Create a learning, then patch the brain index and append its log entry.
pub fn create_brain_learning(
    config: &IndexConfig,
    request: &BrainLearningRequest,
) -> Result<WriteResponse> {
    validate_request(request)?;
    let content = render_learning(request)?;
    parse_markdown(&request.path, &content)?;
    let index_citation = section_citation(config, "wiki/index.md", Some("Learnings"))?;
    let log_citation = section_citation(config, "wiki/log.md", None)?;
    let stem = request
        .path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| {
            JabfaError::InvalidWriteInput("learning path needs a file stem".to_owned())
        })?;
    if index_citation.text.contains(&format!("[[{stem}]]")) {
        return Err(JabfaError::InvalidWriteInput(format!(
            "brain index already contains [[{stem}]]"
        )));
    }

    let mut response = commit_content(config, &request.path, &content, true)?;
    let index = get_section(
        config,
        Path::new("wiki/index.md"),
        index_citation.start_line,
        index_citation.end_line,
        1_048_576,
    )?;
    let index_replacement = format!(
        "{}\n- [[{}]] — {} ({})\n\n",
        index.content.trim_end(),
        stem,
        request.finding.trim(),
        request.created
    );
    response.generation = patch_section(
        config,
        &crate::PatchSectionRequest {
            path: PathBuf::from("wiki/index.md"),
            start_line: index_citation.start_line,
            end_line: index_citation.end_line,
            expected_hash: index_citation.content_hash,
            replacement: index_replacement,
        },
    )?
    .generation;

    let log = get_section(
        config,
        Path::new("wiki/log.md"),
        log_citation.start_line,
        log_citation.end_line,
        1_048_576,
    )?;
    let log_replacement = format!(
        "{}\n\n## [{}] learn | {}\n\n- {}\n- Updated `{}`, `wiki/index.md`, and `wiki/log.md`.\n",
        log.content.trim_end(),
        request.created,
        request.title,
        request.finding.trim(),
        request.path.to_string_lossy()
    );
    response.generation = patch_section(
        config,
        &crate::PatchSectionRequest {
            path: PathBuf::from("wiki/log.md"),
            start_line: log_citation.start_line,
            end_line: log_citation.end_line,
            expected_hash: log_citation.content_hash,
            replacement: log_replacement,
        },
    )?
    .generation;
    Ok(response)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SectionCitation {
    start_line: usize,
    end_line: usize,
    content_hash: String,
    text: String,
}

fn section_citation(
    config: &IndexConfig,
    path: &str,
    heading: Option<&str>,
) -> Result<SectionCitation> {
    if !config.database_path.is_file() {
        return Err(JabfaError::DatabaseNotFound(config.database_path.clone()));
    }
    let connection = Connection::open_with_flags(
        &config.database_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let (sql, heading_value) = match heading {
        Some(heading) => (
            "SELECT sections.start_line, sections.end_line, sections.content_hash, sections.text
             FROM sections JOIN documents ON documents.id = sections.document_id
             WHERE documents.path = ?1 AND sections.heading = ?2",
            heading.to_owned(),
        ),
        None => (
            "SELECT sections.start_line, sections.end_line, sections.content_hash, sections.text
             FROM sections JOIN documents ON documents.id = sections.document_id
             WHERE documents.path = ?1 AND sections.heading = ?2
             ORDER BY sections.end_line DESC LIMIT 1",
            String::new(),
        ),
    };
    let citation = if heading.is_some() {
        connection.query_row(sql, params![path, heading_value], section_citation_row)
    } else {
        connection.query_row(
            "SELECT sections.start_line, sections.end_line, sections.content_hash, sections.text
             FROM sections JOIN documents ON documents.id = sections.document_id
             WHERE documents.path = ?1
             ORDER BY sections.end_line DESC LIMIT 1",
            params![path],
            section_citation_row,
        )
    }
    .optional()?;
    citation.ok_or_else(|| JabfaError::DocumentNotFound(PathBuf::from(path)))
}

fn section_citation_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SectionCitation> {
    Ok(SectionCitation {
        start_line: row.get(0)?,
        end_line: row.get(1)?,
        content_hash: row.get(2)?,
        text: row.get(3)?,
    })
}

fn validate_request(request: &BrainLearningRequest) -> Result<()> {
    validate_write_path(&request.path)?;
    if !request.path.starts_with("wiki/learnings/") {
        return Err(JabfaError::InvalidWriteInput(
            "brain learning path must be under wiki/learnings/".to_owned(),
        ));
    }
    validate_single_line(&request.title, "title")?;
    validate_single_line(&request.finding, "finding")?;
    validate_single_line(&request.context, "context")?;
    validate_single_line(&request.consequences, "consequences")?;
    validate_single_line(&request.origin, "origin")?;
    if request.title.trim().is_empty()
        || request.finding.trim().is_empty()
        || request.context.trim().is_empty()
        || request.consequences.trim().is_empty()
        || request.origin.trim().is_empty()
    {
        return Err(JabfaError::InvalidWriteInput(
            "title, finding, context, consequences, and origin must be nonempty".to_owned(),
        ));
    }
    if !is_iso_date(&request.created) {
        return Err(JabfaError::InvalidWriteInput(
            "created must use YYYY-MM-DD".to_owned(),
        ));
    }
    Ok(())
}

fn render_learning(request: &BrainLearningRequest) -> Result<String> {
    let mut content = format!(
        "---\ntype: learning\ncreated: {}\nupdated: {}\norigin: {}\ntags: [",
        request.created,
        request.created,
        serde_json::to_string(&request.origin)?
    );
    let mut tags = request.tags.clone();
    tags.sort();
    tags.dedup();
    for (index, tag) in tags.iter().enumerate() {
        if index > 0 {
            content.push_str(", ");
        }
        content.push_str(&serde_json::to_string(tag)?);
    }
    content.push_str("]\n---\n# ");
    content.push_str(request.title.trim());
    content.push_str("\n\n**The finding.** ");
    content.push_str(request.finding.trim());
    content.push_str("\n\n**Context.** ");
    content.push_str(request.context.trim());
    content.push_str("\n\n**Consequences.** ");
    content.push_str(request.consequences.trim());
    content.push('\n');
    Ok(content)
}

fn is_iso_date(value: &str) -> bool {
    value.len() == 10
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 4 | 7) {
                byte == b'-'
            } else {
                byte.is_ascii_digit()
            }
        })
}
