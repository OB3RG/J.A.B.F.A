# Local MCP Server

JABFA's MCP adapter uses the official `rmcp` 3.10 Rust SDK over **local stdio**. It opens no listening socket, provides no HTTP transport, and requires no runtime network access. The client launches one server process for a configured vault and derived database.

## Start and register

Build and index first:

```bash
cargo build --release
target/release/jabfa index /absolute/path/to/vault --database /tmp/jabfa-vault.sqlite3
```

Run directly:

```bash
target/release/jabfa-mcp --vault /absolute/path/to/vault --database /tmp/jabfa-vault.sqlite3
```

A typical MCP client server registration is:

```json
{
  "mcpServers": {
    "jabfa": {
      "command": "/absolute/path/to/jabfa/target/release/jabfa-mcp",
      "args": [
        "--vault", "/absolute/path/to/vault",
        "--database", "/tmp/jabfa-vault.sqlite3"
      ]
    }
  }
}
```

Configuration file placement is client-specific. Use absolute paths. The server does not automatically index or watch; run `jabfa index` after edits, or run `jabfa index --watch` in a separate process. MCP reads see committed generations and can use `index_status` or CLI `doctor` to assess freshness.

## Tools

Successful calls return the shared versioned response in `structuredContent` and the same JSON text in a content block. AT-17 verifies exact semantic equality with library and CLI responses. All tools are annotated read-only, non-destructive, idempotent, and closed-world.

| Tool | Required arguments | Optional arguments and bounds |
|---|---|---|
| `search` | `query` | `limit`: default 10, maximum 100; query maximum 16 KiB |
| `context` | `query` | `max_tokens`: default 2,000, maximum 8,192; query maximum 16 KiB |
| `get_document` | `path` | `max_bytes`: default/maximum 1 MiB |
| `get_section` | `path`, `start_line`, `end_line` | `max_bytes`: default/maximum 1 MiB; range must match an indexed section |
| `related` | `path` | `max_depth`: default 2/max 8; `max_nodes`: default 50/max 100; `max_bytes`: default/max 65,536 |
| `query_sql` | `query` | `max_rows`: default 100/max 1,000; `max_bytes`: default 65,536/max 1 MiB; `timeout_ms`: default 1,000/max 5,000 |
| `index_status` | None | None |

Unknown argument fields, missing/wrongly typed required fields, invalid line ranges, and values above these bounds produce JSON-RPC invalid-params errors (`-32602`). Unknown tools produce method-not-found (`-32601`).

A valid tool invocation whose operation fails—such as unsafe SQL, an excessive result set, a missing document, or a source-byte limit—returns `isError: true` with a structured `message`. AT-18 verifies that valid requests continue on the same connection after validation and operational errors.

## Security and dogfooding boundaries

- MCP exposes only the seven read-only operations above; there are no MCP write, indexing, shell, or arbitrary-file tools.
- SQL uses the shared read-only authorizer and row/byte/time limits. Attachments, pragmas, writes, schema changes, and file functions are denied.
- Source paths are vault-relative and canonicalized. Traversal and symlink escapes are rejected.
- The MCP client can read whatever the configured vault contains. Configure a dedicated vault and protect its path appropriately; do not register a vault containing secrets that the client should not receive.
- stdio stdout is reserved for protocol messages. Do not print logs or diagnostics to stdout in the server.
- Read-only dogfooding is appropriate now. Structured source writes and any replacement of the existing memory write workflow remain outside this boundary until M5 and a separate explicit migration.
