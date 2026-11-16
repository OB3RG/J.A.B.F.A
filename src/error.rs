use std::path::PathBuf;

use thiserror::Error;

pub type Result<T> = std::result::Result<T, JabfaError>;

#[derive(Debug, Error)]
pub enum JabfaError {
    #[error("vault root is not a directory: {0}")]
    InvalidVaultRoot(PathBuf),

    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid YAML frontmatter in {path}: {source}")]
    Frontmatter {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },

    #[error("index database does not exist: {0}")]
    DatabaseNotFound(PathBuf),

    #[error("invalid index metadata for {key}: {value}")]
    InvalidIndexMetadata { key: String, value: String },

    #[error("path is outside the configured vault: {0}")]
    PathOutsideVault(PathBuf),

    #[error("document is not present in the index: {0}")]
    DocumentNotFound(PathBuf),

    #[error("subject selection exceeded the path limit of {max_paths}")]
    SubjectSelectionLimitExceeded { max_paths: usize },

    #[error("section is not present in the index: {path} lines {start_line}-{end_line}")]
    SectionNotFound {
        path: PathBuf,
        start_line: usize,
        end_line: usize,
    },

    #[error("invalid source line range: {start}-{end}")]
    InvalidSourceRange { start: usize, end: usize },

    #[error("source content exceeded the byte limit of {max_bytes}")]
    SourceByteLimitExceeded { max_bytes: usize },

    #[error("link query exceeded the result limit of {max_links}")]
    LinkResultLimitExceeded { max_links: usize },

    #[error("related traversal exceeded the node limit of {max_nodes}")]
    GraphNodeLimitExceeded { max_nodes: usize },

    #[error("related traversal exceeded the byte limit of {max_bytes}")]
    GraphByteLimitExceeded { max_bytes: usize },

    #[error(
        "SQL statement is not allowed; use one read-only SELECT without pragmas, attachments, or file functions"
    )]
    UnsafeSql,

    #[error("SQL query exceeded the row limit of {max_rows}")]
    SqlRowLimitExceeded { max_rows: usize },

    #[error("SQL query exceeded the result-byte limit of {max_bytes}")]
    SqlByteLimitExceeded { max_bytes: usize },

    #[error("SQL query exceeded its execution-time limit")]
    SqlQueryTimeout,

    #[error("write path is in immutable raw storage: {0}")]
    ImmutableRawPath(PathBuf),

    #[error("invalid structured write input: {0}")]
    InvalidWriteInput(String),

    #[error("write destination already exists: {0}")]
    DestinationExists(PathBuf),

    #[error("stale section citation for {path} lines {start_line}-{end_line}")]
    StaleCitation {
        path: PathBuf,
        start_line: usize,
        end_line: usize,
    },

    #[error(
        "source committed at {path} with hash {source_hash}, but indexing requires recovery: {message}"
    )]
    WriteRecoveryRequired {
        path: PathBuf,
        source_hash: String,
        message: String,
    },

    #[error("filesystem watcher error: {0}")]
    Watch(String),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("SQLite error: {0}")]
    Database(#[from] rusqlite::Error),
}

pub(crate) fn io_error(path: impl Into<PathBuf>, source: std::io::Error) -> JabfaError {
    JabfaError::Io {
        path: path.into(),
        source,
    }
}
