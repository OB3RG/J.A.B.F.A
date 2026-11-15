use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::path::{Component, Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Serialize;

use crate::OUTPUT_SCHEMA_VERSION;
use crate::error::{JabfaError, Result, io_error};
use crate::index::IndexConfig;
use crate::query::read_generation;

const MAX_SOURCE_BYTES: usize = 1_048_576;
const MAX_LINKS: usize = 1_000;
const MAX_DEPTH: usize = 8;
const MAX_NODES: usize = 100;
const MAX_GRAPH_BYTES: usize = 65_536;

/// Exact source content for a complete document or an indexed section range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceResponse {
    pub output_schema_version: u32,
    pub generation: i64,
    pub path: PathBuf,
    pub start_line: usize,
    pub end_line: usize,
    pub content: String,
    pub content_hash: String,
    pub bytes: usize,
}

/// One indexed outgoing link and its resolution state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinkEntry {
    pub kind: String,
    pub raw_target: String,
    pub target_heading: Option<String>,
    pub resolution: String,
    pub line_number: usize,
    pub target_path: Option<PathBuf>,
}

/// Bounded outgoing links for one document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinksResponse {
    pub output_schema_version: u32,
    pub generation: i64,
    pub path: PathBuf,
    pub links: Vec<LinkEntry>,
}

/// Limits for deterministic bidirectional related-document traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RelatedLimits {
    pub max_depth: usize,
    pub max_nodes: usize,
    pub max_bytes: usize,
}

/// A document discovered at a specific breadth-first depth.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelatedNode {
    pub path: PathBuf,
    pub depth: usize,
}

/// An unresolved outgoing link encountered during graph traversal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelatedDiagnostic {
    pub path: PathBuf,
    pub line_number: usize,
    pub raw_target: String,
    pub resolution: String,
    pub message: String,
}

/// A bounded related-document graph payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RelatedResponse {
    pub output_schema_version: u32,
    pub generation: i64,
    pub root: PathBuf,
    pub nodes: Vec<RelatedNode>,
    pub diagnostics: Vec<RelatedDiagnostic>,
    pub bytes: usize,
}

/// Read a complete indexed document without changing the source vault.
pub fn get_document(config: &IndexConfig, path: &Path, max_bytes: usize) -> Result<SourceResponse> {
    get_source(config, path, None, max_bytes)
}

/// Read the exact source lines for an indexed section citation.
pub fn get_section(
    config: &IndexConfig,
    path: &Path,
    start_line: usize,
    end_line: usize,
    max_bytes: usize,
) -> Result<SourceResponse> {
    get_source(config, path, Some((start_line, end_line)), max_bytes)
}

