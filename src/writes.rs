use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Serialize;
use tempfile::NamedTempFile;

use crate::OUTPUT_SCHEMA_VERSION;
use crate::error::{JabfaError, Result, io_error};
use crate::index::{IndexConfig, index_status, index_vault_incremental};
use crate::navigation::{read_source, safe_relative_path};
use crate::parser::parse_markdown;

/// Input for creating one structured Markdown learning note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateLearningRequest {
    pub path: PathBuf,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
}

/// Input for replacing one exact indexed section citation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchSectionRequest {
    pub path: PathBuf,
    pub start_line: usize,
    pub end_line: usize,
    pub expected_hash: String,
    pub replacement: String,
}

/// A source commit that has also been verified in a committed index generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WriteResponse {
    pub output_schema_version: u32,
    pub path: PathBuf,
    pub content_hash: String,
    pub bytes: usize,
    pub generation: i64,
}

/// Create one structured learning note in a validated vault location.
pub fn create_learning(
    config: &IndexConfig,
    request: &CreateLearningRequest,
) -> Result<WriteResponse> {
    validate_write_path(&request.path)?;
    validate_single_line(&request.title, "title")?;
    if request.title.trim().is_empty() || request.body.trim().is_empty() {
        return Err(JabfaError::InvalidWriteInput(
            "title and body must be nonempty".to_owned(),
        ));
    }
    for tag in &request.tags {
        validate_single_line(tag, "tag")?;
        if tag.trim().is_empty() {
            return Err(JabfaError::InvalidWriteInput(
                "tags must be nonempty".to_owned(),
            ));
        }
    }
    let content = render_learning(request)?;
    parse_markdown(&request.path, &content)?;
    let mut response = commit_content(config, &request.path, &content, true)?;
    if let Some(catalog_path) = &config.generated_catalog_path {
        response.generation = update_generated_output(
            config,
            request,
            catalog_path,
            "Learning catalog",
            format!(
                "- [[{}]] — {}\n",
                request.path.with_extension("").to_string_lossy(),
                request.title
            ),
        )?;
    }
    if let Some(log_path) = &config.generated_log_path {
        response.generation = update_generated_output(
            config,
            request,
            log_path,
            "Learning log",
            format!(
                "- `{}` — {}\n",
                request.path.to_string_lossy(),
                request.title
            ),
        )?;
    }
    Ok(response)
}

