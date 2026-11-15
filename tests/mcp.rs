mod common;

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use common::{basic_vault, obsidian_vault, vault_manifest};
use jabfa::{
    IndexConfig, RelatedLimits, SqlQueryLimits, build_context, get_document, get_section,
    index_status, index_vault, query_sql, related_documents, search_sections,
};
use serde_json::Value;
use tempfile::TempDir;

#[test]
fn rmcp_minimal_stdio_initialize_and_list_tools() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = basic_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    index_vault(&IndexConfig::new(&vault, &database)).expect("index MCP fixture");
    let mut server = McpChild::spawn(&vault, &database);

    let response = server.request(serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "jabfa-test", "version": "0.1.0"}
        }
    }));
    assert_eq!(response["jsonrpc"], "2.0");
    assert!(response["result"]["capabilities"]["tools"].is_object());
    assert_eq!(response["result"]["serverInfo"]["name"], "jabfa-mcp");
    server.notify(serde_json::json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    }));

    let response = server.request(serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    }));
    let tools = response["result"]["tools"]
        .as_array()
        .expect("MCP tool array");
    let names = tools
        .iter()
        .map(|tool| tool["name"].as_str().expect("tool name"))
        .collect::<std::collections::BTreeSet<_>>();
    for required in [
        "search",
        "context",
        "get_document",
        "get_section",
        "related",
    ] {
        assert!(names.contains(required), "missing MCP tool: {required}");
    }
}

#[test]
fn at_17_mcp_and_cli_return_equivalent_semantic_results() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = obsidian_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index MCP fixture");
    let before = vault_manifest(&vault);
    let mut server = McpChild::spawn(&vault, &database);
    server.request(serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "jabfa-test", "version": "0.1.0"}
        }
    }));
    server.notify(serde_json::json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    }));

    let expected = search_sections(&config, "Shared", 5).expect("library search");
    let expected = serde_json::to_value(expected).expect("search JSON value");
    let mcp = server.call_tool(
        2,
        "search",
        serde_json::json!({"query": "Shared", "limit": 5}),
    );
    assert_eq!(mcp["result"]["structuredContent"], expected);
    assert_eq!(mcp["result"]["isError"], false);
    let cli = cli_json(
        Command::new(env!("CARGO_BIN_EXE_jabfa"))
            .arg("search")
            .arg(&vault)
            .arg("Shared")
            .arg("--database")
            .arg(&database)
            .arg("--limit")
            .arg("5")
            .arg("--json"),
    );
    assert_eq!(cli, expected);

    let expected = build_context(&config, "Shared", 500).expect("library context");
    let expected = serde_json::to_value(expected).expect("context JSON value");
    let mcp = server.call_tool(
        3,
        "context",
        serde_json::json!({"query": "Shared", "max_tokens": 500}),
    );
    assert_eq!(mcp["result"]["structuredContent"], expected);
    let cli = cli_json(
        Command::new(env!("CARGO_BIN_EXE_jabfa"))
            .arg("context")
            .arg(&vault)
            .arg("Shared")
            .arg("--database")
            .arg(&database)
            .arg("--max-tokens")
            .arg("500")
            .arg("--json"),
    );
    assert_eq!(cli, expected);

    let expected = get_document(&config, Path::new("Home.md"), 4_096).expect("library document");
    let expected = serde_json::to_value(expected).expect("document JSON value");
    let mcp = server.call_tool(
        4,
        "get_document",
        serde_json::json!({"path": "Home.md", "max_bytes": 4096}),
    );
    assert_eq!(mcp["result"]["structuredContent"], expected);
    let cli = cli_json(
        Command::new(env!("CARGO_BIN_EXE_jabfa"))
            .arg("get")
            .arg(&vault)
            .arg("Home.md")
            .arg("--database")
            .arg(&database)
            .arg("--max-bytes")
            .arg("4096")
            .arg("--json"),
    );
    assert_eq!(cli, expected);

    let expected =
        get_section(&config, Path::new("Home.md"), 4, 11, 4_096).expect("library section");
    let expected = serde_json::to_value(expected).expect("section JSON value");
    let mcp = server.call_tool(
        5,
        "get_section",
        serde_json::json!({
            "path": "Home.md", "start_line": 4, "end_line": 11, "max_bytes": 4096
        }),
    );
    assert_eq!(mcp["result"]["structuredContent"], expected);
    let cli = cli_json(
        Command::new(env!("CARGO_BIN_EXE_jabfa"))
            .arg("get")
            .arg(&vault)
            .arg("Home.md")
            .arg("--database")
            .arg(&database)
            .arg("--start-line")
            .arg("4")
            .arg("--end-line")
            .arg("11")
            .arg("--max-bytes")
            .arg("4096")
            .arg("--json"),
    );
    assert_eq!(cli, expected);

    let limits = RelatedLimits {
        max_depth: 1,
        max_nodes: 10,
        max_bytes: 4_096,
    };
    let expected =
        related_documents(&config, Path::new("Home.md"), &limits).expect("library related");
    let expected = serde_json::to_value(expected).expect("related JSON value");
    let mcp = server.call_tool(
        6,
        "related",
        serde_json::json!({
            "path": "Home.md", "max_depth": 1, "max_nodes": 10, "max_bytes": 4096
        }),
    );
    assert_eq!(mcp["result"]["structuredContent"], expected);
    let cli = cli_json(
        Command::new(env!("CARGO_BIN_EXE_jabfa"))
            .arg("related")
            .arg(&vault)
            .arg("Home.md")
            .arg("--database")
            .arg(&database)
            .arg("--max-depth")
            .arg("1")
            .arg("--max-nodes")
            .arg("10")
            .arg("--max-bytes")
            .arg("4096")
            .arg("--json"),
    );
    assert_eq!(cli, expected);
    assert_eq!(vault_manifest(&vault), before, "MCP changed source vault");
}

