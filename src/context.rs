use std::collections::HashSet;

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;

use crate::OUTPUT_SCHEMA_VERSION;
use crate::error::{JabfaError, Result};
use crate::index::IndexConfig;
use crate::query::{SearchResult, search_sections};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextResponse {
    pub output_schema_version: u32,
    pub generation: i64,
    pub context: String,
    pub estimated_tokens: usize,
    pub included: usize,
    pub omitted: usize,
    pub deduplicated: usize,
    pub truncated: usize,
}

pub fn build_context(
    config: &IndexConfig,
    query: &str,
    max_tokens: usize,
) -> Result<ContextResponse> {
    let search = search_sections(config, query, 100)?;
    if !config.database_path.is_file() {
        return Err(JabfaError::DatabaseNotFound(config.database_path.clone()));
    }
    let connection = Connection::open_with_flags(
        &config.database_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;

    let mut response = ContextResponse {
        output_schema_version: OUTPUT_SCHEMA_VERSION,
        generation: search.generation,
        context: String::new(),
        estimated_tokens: 0,
        included: 0,
        omitted: 0,
        deduplicated: 0,
        truncated: 0,
    };
    let mut seen_text = HashSet::new();

    for result in search.results {
        let text: String = connection.query_row(
            "SELECT text FROM sections WHERE id = ?1 AND content_hash = ?2",
            rusqlite::params![result.section_id, result.content_hash],
            |row| row.get(0),
        )?;
        let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
        let text_hash = blake3::hash(normalized.as_bytes()).to_hex().to_string();
        if !seen_text.insert(text_hash) {
            response.deduplicated += 1;
            continue;
        }

        let citation = citation(&result);
        let block = format!("{citation}{text}\n");
        let candidate = append_block(&response.context, &block);
        if estimate_tokens(&candidate) <= max_tokens {
            response.context = candidate;
            response.included += 1;
            continue;
        }

        if response.included == 0 {
            let suffix = "…\n";
            let available = max_tokens
                .saturating_mul(4)
                .saturating_sub(citation.len() + suffix.len());
            if available > 0 {
                let shortened = truncate_utf8(&text, available);
                response.context = format!("{citation}{shortened}{suffix}");
                response.included += 1;
                response.truncated += 1;
                continue;
            }
        }

        response.omitted += 1;
    }

    response.estimated_tokens = estimate_tokens(&response.context);
    Ok(response)
}

fn citation(result: &SearchResult) -> String {
    let heading = if result.heading_path.is_empty() {
        "Preamble".to_owned()
    } else {
        result.heading_path.join(" > ")
    };
    format!(
        "## Evidence\nPath: `{}`\nHeading: {}\nLines: {}-{}\nHash: {}\n\n",
        result.path.display(),
        heading,
        result.start_line,
        result.end_line,
        result.content_hash
    )
}

fn append_block(context: &str, block: &str) -> String {
    if context.is_empty() {
        block.to_owned()
    } else {
        format!("{context}\n{block}")
    }
}

fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

fn truncate_utf8(text: &str, available_bytes: usize) -> &str {
    let mut end = available_bytes.min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