/// Replace an exact indexed section after validating its content hash.
pub fn patch_section(config: &IndexConfig, request: &PatchSectionRequest) -> Result<WriteResponse> {
    validate_write_path(&request.path)?;
    if request.start_line == 0 || request.end_line < request.start_line {
        return Err(JabfaError::InvalidSourceRange {
            start: request.start_line,
            end: request.end_line,
        });
    }
    if !config.database_path.is_file() {
        return Err(JabfaError::DatabaseNotFound(config.database_path.clone()));
    }
    let connection = Connection::open_with_flags(
        &config.database_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let document_id = connection
        .query_row(
            "SELECT id FROM documents WHERE path = ?1",
            params![request.path.to_string_lossy()],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .ok_or_else(|| JabfaError::DocumentNotFound(request.path.clone()))?;
    let indexed_hash = connection
        .query_row(
            "SELECT content_hash FROM sections
             WHERE document_id = ?1 AND start_line = ?2 AND end_line = ?3",
            params![document_id, request.start_line, request.end_line],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .ok_or_else(|| JabfaError::SectionNotFound {
            path: request.path.clone(),
            start_line: request.start_line,
            end_line: request.end_line,
        })?;
    if indexed_hash != request.expected_hash {
        return Err(JabfaError::StaleCitation {
            path: request.path.clone(),
            start_line: request.start_line,
            end_line: request.end_line,
        });
    }
    drop(connection);

    let source = read_source(config, &request.path)?;
    let lines = source.split_inclusive('\n').collect::<Vec<_>>();
    if request.end_line > lines.len() {
        return Err(JabfaError::InvalidSourceRange {
            start: request.start_line,
            end: request.end_line,
        });
    }
    let mut proposed = lines[..request.start_line - 1].concat();
    proposed.push_str(&request.replacement);
    if !request.replacement.is_empty() && !request.replacement.ends_with('\n') {
        proposed.push('\n');
    }
    proposed.push_str(&lines[request.end_line..].concat());
    parse_markdown(&request.path, &proposed)?;
    commit_content(config, &request.path, &proposed, false)
}

fn update_generated_output(
    config: &IndexConfig,
    request: &CreateLearningRequest,
    path: &Path,
    heading: &str,
    entry: String,
) -> Result<i64> {
    validate_write_path(path)?;
    if path == request.path {
        return Err(JabfaError::InvalidWriteInput(
            "generated output must differ from the learning path".to_owned(),
        ));
    }
    let destination = config.vault_root.join(path);
    let exists = destination.is_file();
    let mut content = if exists {
        read_source(config, path)?
    } else {
        format!("# {heading}\n\n")
    };
    if content.lines().any(|line| line == entry.trim_end()) {
        return Ok(index_status(config)?.generation);
    }
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&entry);
    parse_markdown(path, &content)?;
    Ok(commit_content(config, path, &content, !exists)?.generation)
}

fn render_learning(request: &CreateLearningRequest) -> Result<String> {
    let title = serde_json::to_string(&request.title)?;
    let mut content = format!("---\ntitle: {title}\n");
    if request.tags.is_empty() {
        content.push_str("tags: []\n");
    } else {
        content.push_str("tags:\n");
        let mut tags = request.tags.clone();
        tags.sort();
        tags.dedup();
        for tag in tags {
            content.push_str(&format!("  - {}\n", serde_json::to_string(&tag)?));
        }
    }
    content.push_str(&format!("---\n# {}\n\n", request.title));
    content.push_str(request.body.trim());
    content.push('\n');
    Ok(content)
}

pub(crate) fn validate_single_line(value: &str, name: &str) -> Result<()> {
    if value.contains(['\n', '\r']) {
        return Err(JabfaError::InvalidWriteInput(format!(
            "{name} must be a single line"
        )));
    }
    Ok(())
}

pub(crate) fn validate_write_path(path: &Path) -> Result<PathBuf> {
    let safe = safe_relative_path(path)?;
    if path
        .components()
        .any(|component| matches!(component, Component::Normal(part) if part == "raw"))
    {
        return Err(JabfaError::ImmutableRawPath(path.to_path_buf()));
    }
    if safe
        .extension()
        .and_then(|extension| extension.to_str())
        .is_none_or(|extension| !extension.eq_ignore_ascii_case("md"))
    {
        return Err(JabfaError::InvalidWriteInput(
            "write path must end in .md".to_owned(),
        ));
    }
    Ok(safe)
}

pub(crate) fn commit_content(
    config: &IndexConfig,
    path: &Path,
    content: &str,
    create: bool,
) -> Result<WriteResponse> {
    atomic_write(config, path, content, create)?;
    let source_hash = blake3::hash(content.as_bytes()).to_hex().to_string();
    let report = match index_vault_incremental(config) {
        Ok(report) => report,
        Err(error) => {
            return Err(JabfaError::WriteRecoveryRequired {
                path: path.to_path_buf(),
                source_hash,
                message: error.to_string(),
            });
        }
    };
    if let Err(error) = verify_committed_source(config, path, &source_hash) {
        return Err(JabfaError::WriteRecoveryRequired {
            path: path.to_path_buf(),
            source_hash,
            message: error.to_string(),
        });
    }
    Ok(WriteResponse {
        output_schema_version: OUTPUT_SCHEMA_VERSION,
        path: path.to_path_buf(),
        content_hash: source_hash,
        bytes: content.len(),
        generation: report.generation,
    })
}

fn atomic_write(config: &IndexConfig, path: &Path, content: &str, create: bool) -> Result<()> {
    if !config.vault_root.is_dir() {
        return Err(JabfaError::InvalidVaultRoot(config.vault_root.clone()));
    }
    let root = fs::canonicalize(&config.vault_root)
        .map_err(|error| io_error(&config.vault_root, error))?;
    let destination = config.vault_root.join(path);
    if create && destination.exists() {
        return Err(JabfaError::DestinationExists(path.to_path_buf()));
    }
    let parent = destination.parent().ok_or_else(|| {
        JabfaError::InvalidWriteInput("write path has no parent directory".to_owned())
    })?;
    validate_destination_ancestor(&root, parent)?;
    if create {
        fs::create_dir_all(parent).map_err(|error| io_error(parent, error))?;
    }
    let canonical_parent = fs::canonicalize(parent).map_err(|error| io_error(parent, error))?;
    if !canonical_parent.starts_with(&root) {
        return Err(JabfaError::PathOutsideVault(path.to_path_buf()));
    }

    let mut temporary = NamedTempFile::new_in(&canonical_parent)
        .map_err(|error| io_error(&canonical_parent, error))?;
    temporary
        .write_all(content.as_bytes())
        .map_err(|error| io_error(temporary.path(), error))?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|error| io_error(temporary.path(), error))?;
    if create {
        temporary
            .persist_noclobber(&destination)
            .map_err(|error| io_error(&destination, error.error))?;
    } else {
        temporary
            .persist(&destination)
            .map_err(|error| io_error(&destination, error.error))?;
    }
    Ok(())
}

fn validate_destination_ancestor(root: &Path, parent: &Path) -> Result<()> {
    let mut ancestor = parent;
    while !ancestor.exists() {
        ancestor = ancestor.parent().ok_or_else(|| {
            JabfaError::InvalidWriteInput("write path has no existing ancestor".to_owned())
        })?;
    }
    let canonical = fs::canonicalize(ancestor).map_err(|error| io_error(ancestor, error))?;
    if !canonical.starts_with(root) {
        return Err(JabfaError::PathOutsideVault(parent.to_path_buf()));
    }
    Ok(())
}

fn verify_committed_source(config: &IndexConfig, path: &Path, source_hash: &str) -> Result<()> {
    let actual = read_source(config, path)?;
    if blake3::hash(actual.as_bytes()).to_hex().to_string() != source_hash {
        return Err(JabfaError::InvalidWriteInput(
            "source changed while indexing the write".to_owned(),
        ));
    }
    let connection = Connection::open_with_flags(
        &config.database_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let indexed = connection
        .query_row(
            "SELECT value FROM index_metadata WHERE key = ?1",
            [format!("document_hash:{}", path.to_string_lossy())],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if indexed.as_deref() != Some(source_hash) {
        return Err(JabfaError::InvalidWriteInput(
            "index source hash does not match the committed write".to_owned(),
        ));
    }
    Ok(())
}