#[test]
fn mcp_sql_and_status_match_shared_and_cli_results() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = basic_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index MCP SQL fixture");
    let before = vault_manifest(&vault);
    let mut server = McpChild::spawn(&vault, &database);
    server.request(serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "jabfa-test", "version": "0.1.0"}
        }
    }));
    server.notify(serde_json::json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    }));

    let sql = "SELECT path, title FROM documents ORDER BY path LIMIT 2";
    let limits = SqlQueryLimits {
        max_rows: 2,
        max_bytes: 4_096,
        timeout: Duration::from_secs(1),
    };
    let expected = query_sql(&config, sql, &limits).expect("library SQL");
    let expected = serde_json::to_value(expected).expect("SQL JSON value");
    let mcp = server.call_tool(
        2,
        "query_sql",
        serde_json::json!({
            "query": sql, "max_rows": 2, "max_bytes": 4096, "timeout_ms": 1000
        }),
    );
    assert_eq!(mcp["result"]["structuredContent"], expected);
    let cli = cli_json(
        Command::new(env!("CARGO_BIN_EXE_jabfa"))
            .arg("sql")
            .arg(&vault)
            .arg(sql)
            .arg("--database")
            .arg(&database)
            .arg("--max-rows")
            .arg("2")
            .arg("--max-bytes")
            .arg("4096")
            .arg("--timeout-ms")
            .arg("1000")
            .arg("--json"),
    );
    assert_eq!(cli, expected);

    let expected = index_status(&config).expect("library status");
    let expected = serde_json::to_value(expected).expect("status JSON value");
    let mcp = server.call_tool(3, "index_status", serde_json::json!({}));
    assert_eq!(mcp["result"]["structuredContent"], expected);
    let cli = cli_json(
        Command::new(env!("CARGO_BIN_EXE_jabfa"))
            .arg("status")
            .arg(&vault)
            .arg("--database")
            .arg(&database)
            .arg("--json"),
    );
    assert_eq!(cli, expected);
    assert_eq!(vault_manifest(&vault), before, "MCP changed source vault");
}

