use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use rusqlite::{Connection, OpenFlags, OptionalExtension};

use crate::OUTPUT_SCHEMA_VERSION;
use crate::error::{JabfaError, Result, io_error};
use crate::index::{IndexConfig, IndexDiagnostic, discover_markdown};
use crate::query::read_generation;

/// Structured consistency findings for the current source/index pair.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct DoctorResponse {
    pub output_schema_version: u32,
    pub generation: i64,
    pub healthy: bool,
    pub diagnostics: Vec<IndexDiagnostic>,
}

/// Detect changed, missing, unindexed, broken, and ambiguous index entries.
pub fn doctor(config: &IndexConfig) -> Result<DoctorResponse> {
    if !config.vault_root.is_dir() {
        return Err(JabfaError::InvalidVaultRoot(config.vault_root.clone()));
    }
    if !config.database_path.is_file() {
        return Err(JabfaError::DatabaseNotFound(config.database_path.clone()));
    }
    let connection = Connection::open_with_flags(
        &config.database_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let generation = read_generation(&connection)?;
    let current_files = discover_markdown(&config.vault_root)?;
    let mut diagnostics = Vec::new();
    let mut statement = connection.prepare("SELECT path FROM documents ORDER BY path")?;
    let indexed_paths = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(statement);
    let indexed = indexed_paths.iter().cloned().collect::<BTreeSet<_>>();

    for path in indexed_paths {
        let full_path = config.vault_root.join(&path);
        if !full_path.is_file() {
            diagnostics.push(diagnostic(
                &path,
                "missing",
                "indexed Markdown source is missing",
            ));
            continue;
        }
        let source = match fs::read(&full_path) {
            Ok(source) => source,
            Err(error) => {
                diagnostics.push(diagnostic(
                    &path,
                    "io",
                    format!(
                        "cannot read indexed source: {}",
                        io_error(&full_path, error)
                    ),
                ));
                continue;
            }
        };
        let expected = connection
            .query_row(
                "SELECT value FROM index_metadata WHERE key = ?1",
                [format!("document_hash:{path}")],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let actual = blake3::hash(&source).to_hex().to_string();
        if expected.as_deref() != Some(actual.as_str()) {
            diagnostics.push(diagnostic(
                &path,
                "changed",
                "source content hash differs from the committed index",
            ));
        }
    }

    for path in current_files {
        if !indexed.contains(&path.to_string_lossy().into_owned()) {
            diagnostics.push(diagnostic(
                &path,
                "unindexed",
                "Markdown source is not present in the committed index",
            ));
        }
    }

    let mut statement = connection.prepare(
        "SELECT sources.path, links.line_number, links.raw_target, links.resolution
         FROM links
         JOIN documents sources ON sources.id = links.source_document_id
         WHERE links.resolution IN ('broken', 'ambiguous')
         ORDER BY sources.path, links.line_number, links.id",
    )?;
    let unresolved = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, usize>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (path, line_number, raw_target, resolution) in unresolved {
        let kind = if resolution == "broken" {
            "broken_link"
        } else {
            "ambiguous_link"
        };
        diagnostics.push(diagnostic(
            &path,
            kind,
            format!("line {line_number}: {resolution} link target `{raw_target}`"),
        ));
    }

    Ok(DoctorResponse {
        output_schema_version: OUTPUT_SCHEMA_VERSION,
        generation,
        healthy: diagnostics.is_empty(),
        diagnostics,
    })
}

fn diagnostic(path: impl AsRef<Path>, kind: &str, message: impl Into<String>) -> IndexDiagnostic {
    let path = path.as_ref().to_path_buf();
    IndexDiagnostic {
        path,
        kind: kind.to_owned(),
        message: message.into(),
    }
}
