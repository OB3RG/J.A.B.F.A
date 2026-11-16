use std::path::PathBuf;

use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OpenFlags, params_from_iter};
use serde::Serialize;

use crate::OUTPUT_SCHEMA_VERSION;
use crate::error::{JabfaError, Result};
use crate::index::IndexConfig;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchResult {
    pub section_id: i64,
    pub path: PathBuf,
    pub heading_path: Vec<String>,
    pub start_line: usize,
    pub end_line: usize,
    pub content_hash: String,
    pub score: f64,
    pub snippet: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchResponse {
    pub output_schema_version: u32,
    pub generation: i64,
    pub results: Vec<SearchResult>,
}

pub fn search_sections(config: &IndexConfig, query: &str, limit: usize) -> Result<SearchResponse> {
    search_sections_impl(config, query, limit, None)
}

pub(crate) fn search_sections_in_paths(
    config: &IndexConfig,
    query: &str,
    paths: &[PathBuf],
    limit: usize,
) -> Result<SearchResponse> {
    search_sections_impl(config, query, limit, Some(paths))
}

fn search_sections_impl(
    config: &IndexConfig,
    query: &str,
    limit: usize,
    selected_paths: Option<&[PathBuf]>,
) -> Result<SearchResponse> {
    if !config.database_path.is_file() {
        return Err(JabfaError::DatabaseNotFound(config.database_path.clone()));
    }

    let connection = Connection::open_with_flags(
        &config.database_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let generation = read_generation(&connection)?;
    if query.trim().is_empty() || limit == 0 || selected_paths.is_some_and(<[PathBuf]>::is_empty) {
        return Ok(SearchResponse {
            output_schema_version: OUTPUT_SCHEMA_VERSION,
            generation,
            results: Vec::new(),
        });
    }

    let mut parameters = vec![SqlValue::Text(literal_fts_query(query))];
    let mut sql = "SELECT
            documents.path,
            sections.heading_path,
            sections.start_line,
            sections.end_line,
            sections.content_hash,
            bm25(section_search) AS score,
            snippet(section_search, 1, '', '', '…', 13),
            sections.id
         FROM section_search
         JOIN sections ON sections.id = section_search.section_id
         JOIN documents ON documents.id = sections.document_id
         WHERE section_search MATCH ?"
        .to_owned();
    if let Some(paths) = selected_paths {
        sql.push_str(" AND documents.path IN (");
        sql.push_str(&vec!["?"; paths.len()].join(","));
        sql.push(')');
        parameters.extend(
            paths
                .iter()
                .map(|path| SqlValue::Text(path.to_string_lossy().into_owned())),
        );
    }
    sql.push_str(" ORDER BY score ASC, documents.path ASC, sections.start_line ASC LIMIT ?");
    parameters.push(SqlValue::Integer(limit.min(100) as i64));

    let mut statement = connection.prepare(&sql)?;
    let results = statement
        .query_map(params_from_iter(parameters), |row| {
            let path: String = row.get(0)?;
            let heading_path: String = row.get(1)?;
            let score: f64 = row.get(5)?;
            Ok(SearchResult {
                section_id: row.get(7)?,
                path: PathBuf::from(path),
                heading_path: if heading_path.is_empty() {
                    Vec::new()
                } else {
                    heading_path.split('\n').map(str::to_owned).collect()
                },
                start_line: row.get(2)?,
                end_line: row.get(3)?,
                content_hash: row.get(4)?,
                score: canonical_score(score),
                snippet: row.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(SearchResponse {
        output_schema_version: OUTPUT_SCHEMA_VERSION,
        generation,
        results,
    })
}

fn literal_fts_query(query: &str) -> String {
    query
        .split_whitespace()
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ")
}

fn canonical_score(score: f64) -> f64 {
    format!("{score:.12e}").parse().unwrap_or(score)
}

pub(crate) fn read_generation(connection: &Connection) -> Result<i64> {
    let value: String = connection.query_row(
        "SELECT value FROM index_metadata WHERE key = 'generation'",
        [],
        |row| row.get(0),
    )?;
    value.parse().map_err(|_| JabfaError::InvalidIndexMetadata {
        key: "generation".to_owned(),
        value,
    })
}