#[test]
fn at_18_mcp_rejects_invalid_and_oversized_requests_cleanly() {
    let temp = TempDir::new().expect("create temporary directory");
    let vault = obsidian_vault(&temp);
    let database = temp.path().join("index.sqlite3");
    let config = IndexConfig::new(&vault, &database);
    index_vault(&config).expect("index MCP validation fixture");
    let before = vault_manifest(&vault);
    let mut server = McpChild::spawn(&vault, &database);
    server.request(serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "jabfa-test", "version": "0.1.0"}
        }
    }));
    server.notify(serde_json::json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    }));

    let missing = server.call_tool(2, "search", serde_json::json!({"limit": 5}));
    assert_eq!(missing["error"]["code"], -32602);
    let unknown_field = server.call_tool(
        3,
        "search",
        serde_json::json!({"query": "Shared", "unexpected": true}),
    );
    assert_eq!(unknown_field["error"]["code"], -32602);
    let oversized = server.call_tool(
        4,
        "search",
        serde_json::json!({"query": "Shared", "limit": 101}),
    );
    assert_eq!(oversized["error"]["code"], -32602);
    let oversized_context = server.call_tool(
        5,
        "context",
        serde_json::json!({"query": "Shared", "max_tokens": 8193}),
    );
    assert_eq!(oversized_context["error"]["code"], -32602);
    let unknown = server.call_tool(6, "not_a_tool", serde_json::json!({}));
    assert_eq!(unknown["error"]["code"], -32601);

    let unsafe_sql = server.call_tool(
        7,
        "query_sql",
        serde_json::json!({"query": "DELETE FROM documents"}),
    );
    assert_eq!(unsafe_sql["result"]["isError"], true);
    assert!(
        unsafe_sql["result"]["structuredContent"]["message"]
            .as_str()
            .expect("unsafe SQL message")
            .contains("not allowed")
    );
    let excessive = server.call_tool(
        8,
        "query_sql",
        serde_json::json!({"query": "SELECT path FROM documents", "max_rows": 1}),
    );
    assert_eq!(excessive["result"]["isError"], true);
    assert!(
        excessive["result"]["structuredContent"]["message"]
            .as_str()
            .expect("row-limit message")
            .contains("row limit")
    );

    let alive = server.call_tool(9, "search", serde_json::json!({"query": "Shared"}));
    assert_eq!(alive["result"]["isError"], false);
    assert!(
        !alive["result"]["structuredContent"]["results"]
            .as_array()
            .expect("live search results")
            .is_empty()
    );
    let status = server.call_tool(10, "index_status", serde_json::json!({}));
    assert_eq!(status["result"]["isError"], false);
    assert_eq!(status["result"]["structuredContent"]["generation"], 1);
    assert_eq!(vault_manifest(&vault), before, "MCP changed source vault");
}

fn cli_json(command: &mut Command) -> Value {
    let output = command.output().expect("run CLI JSON command");
    assert!(
        output.status.success(),
        "CLI stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).expect("CLI JSON")
}

struct McpChild {
    child: Child,
    stdin: ChildStdin,
    responses: mpsc::Receiver<Value>,
}

impl McpChild {
    fn spawn(vault: &Path, database: &Path) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_jabfa-mcp"))
            .arg("--vault")
            .arg(vault)
            .arg("--database")
            .arg(database)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn MCP server");
        let stdin = child.stdin.take().expect("MCP stdin");
        let stdout = child.stdout.take().expect("MCP stdout");
        let (sender, receiver) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let line = line.expect("read MCP stdout");
                let value = serde_json::from_str(&line).expect("parse MCP JSON line");
                sender.send(value).expect("send MCP response");
            }
        });
        Self {
            child,
            stdin,
            responses: receiver,
        }
    }

    fn request(&mut self, message: Value) -> Value {
        let id = message["id"].as_i64().expect("numeric request ID");
        self.send(message);
        loop {
            let response = self
                .responses
                .recv_timeout(Duration::from_secs(5))
                .expect("MCP response");
            if response["id"].as_i64() == Some(id) {
                return response;
            }
        }
    }

    fn notify(&mut self, message: Value) {
        self.send(message);
    }

    fn call_tool(&mut self, id: i64, name: &str, arguments: Value) -> Value {
        self.request(serde_json::json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments}
        }))
    }

    fn send(&mut self, message: Value) {
        serde_json::to_writer(&mut self.stdin, &message).expect("serialize MCP message");
        self.stdin.write_all(b"\n").expect("write MCP newline");
        self.stdin.flush().expect("flush MCP message");
    }
}

impl Drop for McpChild {
    fn drop(&mut self) {
        if self.child.try_wait().expect("check MCP child").is_none() {
            self.child.kill().expect("kill MCP child");
            self.child.wait().expect("wait for MCP child");
        }
    }
}