fn get_source(
    config: &IndexConfig,
    path: &Path,
    range: Option<(usize, usize)>,
    max_bytes: usize,
) -> Result<SourceResponse> {
    let path = safe_relative_path(path)?;
    let connection = open_index(config)?;
    let generation = read_generation(&connection)?;
    let path_text = path.to_string_lossy();
    match range {
        Some((start_line, end_line)) => {
            let exists = connection
                .query_row(
                    "SELECT 1 FROM sections
                     JOIN documents ON documents.id = sections.document_id
                     WHERE documents.path = ?1 AND sections.start_line = ?2
                       AND sections.end_line = ?3",
                    params![path_text, start_line, end_line],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?;
            if exists.is_none() {
                return Err(JabfaError::SectionNotFound {
                    path: path.clone(),
                    start_line,
                    end_line,
                });
            }
        }
        None => ensure_document(&connection, &path)?,
    }

    let source = read_source(config, &path)?;
    let line_count = source_line_count(&source);
    let (start_line, end_line, content) = match range {
        Some((start, end)) => {
            if start == 0 || end < start || end > line_count {
                return Err(JabfaError::InvalidSourceRange { start, end });
            }
            let content = source
                .split_inclusive('\n')
                .skip(start - 1)
                .take(end - start + 1)
                .collect::<String>();
            (start, end, content)
        }
        None => (1, line_count, source),
    };
    let max_bytes = max_bytes.min(MAX_SOURCE_BYTES);
    if content.len() > max_bytes {
        return Err(JabfaError::SourceByteLimitExceeded { max_bytes });
    }

    Ok(SourceResponse {
        output_schema_version: OUTPUT_SCHEMA_VERSION,
        generation,
        path,
        start_line,
        end_line,
        content_hash: blake3::hash(content.as_bytes()).to_hex().to_string(),
        bytes: content.len(),
        content,
    })
}

/// List one document's outgoing links in source-line order.
pub fn list_links(config: &IndexConfig, path: &Path, max_links: usize) -> Result<LinksResponse> {
    let path = safe_relative_path(path)?;
    let connection = open_index(config)?;
    let generation = read_generation(&connection)?;
    ensure_document(&connection, &path)?;
    let max_links = max_links.min(MAX_LINKS);
    let query_limit = i64::try_from(max_links).unwrap_or(MAX_LINKS as i64) + 1;
    let mut statement = connection.prepare(
        "SELECT links.kind, links.raw_target, links.target_heading, links.resolution,
                links.line_number, targets.path
         FROM links
         JOIN documents sources ON sources.id = links.source_document_id
         LEFT JOIN documents targets ON targets.id = links.target_document_id
         WHERE sources.path = ?1
         ORDER BY links.line_number, links.id
         LIMIT ?2",
    )?;
    let links = statement
        .query_map(params![path.to_string_lossy(), query_limit], |row| {
            let target_path: Option<String> = row.get(5)?;
            Ok(LinkEntry {
                kind: row.get(0)?,
                raw_target: row.get(1)?,
                target_heading: row.get(2)?,
                resolution: row.get(3)?,
                line_number: row.get(4)?,
                target_path: target_path.map(PathBuf::from),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if links.len() > max_links {
        return Err(JabfaError::LinkResultLimitExceeded { max_links });
    }

    Ok(LinksResponse {
        output_schema_version: OUTPUT_SCHEMA_VERSION,
        generation,
        path,
        links,
    })
}

/// Traverse resolved outgoing and incoming links breadth first.
pub fn related_documents(
    config: &IndexConfig,
    root: &Path,
    limits: &RelatedLimits,
) -> Result<RelatedResponse> {
    let root = safe_relative_path(root)?;
    let connection = open_index(config)?;
    let generation = read_generation(&connection)?;
    ensure_document(&connection, &root)?;
    let max_depth = limits.max_depth.min(MAX_DEPTH);
    let max_nodes = limits.max_nodes.min(MAX_NODES);
    let max_bytes = limits.max_bytes.min(MAX_GRAPH_BYTES);

    let mut nodes = vec![RelatedNode {
        path: root.clone(),
        depth: 0,
    }];
    let mut diagnostics = Vec::new();
    let mut visited = BTreeSet::from([root.clone()]);
    let mut queue = VecDeque::from([(root.clone(), 0_usize)]);

    while let Some((current, depth)) = queue.pop_front() {
        if depth >= max_depth {
            continue;
        }
        let current_text = current.to_string_lossy();
        let mut statement = connection.prepare(
            "SELECT sources.path, targets.path, links.raw_target, links.resolution,
                    links.line_number
             FROM links
             JOIN documents sources ON sources.id = links.source_document_id
             LEFT JOIN documents targets ON targets.id = links.target_document_id
             WHERE sources.path = ?1 OR targets.path = ?1
             ORDER BY links.line_number, sources.path, links.raw_target, links.id",
        )?;
        let edges = statement
            .query_map(params![current_text], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, usize>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;

        for (source, target, raw_target, resolution, line_number) in edges {
            let is_outgoing = source == current_text;
            let neighbor = if is_outgoing {
                if resolution != "resolved" {
                    diagnostics.push(RelatedDiagnostic {
                        path: current.clone(),
                        line_number,
                        raw_target: raw_target.clone(),
                        message: format!("{resolution} link: {raw_target}"),
                        resolution,
                    });
                    continue;
                }
                target
            } else {
                Some(source)
            };
            let Some(neighbor) = neighbor.map(PathBuf::from) else {
                continue;
            };
            if visited.contains(&neighbor) {
                continue;
            }
            if nodes.len() >= max_nodes {
                return Err(JabfaError::GraphNodeLimitExceeded { max_nodes });
            }
            visited.insert(neighbor.clone());
            nodes.push(RelatedNode {
                path: neighbor.clone(),
                depth: depth + 1,
            });
            queue.push_back((neighbor, depth + 1));
        }
    }

    let bytes = serde_json::to_vec(&serde_json::json!({
        "nodes": nodes,
        "diagnostics": diagnostics,
    }))?
    .len();
    if bytes > max_bytes {
        return Err(JabfaError::GraphByteLimitExceeded { max_bytes });
    }

    Ok(RelatedResponse {
        output_schema_version: OUTPUT_SCHEMA_VERSION,
        generation,
        root,
        nodes,
        diagnostics,
        bytes,
    })
}

fn open_index(config: &IndexConfig) -> Result<Connection> {
    if !config.database_path.is_file() {
        return Err(JabfaError::DatabaseNotFound(config.database_path.clone()));
    }
    Ok(Connection::open_with_flags(
        &config.database_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?)
}

fn ensure_document(connection: &Connection, path: &Path) -> Result<()> {
    let exists = connection
        .query_row(
            "SELECT 1 FROM documents WHERE path = ?1",
            params![path.to_string_lossy()],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    if exists.is_none() {
        return Err(JabfaError::DocumentNotFound(path.to_path_buf()));
    }
    Ok(())
}

fn safe_relative_path(path: &Path) -> Result<PathBuf> {
    let mut safe = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => safe.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(JabfaError::PathOutsideVault(path.to_path_buf()));
            }
        }
    }
    if safe.as_os_str().is_empty() {
        return Err(JabfaError::PathOutsideVault(path.to_path_buf()));
    }
    Ok(safe)
}

fn read_source(config: &IndexConfig, path: &Path) -> Result<String> {
    if !config.vault_root.is_dir() {
        return Err(JabfaError::InvalidVaultRoot(config.vault_root.clone()));
    }
    let root = fs::canonicalize(&config.vault_root)
        .map_err(|error| io_error(&config.vault_root, error))?;
    let full_path = config.vault_root.join(path);
    let canonical = fs::canonicalize(&full_path).map_err(|error| io_error(&full_path, error))?;
    if !canonical.starts_with(&root) || !canonical.is_file() {
        return Err(JabfaError::PathOutsideVault(path.to_path_buf()));
    }
    fs::read_to_string(&canonical).map_err(|error| io_error(canonical, error))
}

fn source_line_count(source: &str) -> usize {
    if source.is_empty() {
        0
    } else {
        source.split_inclusive('\n').count()
    }
}
