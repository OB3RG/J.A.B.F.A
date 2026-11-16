use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;

use crate::OUTPUT_SCHEMA_VERSION;
use crate::error::Result;
use crate::index::IndexConfig;
use crate::query::{SearchResult, search_sections};

/// A unique document/subject with its best matching section citation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SubjectResult {
    pub path: PathBuf,
    pub title: String,
    pub entry_type: String,
    pub match_source: String,
    pub matched_sections: usize,
    pub section_id: i64,
    pub heading_path: Vec<String>,
    pub start_line: usize,
    pub end_line: usize,
    pub content_hash: String,
    pub score: f64,
    pub snippet: String,
}

/// Bounded subject-discovery results over ranked section matches.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SubjectSearchResponse {
    pub output_schema_version: u32,
    pub generation: i64,
    pub results: Vec<SubjectResult>,
}

/// Group section matches into unique entries for discovery before investigation.
pub fn search_subjects(
    config: &IndexConfig,
    query: &str,
    limit: usize,
) -> Result<SubjectSearchResponse> {
    let search = search_sections(config, query, 100)?;
    let connection = Connection::open_with_flags(
        &config.database_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;

    let mut grouped: BTreeMap<PathBuf, (SearchResult, usize)> = BTreeMap::new();
    for result in search.results {
        grouped
            .entry(result.path.clone())
            .and_modify(|(_, count)| *count += 1)
            .or_insert((result, 1));
    }

    let mut results = grouped
        .into_iter()
        .map(|(path, (section, matched_sections))| {
            subject_result(&connection, query, path, section, matched_sections)
        })
        .collect::<Result<Vec<_>>>()?;
    results.sort_by(|left, right| {
        match_rank(&left.match_source)
            .cmp(&match_rank(&right.match_source))
            .then_with(|| registry_rank(&left.path).cmp(&registry_rank(&right.path)))
            .then_with(|| left.score.total_cmp(&right.score))
            .then_with(|| left.path.cmp(&right.path))
    });
    results.truncate(limit.min(100));

    Ok(SubjectSearchResponse {
        output_schema_version: OUTPUT_SCHEMA_VERSION,
        generation: search.generation,
        results,
    })
}

fn subject_result(
    connection: &Connection,
    query: &str,
    path: PathBuf,
    section: SearchResult,
    matched_sections: usize,
) -> Result<SubjectResult> {
    let (document_title, frontmatter): (Option<String>, Option<String>) = connection.query_row(
        "SELECT title, frontmatter FROM documents WHERE path = ?1",
        [path.to_string_lossy()],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let frontmatter: Option<serde_json::Value> = frontmatter
        .map(|value| serde_json::from_str(&value))
        .transpose()?;
    let frontmatter_title = frontmatter
        .as_ref()
        .and_then(|value| value.get("title"))
        .and_then(serde_json::Value::as_str);
    let title = document_title
        .filter(|title| !title.is_empty())
        .or_else(|| frontmatter_title.map(str::to_owned))
        .unwrap_or_else(|| {
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string())
        });
    let title_matches = matches_query(&title, query)
        || frontmatter_title.is_some_and(|value| matches_query(value, query));
    let alias_matches = frontmatter
        .as_ref()
        .and_then(|value| value.get("aliases"))
        .is_some_and(|aliases| match aliases {
            serde_json::Value::String(alias) => matches_query(alias.as_str(), query),
            serde_json::Value::Array(aliases) => aliases
                .iter()
                .filter_map(serde_json::Value::as_str)
                .any(|alias| matches_query(alias, query)),
            _ => false,
        });
    let match_source = if title_matches {
        "title"
    } else if alias_matches {
        "alias"
    } else {
        "content"
    };
    let entry_type = frontmatter
        .as_ref()
        .and_then(|value| value.get("type"))
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| default_entry_type(&path).to_owned());

    Ok(SubjectResult {
        path,
        title,
        entry_type,
        match_source: match_source.to_owned(),
        matched_sections,
        section_id: section.section_id,
        heading_path: section.heading_path,
        start_line: section.start_line,
        end_line: section.end_line,
        content_hash: section.content_hash,
        score: section.score,
        snippet: section.snippet,
    })
}

fn matches_query(value: &str, query: &str) -> bool {
    let value_tokens = normalized_tokens(value);
    let query_tokens = normalized_tokens(query);
    !query_tokens.is_empty() && query_tokens.iter().all(|term| value_tokens.contains(term))
}

fn normalized_tokens(value: &str) -> Vec<String> {
    value
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_lowercase)
        .collect()
}

fn match_rank(match_source: &str) -> u8 {
    match match_source {
        "title" => 0,
        "alias" => 1,
        _ => 2,
    }
}

fn registry_rank(path: &Path) -> u8 {
    u8::from(is_registry(path))
}

fn is_registry(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| matches!(name.to_string_lossy().as_ref(), "log.md" | "index.md"))
}

fn default_entry_type(path: &Path) -> &'static str {
    match path.file_name().map(|name| name.to_string_lossy()) {
        Some(name) if name == "log.md" => "log",
        Some(name) if name == "index.md" => "index",
        _ => "document",
    }
}
