use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, params};
use serde::Serialize;

use crate::OUTPUT_SCHEMA_VERSION;
use crate::error::{JabfaError, Result, io_error};
use crate::links::LinkResolver;
use crate::model::{Link, LinkKind, ParsedDocument, Section};
use crate::parser::parse_markdown;

const IGNORED_DIRECTORIES: [&str; 3] = [".git", ".obsidian", ".jabfa"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexConfig {
    pub(crate) vault_root: PathBuf,
    pub(crate) database_path: PathBuf,
}

impl IndexConfig {
    pub fn new(vault_root: impl AsRef<Path>, database_path: impl AsRef<Path>) -> Self {
        Self {
            vault_root: vault_root.as_ref().to_path_buf(),
            database_path: database_path.as_ref().to_path_buf(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IndexDiagnostic {
    pub path: PathBuf,
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IndexReport {
    pub output_schema_version: u32,
    pub files: Vec<PathBuf>,
    pub documents_indexed: usize,
    pub sections_indexed: usize,
    pub generation: i64,
    pub diagnostics: Vec<IndexDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IndexStatus {
    pub output_schema_version: u32,
    pub generation: i64,
    pub schema_version: String,
    pub state: String,
    pub documents: i64,
    pub sections: i64,
}

pub fn index_vault(config: &IndexConfig) -> Result<IndexReport> {
    if !config.vault_root.is_dir() {
        return Err(JabfaError::InvalidVaultRoot(config.vault_root.clone()));
    }

    let files = discover_markdown(&config.vault_root)?;
    let mut documents = Vec::with_capacity(files.len());
    let mut diagnostics = Vec::new();
    for relative_path in &files {
        let full_path = config.vault_root.join(relative_path);
        let source = match fs::read_to_string(&full_path) {
            Ok(source) => source,
            Err(source) => {
                diagnostics.push(document_diagnostic(
                    relative_path,
                    io_error(full_path.clone(), source),
                ));
                continue;
            }
        };
        match parse_markdown(relative_path, &source) {
            Ok(document) => documents.push(document),
            Err(error) => diagnostics.push(document_diagnostic(relative_path, error)),
        }
    }

    let generation = persist_documents(&config.database_path, &documents)?;

    let sections_indexed = documents
        .iter()
        .map(|document| document.sections.len())
        .sum();
    Ok(IndexReport {
        output_schema_version: OUTPUT_SCHEMA_VERSION,
        documents_indexed: documents.len(),
        sections_indexed,
        generation,
        files,
        diagnostics,
    })
}

/// Reindex only added, changed, deleted, or newly invalid Markdown documents.
pub fn index_vault_incremental(config: &IndexConfig) -> Result<IndexReport> {
    if !config.vault_root.is_dir() {
        return Err(JabfaError::InvalidVaultRoot(config.vault_root.clone()));
    }
    if !config.database_path.is_file() {
        return index_vault(config);
    }

    let files = discover_markdown(&config.vault_root)?;
    let connection = Connection::open_with_flags(
        &config.database_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let mut stored_hashes = HashMap::new();
    let mut statement = connection
        .prepare("SELECT key, value FROM index_metadata WHERE key LIKE 'document_hash:%'")?;
    let hashes = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (key, value) in hashes {
        if let Some(path) = key.strip_prefix("document_hash:") {
            stored_hashes.insert(PathBuf::from(path), value);
        }
    }

    let mut documents = Vec::with_capacity(files.len());
    let mut diagnostics = Vec::new();
    let mut invalidated = BTreeSet::new();
    for relative_path in &files {
        let full_path = config.vault_root.join(relative_path);
        let source = match fs::read_to_string(&full_path) {
            Ok(source) => source,
            Err(source) => {
                invalidated.insert(relative_path.clone());
                diagnostics.push(document_diagnostic(
                    relative_path,
                    io_error(full_path.clone(), source),
                ));
                continue;
            }
        };
        let content_hash = blake3::hash(source.as_bytes()).to_hex().to_string();
        if stored_hashes.get(relative_path) == Some(&content_hash)
            && let Some(document) =
                load_document_stub(&connection, relative_path, content_hash.clone())?
        {
            documents.push(document);
            continue;
        }

        invalidated.insert(relative_path.clone());
        match parse_markdown(relative_path, &source) {
            Ok(document) => documents.push(document),
            Err(error) => diagnostics.push(document_diagnostic(relative_path, error)),
        }
    }
    drop(statement);
    drop(connection);

    let generation = persist_incremental(&config.database_path, &documents, &invalidated, &files)?;
    let connection = Connection::open_with_flags(
        &config.database_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let sections_indexed = connection.query_row("SELECT COUNT(*) FROM sections", [], |row| {
        row.get::<_, usize>(0)
    })?;
    Ok(IndexReport {
        output_schema_version: OUTPUT_SCHEMA_VERSION,
        documents_indexed: documents.len(),
        sections_indexed,
        generation,
        files,
        diagnostics,
    })
}

pub fn index_status(config: &IndexConfig) -> Result<IndexStatus> {
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
    let generation = metadata_number(&connection, "generation")?;
    let schema_version = metadata_text(&connection, "schema_version")?;
    let state = metadata_text(&connection, "state")?;
    let documents = connection.query_row("SELECT COUNT(*) FROM documents", [], |row| row.get(0))?;
    let sections = connection.query_row("SELECT COUNT(*) FROM sections", [], |row| row.get(0))?;
    Ok(IndexStatus {
        output_schema_version: OUTPUT_SCHEMA_VERSION,
        generation,
        schema_version,
        state,
        documents,
        sections,
    })
}

fn metadata_text(connection: &Connection, key: &str) -> Result<String> {
    Ok(connection.query_row(
        "SELECT value FROM index_metadata WHERE key = ?1",
        [key],
        |row| row.get(0),
    )?)
}

fn metadata_number(connection: &Connection, key: &str) -> Result<i64> {
    metadata_number_from_value(key, metadata_text(connection, key)?)
}

fn metadata_number_from_value(key: &str, value: String) -> Result<i64> {
    value.parse().map_err(|_| JabfaError::InvalidIndexMetadata {
        key: key.to_owned(),
        value,
    })
}

fn section_content_hash(section: &Section) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(section.heading.as_deref().unwrap_or("").as_bytes());
    hasher.update(b"\0");
    hasher.update(section.heading_path.join("\n").as_bytes());
    hasher.update(b"\0");
    hasher.update(section.text.as_bytes());
    hasher.finalize().to_hex().to_string()
}

fn document_diagnostic(path: &Path, error: JabfaError) -> IndexDiagnostic {
    let kind = match &error {
        JabfaError::Frontmatter { .. } => "frontmatter",
        JabfaError::Io { .. } => "io",
        JabfaError::Database(_) => "database",
        JabfaError::InvalidVaultRoot(_)
        | JabfaError::InvalidIndexMetadata { .. }
        | JabfaError::DatabaseNotFound(_)
        | JabfaError::PathOutsideVault(_)
        | JabfaError::DocumentNotFound(_)
        | JabfaError::SectionNotFound { .. }
        | JabfaError::InvalidSourceRange { .. }
        | JabfaError::SourceByteLimitExceeded { .. }
        | JabfaError::LinkResultLimitExceeded { .. }
        | JabfaError::GraphNodeLimitExceeded { .. }
        | JabfaError::GraphByteLimitExceeded { .. }
        | JabfaError::UnsafeSql
        | JabfaError::SqlRowLimitExceeded { .. }
        | JabfaError::SqlByteLimitExceeded { .. }
        | JabfaError::SqlQueryTimeout
        | JabfaError::Watch(_)
        | JabfaError::Json(_) => "other",
    };
    IndexDiagnostic {
        path: path.to_path_buf(),
        kind: kind.to_owned(),
        message: error.to_string(),
    }
}

pub(crate) fn discover_markdown(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    visit_directory(root, root, &mut files)?;
    files.sort();
    Ok(files)
}

fn visit_directory(root: &Path, directory: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    let entries = fs::read_dir(directory).map_err(|error| io_error(directory, error))?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| io_error(directory, error))?;
        paths.push(entry.path());
    }
    paths.sort();

    for path in paths {
        let file_type = fs::symlink_metadata(&path)
            .map_err(|error| io_error(path.clone(), error))?
            .file_type();
        if file_type.is_dir() {
            let name = path.file_name().and_then(|name| name.to_str());
            if name.is_some_and(|name| IGNORED_DIRECTORIES.contains(&name)) {
                continue;
            }
            visit_directory(root, &path, files)?;
        } else if file_type.is_file()
            && path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        {
            let relative = path
                .strip_prefix(root)
                .expect("discovered path must remain under its root")
                .to_path_buf();
            files.push(relative);
        }
    }

    Ok(())
}

fn persist_documents(database_path: &Path, documents: &[ParsedDocument]) -> Result<i64> {
    let mut connection = Connection::open(database_path)?;
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    let transaction = connection.transaction()?;
    transaction.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS index_metadata (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS documents (
            id INTEGER PRIMARY KEY,
            path TEXT NOT NULL UNIQUE,
            title TEXT,
            frontmatter TEXT
        );
        CREATE TABLE IF NOT EXISTS sections (
            id INTEGER PRIMARY KEY,
            document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
            heading TEXT,
            heading_path TEXT NOT NULL,
            level INTEGER NOT NULL,
            start_line INTEGER NOT NULL,
            end_line INTEGER NOT NULL,
            text TEXT NOT NULL,
            content_hash TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS tags (
            document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
            tag TEXT NOT NULL,
            UNIQUE(document_id, tag)
        );
        CREATE TABLE IF NOT EXISTS tasks (
            id INTEGER PRIMARY KEY,
            document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
            text TEXT NOT NULL,
            completed INTEGER NOT NULL,
            line_number INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS links (
            id INTEGER PRIMARY KEY,
            source_document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
            target_document_id INTEGER REFERENCES documents(id) ON DELETE CASCADE,
            kind TEXT NOT NULL,
            raw_target TEXT NOT NULL,
            target_heading TEXT,
            resolution TEXT NOT NULL,
            line_number INTEGER NOT NULL
        );
        CREATE VIRTUAL TABLE IF NOT EXISTS section_search USING fts5(
            heading,
            text,
            document_id UNINDEXED,
            section_id UNINDEXED
        );
        DELETE FROM index_metadata WHERE key LIKE 'document_hash:%';
        DELETE FROM section_search;
        DELETE FROM links;
        DELETE FROM tasks;
        DELETE FROM tags;
        DELETE FROM sections;
        DELETE FROM documents;
        ",
    )?;

    let previous_generation = transaction
        .query_row(
            "SELECT value FROM index_metadata WHERE key = 'generation'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let generation = match previous_generation {
        Some(value) => metadata_number_from_value("generation", value)?,
        None => 0,
    } + 1;
    transaction.execute(
        "INSERT OR REPLACE INTO index_metadata (key, value) VALUES ('generation', ?1)",
        [generation.to_string()],
    )?;
    transaction.execute(
        "INSERT OR REPLACE INTO index_metadata (key, value) VALUES ('state', 'indexing')",
        [],
    )?;
    transaction.execute(
        "INSERT OR REPLACE INTO index_metadata (key, value) VALUES ('schema_version', '1')",
        [],
    )?;

    let mut document_ids = HashMap::new();
    for document in documents {
        let document_id = insert_document_row(&transaction, document)?;
        document_ids.insert(document.path.to_string_lossy().into_owned(), document_id);
        insert_document_content(&transaction, document, document_id)?;
        transaction.execute(
            "INSERT OR REPLACE INTO index_metadata (key, value) VALUES (?1, ?2)",
            params![
                format!("document_hash:{}", document.path.to_string_lossy()),
                document.content_hash
            ],
        )?;
    }

    rebuild_links(&transaction, documents, &document_ids)?;

    transaction.execute(
        "INSERT OR REPLACE INTO index_metadata (key, value) VALUES ('state', 'complete')",
        [],
    )?;
    transaction.commit()?;
    Ok(generation)
}

fn persist_incremental(
    database_path: &Path,
    documents: &[ParsedDocument],
    invalidated: &BTreeSet<PathBuf>,
    files: &[PathBuf],
) -> Result<i64> {
    let mut connection = Connection::open(database_path)?;
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    let transaction = connection.transaction()?;
    let previous_generation = transaction
        .query_row(
            "SELECT value FROM index_metadata WHERE key = 'generation'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let generation = match previous_generation {
        Some(value) => metadata_number_from_value("generation", value)?,
        None => 0,
    } + 1;
    transaction.execute(
        "INSERT OR REPLACE INTO index_metadata (key, value) VALUES ('generation', ?1)",
        [generation.to_string()],
    )?;
    transaction.execute(
        "INSERT OR REPLACE INTO index_metadata (key, value) VALUES ('state', 'indexing')",
        [],
    )?;

    let file_paths = files
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect::<BTreeSet<_>>();
    let mut statement = transaction.prepare("SELECT path FROM documents")?;
    let existing_paths = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(statement);
    let mut invalidated = invalidated.clone();
    for path in &existing_paths {
        if !file_paths.contains(path) {
            invalidated.insert(PathBuf::from(path));
        }
    }
    preserve_unambiguous_renames(
        &transaction,
        documents,
        &existing_paths,
        &file_paths,
        &mut invalidated,
    )?;

    for path in &invalidated {
        let path_text = path.to_string_lossy();
        transaction.execute(
            "DELETE FROM section_search WHERE document_id IN (
                 SELECT id FROM documents WHERE path = ?1
             )",
            params![path_text],
        )?;
        transaction.execute("DELETE FROM documents WHERE path = ?1", params![path_text])?;
        transaction.execute(
            "DELETE FROM index_metadata WHERE key = ?1",
            [format!("document_hash:{path_text}")],
        )?;
    }

    for document in documents {
        if !invalidated.contains(&document.path) {
            continue;
        }
        let document_id = insert_document_row(&transaction, document)?;
        insert_document_content(&transaction, document, document_id)?;
        transaction.execute(
            "INSERT OR REPLACE INTO index_metadata (key, value) VALUES (?1, ?2)",
            params![
                format!("document_hash:{}", document.path.to_string_lossy()),
                document.content_hash
            ],
        )?;
    }

    let mut statement = transaction.prepare("SELECT path, id FROM documents")?;
    let document_ids = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<rusqlite::Result<HashMap<_, _>>>()?;
    drop(statement);
    transaction.execute("DELETE FROM links", [])?;
    rebuild_links(&transaction, documents, &document_ids)?;

    transaction.execute(
        "INSERT OR REPLACE INTO index_metadata (key, value) VALUES ('state', 'complete')",
        [],
    )?;
    transaction.commit()?;
    Ok(generation)
}

fn preserve_unambiguous_renames(
    transaction: &Transaction,
    documents: &[ParsedDocument],
    existing_paths: &[String],
    file_paths: &BTreeSet<String>,
    invalidated: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    let existing = existing_paths.iter().cloned().collect::<BTreeSet<_>>();
    let mut deleted_by_hash: BTreeMap<String, Vec<(PathBuf, i64)>> = BTreeMap::new();
    for old_path in existing_paths {
        if file_paths.contains(old_path) {
            continue;
        }
        let hash_key = format!("document_hash:{old_path}");
        let hash = transaction
            .query_row(
                "SELECT value FROM index_metadata WHERE key = ?1",
                [&hash_key],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(hash) = hash else {
            continue;
        };
        let id = transaction.query_row(
            "SELECT id FROM documents WHERE path = ?1",
            [old_path],
            |row| row.get::<_, i64>(0),
        )?;
        deleted_by_hash
            .entry(hash)
            .or_default()
            .push((PathBuf::from(old_path), id));
    }

    let mut added_by_hash: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for document in documents {
        let path_text = document.path.to_string_lossy().into_owned();
        if invalidated.contains(&document.path) && !existing.contains(&path_text) {
            added_by_hash
                .entry(document.content_hash.clone())
                .or_default()
                .push(document.path.clone());
        }
    }

    for (hash, old_paths) in deleted_by_hash {
        if old_paths.len() != 1 {
            continue;
        }
        let Some(new_paths) = added_by_hash.get(&hash) else {
            continue;
        };
        if new_paths.len() != 1 {
            continue;
        }
        let (old_path, document_id) = &old_paths[0];
        let new_path = &new_paths[0];
        let new_path_text = new_path.to_string_lossy();
        transaction.execute(
            "UPDATE documents SET path = ?1 WHERE id = ?2",
            params![new_path_text, document_id],
        )?;
        transaction.execute(
            "DELETE FROM index_metadata WHERE key = ?1",
            [format!("document_hash:{}", old_path.to_string_lossy())],
        )?;
        transaction.execute(
            "INSERT OR REPLACE INTO index_metadata (key, value) VALUES (?1, ?2)",
            params![format!("document_hash:{new_path_text}"), hash],
        )?;
        invalidated.remove(old_path);
        invalidated.remove(new_path);
    }
    Ok(())
}

fn load_document_stub(
    connection: &Connection,
    path: &Path,
    content_hash: String,
) -> Result<Option<ParsedDocument>> {
    let frontmatter = connection
        .query_row(
            "SELECT frontmatter FROM documents WHERE path = ?1",
            params![path.to_string_lossy()],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()?;
    let Some(frontmatter) = frontmatter else {
        return Ok(None);
    };
    let frontmatter = frontmatter
        .map(|value| serde_json::from_str(&value))
        .transpose()?;

    let mut statement = connection.prepare(
        "SELECT kind, raw_target, line_number FROM links
         JOIN documents ON documents.id = links.source_document_id
         WHERE documents.path = ?1 ORDER BY links.id",
    )?;
    let stored_links = statement
        .query_map(params![path.to_string_lossy()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, usize>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut links = Vec::with_capacity(stored_links.len());
    for (kind, raw_target, line_number) in stored_links {
        let kind = match kind.as_str() {
            "markdown" => LinkKind::Markdown,
            "wiki" => LinkKind::Wiki,
            "embed" => LinkKind::Embed,
            value => {
                return Err(JabfaError::InvalidIndexMetadata {
                    key: "links.kind".to_owned(),
                    value: value.to_owned(),
                });
            }
        };
        links.push(Link {
            raw_target,
            kind,
            line_number,
        });
    }

    Ok(Some(ParsedDocument {
        path: path.to_path_buf(),
        content_hash,
        frontmatter,
        tags: Vec::new(),
        tasks: Vec::new(),
        links,
        sections: Vec::new(),
    }))
}

fn insert_document_row(transaction: &Transaction, document: &ParsedDocument) -> Result<i64> {
    let title = document.sections.iter().find_map(|section| {
        if section.level == 1 {
            section.heading.clone()
        } else {
            None
        }
    });
    let frontmatter = document.frontmatter.as_ref().map(ToString::to_string);
    transaction.execute(
        "INSERT INTO documents (path, title, frontmatter) VALUES (?1, ?2, ?3)",
        params![document.path.to_string_lossy(), title, frontmatter],
    )?;
    Ok(transaction.last_insert_rowid())
}

fn insert_document_content(
    transaction: &Transaction,
    document: &ParsedDocument,
    document_id: i64,
) -> Result<()> {
    for section in &document.sections {
        let content_hash = section_content_hash(section);
        transaction.execute(
            "INSERT INTO sections (
                document_id, heading, heading_path, level, start_line, end_line, text,
                content_hash
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                document_id,
                section.heading,
                section.heading_path.join("\n"),
                section.level,
                section.start_line,
                section.end_line,
                section.text,
                content_hash
            ],
        )?;
        let section_id = transaction.last_insert_rowid();
        transaction.execute(
            "INSERT INTO section_search (heading, text, document_id, section_id)
             VALUES (?1, ?2, ?3, ?4)",
            params![section.heading, section.text, document_id, section_id],
        )?;
    }
    for tag in &document.tags {
        transaction.execute(
            "INSERT INTO tags (document_id, tag) VALUES (?1, ?2)",
            params![document_id, tag],
        )?;
    }
    for task in &document.tasks {
        transaction.execute(
            "INSERT INTO tasks (document_id, text, completed, line_number)
             VALUES (?1, ?2, ?3, ?4)",
            params![document_id, task.text, task.completed, task.line_number],
        )?;
    }
    Ok(())
}

fn rebuild_links(
    transaction: &Transaction,
    documents: &[ParsedDocument],
    document_ids: &HashMap<String, i64>,
) -> Result<()> {
    let resolver = LinkResolver::new(documents);
    for document in documents {
        let document_id = document_ids[document.path.to_string_lossy().as_ref()];
        for link in &document.links {
            let resolved = resolver.resolve(&document.path, link);
            let target_id = resolved
                .target
                .as_ref()
                .and_then(|path| document_ids.get(path.to_string_lossy().as_ref()).copied());
            transaction.execute(
                "INSERT INTO links (
                    source_document_id, target_document_id, kind, raw_target,
                    target_heading, resolution, line_number
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    document_id,
                    target_id,
                    link.kind.as_str(),
                    link.raw_target,
                    resolved.target_heading,
                    resolved.resolution,
                    link.line_number
                ],
            )?;
        }
    }
    Ok(())
}
