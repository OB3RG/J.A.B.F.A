use std::path::PathBuf;
use std::time::Duration;

use rmcp::model::{
    CallToolRequestMethod, CallToolRequestParams, CallToolResponse, CallToolResult, Implementation,
    JsonObject, ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{
    IndexConfig, RelatedLimits, Result, SqlQueryLimits, build_context, build_context_for_paths,
    get_document, get_section, index_status, query_sql, related_documents, search_sections,
    search_subjects,
};

/// JABFA's local MCP server adapter.
#[derive(Debug, Clone)]
pub struct JabfaMcp {
    config: IndexConfig,
}

impl JabfaMcp {
    pub fn new(config: IndexConfig) -> Self {
        Self { config }
    }
}

impl ServerHandler for JabfaMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("jabfa-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions("Local read-only retrieval over a JABFA Markdown index")
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, McpError> {
        Ok(ListToolsResult {
            tools: tool_definitions(),
            ..Default::default()
        })
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        tool_definitions()
            .into_iter()
            .find(|tool| tool.name == name)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResponse, McpError> {
        let arguments = Value::Object(request.arguments.unwrap_or_default());
        match request.name.as_ref() {
            "subjects" => {
                let arguments: SearchArguments = parse_arguments(arguments)?;
                validate_query(&arguments.query)?;
                let limit = arguments.limit.unwrap_or(10);
                validate_max(limit, 100, "limit")?;
                structured_result(search_subjects(&self.config, &arguments.query, limit))
            }
            "search" => {
                let arguments: SearchArguments = parse_arguments(arguments)?;
                validate_query(&arguments.query)?;
                let limit = arguments.limit.unwrap_or(10);
                validate_max(limit, 100, "limit")?;
                structured_result(search_sections(&self.config, &arguments.query, limit))
            }
            "investigate" => {
                let arguments: InvestigateArguments = parse_arguments(arguments)?;
                validate_query(&arguments.query)?;
                let max_tokens = arguments.max_tokens.unwrap_or(1_200);
                validate_max(max_tokens, 8_192, "max_tokens")?;
                validate_selected_paths(&arguments.paths)?;
                let paths = arguments
                    .paths
                    .iter()
                    .map(PathBuf::as_path)
                    .collect::<Vec<_>>();
                structured_result(build_context_for_paths(
                    &self.config,
                    &arguments.query,
                    &paths,
                    max_tokens,
                ))
            }
            "context" => {
                let arguments: ContextArguments = parse_arguments(arguments)?;
                validate_query(&arguments.query)?;
                let max_tokens = arguments.max_tokens.unwrap_or(2_000);
                validate_max(max_tokens, 8_192, "max_tokens")?;
                if let Some(paths) = arguments.paths {
                    validate_selected_paths(&paths)?;
                    let paths = paths.iter().map(PathBuf::as_path).collect::<Vec<_>>();
                    structured_result(build_context_for_paths(
                        &self.config,
                        &arguments.query,
                        &paths,
                        max_tokens,
                    ))
                } else {
                    structured_result(build_context(&self.config, &arguments.query, max_tokens))
                }
            }
            "get_document" => {
                let arguments: GetDocumentArguments = parse_arguments(arguments)?;
                let max_bytes = arguments.max_bytes.unwrap_or(1_048_576);
                validate_max(max_bytes, 1_048_576, "max_bytes")?;
                structured_result(get_document(&self.config, &arguments.path, max_bytes))
            }
            "get_section" => {
                let arguments: GetSectionArguments = parse_arguments(arguments)?;
                if arguments.start_line == 0 || arguments.end_line < arguments.start_line {
                    return Err(McpError::invalid_params("invalid source line range", None));
                }
                let max_bytes = arguments.max_bytes.unwrap_or(1_048_576);
                validate_max(max_bytes, 1_048_576, "max_bytes")?;
                structured_result(get_section(
                    &self.config,
                    &arguments.path,
                    arguments.start_line,
                    arguments.end_line,
                    max_bytes,
                ))
            }
            "related" => {
                let arguments: RelatedArguments = parse_arguments(arguments)?;
                let limits = RelatedLimits {
                    max_depth: arguments.max_depth.unwrap_or(2),
                    max_nodes: arguments.max_nodes.unwrap_or(50),
                    max_bytes: arguments.max_bytes.unwrap_or(65_536),
                };
                validate_max(limits.max_depth, 8, "max_depth")?;
                validate_max(limits.max_nodes, 100, "max_nodes")?;
                validate_max(limits.max_bytes, 65_536, "max_bytes")?;
                structured_result(related_documents(&self.config, &arguments.path, &limits))
            }
            "query_sql" => {
                let arguments: QuerySqlArguments = parse_arguments(arguments)?;
                let timeout_ms = arguments.timeout_ms.unwrap_or(1_000);
                let limits = SqlQueryLimits {
                    max_rows: arguments.max_rows.unwrap_or(100),
                    max_bytes: arguments.max_bytes.unwrap_or(65_536),
                    timeout: Duration::from_millis(timeout_ms),
                };
                validate_max(limits.max_rows, 1_000, "max_rows")?;
                validate_max(limits.max_bytes, 1_048_576, "max_bytes")?;
                validate_max(timeout_ms, 5_000, "timeout_ms")?;
                structured_result(query_sql(&self.config, &arguments.query, &limits))
            }
            "index_status" => {
                let _: EmptyArguments = parse_arguments(arguments)?;
                structured_result(index_status(&self.config))
            }
            _ => Err(McpError::method_not_found::<CallToolRequestMethod>()),
        }
    }
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchArguments {
    query: String,
    limit: Option<usize>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct InvestigateArguments {
    query: String,
    paths: Vec<PathBuf>,
    max_tokens: Option<usize>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextArguments {
    query: String,
    max_tokens: Option<usize>,
    paths: Option<Vec<PathBuf>>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct GetDocumentArguments {
    path: PathBuf,
    max_bytes: Option<usize>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct GetSectionArguments {
    path: PathBuf,
    start_line: usize,
    end_line: usize,
    max_bytes: Option<usize>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct QuerySqlArguments {
    query: String,
    max_rows: Option<usize>,
    max_bytes: Option<usize>,
    timeout_ms: Option<u64>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyArguments {}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct RelatedArguments {
    path: PathBuf,
    max_depth: Option<usize>,
    max_nodes: Option<usize>,
    max_bytes: Option<usize>,
}

fn parse_arguments<T: DeserializeOwned>(arguments: Value) -> std::result::Result<T, McpError> {
    serde_json::from_value(arguments)
        .map_err(|error| McpError::invalid_params(error.to_string(), None))
}

fn validate_query(query: &str) -> std::result::Result<(), McpError> {
    validate_max(query.len(), 16_384, "query bytes")
}

fn validate_selected_paths(paths: &[PathBuf]) -> std::result::Result<(), McpError> {
    validate_max(paths.len(), 20, "paths")?;
    for path in paths {
        validate_max(path.to_string_lossy().len(), 4_096, "path bytes")?;
    }
    Ok(())
}

fn validate_max<T>(value: T, maximum: T, name: &str) -> std::result::Result<(), McpError>
where
    T: PartialOrd + std::fmt::Display,
{
    if value > maximum {
        return Err(McpError::invalid_params(
            format!("{name} exceeds maximum {maximum}"),
            None,
        ));
    }
    Ok(())
}

fn structured_result<T: Serialize>(
    result: Result<T>,
) -> std::result::Result<CallToolResponse, McpError> {
    match result {
        Ok(response) => {
            let value = serde_json::to_value(response).map_err(|error| {
                McpError::internal_error(format!("serialize tool response: {error}"), None)
            })?;
            Ok(CallToolResult::structured(value).into())
        }
        Err(error) => Ok(CallToolResult::structured_error(serde_json::json!({
            "message": error.to_string(),
        }))
        .into()),
    }
}

fn tool_definitions() -> Vec<Tool> {
    vec![
        tool(
            "query_sql",
            "Execute one bounded read-only SELECT against the derived index",
            object_schema(
                serde_json::json!({
                    "query": {"type": "string"},
                    "max_rows": {"type": "integer", "minimum": 0, "maximum": 1000, "default": 100},
                    "max_bytes": {"type": "integer", "minimum": 0, "maximum": 1048576, "default": 65536},
                    "timeout_ms": {"type": "integer", "minimum": 0, "maximum": 5000, "default": 1000}
                }),
                &["query"],
            ),
        ),
        tool(
            "index_status",
            "Return the committed index generation and document counts",
            object_schema(serde_json::json!({}), &[]),
        ),
        tool(
            "subjects",
            "Discover unique subjects with best-section citations",
            object_schema(
                serde_json::json!({
                    "query": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 0, "maximum": 100, "default": 10}
                }),
                &["query"],
            ),
        ),
        tool(
            "search",
            "Search ranked heading-level sections with citations",
            object_schema(
                serde_json::json!({
                    "query": {"type": "string"},
                    "limit": {"type": "integer", "minimum": 0, "maximum": 100, "default": 10}
                }),
                &["query"],
            ),
        ),
        tool(
            "investigate",
            "Build token-bounded cited evidence from required selected subject paths",
            object_schema(
                serde_json::json!({
                    "query": {"type": "string"},
                    "paths": {"type": "array", "items": {"type": "string", "maxLength": 4096}, "maxItems": 20},
                    "max_tokens": {"type": "integer", "minimum": 0, "maximum": 8192, "default": 1200}
                }),
                &["query", "paths"],
            ),
        ),
        tool(
            "context",
            "Build deduplicated token-bounded cited evidence",
            object_schema(
                serde_json::json!({
                    "query": {"type": "string"},
                    "max_tokens": {"type": "integer", "minimum": 0, "maximum": 8192, "default": 2000},
                    "paths": {"type": "array", "items": {"type": "string", "maxLength": 4096}, "maxItems": 20}
                }),
                &["query"],
            ),
        ),
        tool(
            "get_document",
            "Read an exact indexed Markdown document",
            object_schema(
                serde_json::json!({
                    "path": {"type": "string"},
                    "max_bytes": {"type": "integer", "minimum": 0, "maximum": 1048576, "default": 1048576}
                }),
                &["path"],
            ),
        ),
        tool(
            "get_section",
            "Read exact source lines for an indexed section citation",
            object_schema(
                serde_json::json!({
                    "path": {"type": "string"},
                    "start_line": {"type": "integer", "minimum": 1},
                    "end_line": {"type": "integer", "minimum": 1},
                    "max_bytes": {"type": "integer", "minimum": 0, "maximum": 1048576, "default": 1048576}
                }),
                &["path", "start_line", "end_line"],
            ),
        ),
        tool(
            "related",
            "Traverse resolved incoming and outgoing links within bounds",
            object_schema(
                serde_json::json!({
                    "path": {"type": "string"},
                    "max_depth": {"type": "integer", "minimum": 0, "maximum": 8, "default": 2},
                    "max_nodes": {"type": "integer", "minimum": 0, "maximum": 100, "default": 50},
                    "max_bytes": {"type": "integer", "minimum": 0, "maximum": 65536, "default": 65536}
                }),
                &["path"],
            ),
        ),
    ]
}

fn tool(name: &'static str, description: &'static str, schema: JsonObject) -> Tool {
    Tool::new(name, description, schema).with_annotations(
        ToolAnnotations::new()
            .read_only(true)
            .destructive(false)
            .idempotent(true)
            .open_world(false),
    )
}

fn object_schema(properties: Value, required: &[&str]) -> JsonObject {
    match serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    }) {
        Value::Object(schema) => schema,
        _ => serde_json::Map::new(),
    }
}
