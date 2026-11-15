use std::time::{Duration, Instant};

use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::limits::Limit;
use rusqlite::types::ValueRef;
use rusqlite::{Connection, Error, ErrorCode, OpenFlags};
use serde::Serialize;
use serde_json::Value;

use crate::OUTPUT_SCHEMA_VERSION;
use crate::error::{JabfaError, Result};
use crate::index::IndexConfig;

const MAX_ROWS: usize = 1_000;
const MAX_BYTES: usize = 1_048_576;
const MAX_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_SQL_LENGTH: i32 = 16_384;
const PROGRESS_OPERATIONS: i32 = 100;

/// Caller-provided limits for one read-only SQL query.
///
/// Requests are additionally capped at 1,000 rows, 1 MiB of JSON-encoded row
/// data, and five seconds. The byte limit covers `SqlResponse.rows`, not the
/// transport envelope or column names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SqlQueryLimits {
    pub max_rows: usize,
    pub max_bytes: usize,
    pub timeout: Duration,
}

/// A bounded SQL result with positional rows corresponding to `columns`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SqlResponse {
    pub output_schema_version: u32,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<Value>>,
    pub bytes: usize,
}

/// Execute one read-only SQL statement against the derived index.
///
/// SQLite's authorizer permits only reads, SELECT evaluation, recursive CTEs,
/// and non-file functions. Writes, schema changes, pragmas, transactions, and
/// attachments are denied. The connection itself is also read-only and has
/// `query_only` enabled as defense in depth.
pub fn query_sql(config: &IndexConfig, sql: &str, limits: &SqlQueryLimits) -> Result<SqlResponse> {
    if !config.database_path.is_file() {
        return Err(JabfaError::DatabaseNotFound(config.database_path.clone()));
    }

    let connection = Connection::open_with_flags(
        &config.database_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    connection.execute_batch("PRAGMA query_only = ON;")?;
    connection.set_limit(Limit::SQLITE_LIMIT_ATTACHED, 0)?;
    connection.set_limit(Limit::SQLITE_LIMIT_LENGTH, MAX_BYTES as i32)?;
    connection.set_limit(Limit::SQLITE_LIMIT_SQL_LENGTH, MAX_SQL_LENGTH)?;

    let max_rows = limits.max_rows.min(MAX_ROWS);
    let max_bytes = limits.max_bytes.min(MAX_BYTES);
    let timeout = limits.timeout.min(MAX_TIMEOUT);
    let started = Instant::now();
    connection.progress_handler(
        PROGRESS_OPERATIONS,
        Some(move || started.elapsed() >= timeout),
    );
    connection.authorizer(Some(|context: AuthContext| match context.action {
        AuthAction::Select | AuthAction::Read { .. } | AuthAction::Recursive => {
            Authorization::Allow
        }
        AuthAction::Function { function_name } if is_safe_function(function_name) => {
            Authorization::Allow
        }
        _ => Authorization::Deny,
    }));

    let mut statement = connection.prepare(sql).map_err(map_sql_error)?;
    if !statement.readonly() || statement.column_count() == 0 {
        return Err(JabfaError::UnsafeSql);
    }

    let columns = statement
        .column_names()
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    let column_count = statement.column_count();
    let mut rows: Vec<Vec<Value>> = Vec::new();
    let mut bytes = serialized_rows_size(&rows)?;
    if bytes > max_bytes {
        return Err(JabfaError::SqlByteLimitExceeded { max_bytes });
    }

    {
        let mut cursor = statement.query([]).map_err(map_sql_error)?;
        while let Some(row) = cursor.next().map_err(map_sql_error)? {
            if rows.len() >= max_rows {
                return Err(JabfaError::SqlRowLimitExceeded { max_rows });
            }

            let mut values = Vec::with_capacity(column_count);
            for index in 0..column_count {
                values.push(json_value(row.get_ref(index)?)?);
            }
            rows.push(values);
            bytes = serialized_rows_size(&rows)?;
            if bytes > max_bytes {
                return Err(JabfaError::SqlByteLimitExceeded { max_bytes });
            }
        }
    }

    Ok(SqlResponse {
        output_schema_version: OUTPUT_SCHEMA_VERSION,
        columns,
        rows,
        bytes,
    })
}

fn is_safe_function(function_name: &str) -> bool {
    !matches!(
        function_name.to_ascii_lowercase().as_str(),
        "load_extension" | "readfile" | "writefile"
    )
}

fn json_value(value: ValueRef<'_>) -> rusqlite::Result<Value> {
    Ok(match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(value) => Value::from(value),
        ValueRef::Real(value) => {
            serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
        }
        ValueRef::Text(_) => Value::String(value.as_str()?.to_owned()),
        ValueRef::Blob(value) => Value::String(hex_encode(value)),
    })
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(2 + bytes.len().saturating_mul(2));
    encoded.push_str("0x");
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

fn serialized_rows_size(rows: &[Vec<Value>]) -> Result<usize> {
    Ok(serde_json::to_vec(rows)?.len())
}

fn map_sql_error(error: Error) -> JabfaError {
    match error {
        Error::MultipleStatement => JabfaError::UnsafeSql,
        Error::SqliteFailure(error, _)
            if matches!(
                error.code,
                ErrorCode::AuthorizationForStatementDenied | ErrorCode::ReadOnly
            ) =>
        {
            JabfaError::UnsafeSql
        }
        Error::SqliteFailure(error, _) if error.code == ErrorCode::OperationInterrupted => {
            JabfaError::SqlQueryTimeout
        }
        error => JabfaError::Database(error),
    }
}
