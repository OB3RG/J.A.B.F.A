# JABFA

**Just A Brain For AI**

JABFA is a local-first Rust knowledge engine for Obsidian vaults and ordinary folders of structured Markdown. It compiles human-readable notes into a rebuildable SQLite index, then exposes fast, token-bounded retrieval through a CLI and the Model Context Protocol (MCP).

The core idea is simple:

> Markdown is the source of truth. Rust handles indexing and retrieval. The LLM receives only the evidence it needs.

## Why use JABFA?

LLM agents often waste time and tokens walking directories, rereading catalogs, and loading complete notes just to find a few relevant passages. JABFA moves that work into deterministic local software.

- **Markdown-native:** keep Obsidian compatibility, wikilinks, frontmatter, Git history, and direct human access.
- **Queryable:** index documents, heading-level sections, metadata, tags, links, and tasks in SQLite.
- **Agent-efficient:** find relevant notes by subject, then retrieve selected passages or a bounded evidence bundle instead of scanning the vault.
- **Dual interface:** use the same query engine from a scriptable CLI and an MCP server.
- **Local-first:** no cloud service or network access is required for core indexing and search.

## Build from source

JABFA pins Rust **1.100.0** in `rust-toolchain.toml` and uses Rust edition 2024. With rustup installed, build both binaries from the repository root and add them to the current shell's `PATH`:

```bash
cargo build --release
export PATH="$PWD/target/release:$PATH"
```

The examples below assume this setup. If rustup is not on your shell's `PATH`, source `~/.cargo/env` before running Cargo.

## CLI

Every command takes a vault path and an explicit database path. Build the index before querying it.

### Index and check a vault

```bash
jabfa index ~/Documents/my-vault --database /tmp/my-vault.sqlite3
jabfa status ~/Documents/my-vault --database /tmp/my-vault.sqlite3 --json
```

Indexing is incremental by default. Request a full rebuild, watch for changes, or check index health explicitly:

```bash
jabfa index ~/Documents/my-vault --database /tmp/my-vault.sqlite3 --full
jabfa index ~/Documents/my-vault --database /tmp/my-vault.sqlite3 --watch --debounce-ms 100
jabfa doctor ~/Documents/my-vault --database /tmp/my-vault.sqlite3 --json
```

### Discover and retrieve notes

`subjects` groups ranked section matches by document so you can choose which notes to investigate. Use `search` when you need individual ranked sections instead:

```bash
jabfa subjects ~/Documents/my-vault "PETG adhesion" --database /tmp/my-vault.sqlite3 --limit 5 --json
jabfa search ~/Documents/my-vault "PETG adhesion" --database /tmp/my-vault.sqlite3 --limit 5 --json
```

`context` returns deduplicated, token-bounded evidence. Search the whole vault, or repeat `--path` to restrict retrieval to at most 20 selected documents:

```bash
jabfa context ~/Documents/my-vault "Why were the brain agents slow?" --database /tmp/my-vault.sqlite3 --max-tokens 2000
jabfa context ~/Documents/my-vault "PETG adhesion" --database /tmp/my-vault.sqlite3 --path projects/PETG.md --max-tokens 1200 --json
```

By default, `context` writes a Markdown evidence payload. With `--json`, it returns the evidence together with budget, deduplication, omission, and truncation counts. Selected context can compose up to four complementary bold-milestone passages, prioritizing the latest relevant milestone and distinct query facets without exceeding the budget. MCP `investigate` exposes the same behavior with required paths and a 1,200-token default.

### Inspect documents and links

Read an exact document or source range, list its outgoing links, or traverse related notes within explicit depth and result limits:

```bash
jabfa get ~/Documents/my-vault projects/PETG.md --database /tmp/my-vault.sqlite3
jabfa get ~/Documents/my-vault projects/PETG.md --database /tmp/my-vault.sqlite3 --start-line 20 --end-line 35 --json
jabfa links ~/Documents/my-vault projects/PETG.md --database /tmp/my-vault.sqlite3 --json
jabfa related ~/Documents/my-vault projects/PETG.md --database /tmp/my-vault.sqlite3 --max-depth 2 --max-nodes 50 --json
```

### Query SQLite directly

`sql` executes one bounded, read-only `SELECT` against the derived index:

```bash
jabfa sql ~/Documents/my-vault "SELECT path, title FROM documents ORDER BY path LIMIT 10" --database /tmp/my-vault.sqlite3 --max-rows 10 --max-bytes 65536 --timeout-ms 1000 --json
```

SQL output is tab-separated in human mode and typed JSON rows in JSON mode. Every successful JSON object contains `output_schema_version: 1`; see the [CLI contract](docs/CLI.md) for frozen fields, diagnostics, and exit codes.

## MCP server

The local MCP server communicates over standard input/output (stdio) and opens no network socket:

```bash
jabfa-mcp --vault ~/Documents/my-vault --database /tmp/my-vault.sqlite3
```

The server is read-only and does not index automatically. Run `jabfa index` after edits, or run `jabfa index --watch` in a separate process. See [MCP registration and security](docs/MCP.md) for client setup and tool limits.

## Structured writes

Write operations modify source Markdown and require the literal `--allow-write` flag. They are available only through the CLI; MCP remains read-only.

Create a schema-valid learning note for an `llm-brain` vault and update its registry and log:

```bash
jabfa brain-create-learning ~/Documents/my-vault \
  wiki/learnings/2026-11-15-example.md \
  --database /tmp/my-vault.sqlite3 \
  --title "Example" \
  --finding "Evidence-backed learning" \
  --context "Dogfood canary" \
  --consequences "Use structured writes" \
  --origin "Example session" \
  --created 2026-11-15 \
  --tag testing \
  --allow-write \
  --json
```

Create a generic learning note:

```bash
jabfa create-learning ~/Documents/my-vault \
  learnings/example.md \
  --database /tmp/my-vault.sqlite3 \
  --title "Example" \
  --body "Evidence-backed learning" \
  --tag testing \
  --allow-write \
  --json
```

Replace an exact indexed section after setting `CITATION_HASH` to its current citation hash:

```bash
CITATION_HASH="replace-with-current-citation-hash"

jabfa patch-section ~/Documents/my-vault \
  learnings/example.md \
  --database /tmp/my-vault.sqlite3 \
  --start-line 7 \
  --end-line 9 \
  --expected-hash "$CITATION_HASH" \
  --replacement $'# Example\n\nUpdated body.\n' \
  --allow-write \
  --json
```

See [Structured write design](docs/WRITES.md) for validation, recovery, and real-vault safety boundaries.

## Design principles

1. Never require conversion away from Markdown.
2. Treat the index as a derived, rebuildable artifact.
3. Make read-only operations leave the source vault byte-for-byte unchanged.
4. Prefer section-level retrieval over whole-document context.
5. Start with SQLite FTS5; add embeddings only when measured retrieval gaps justify them.
6. Keep CLI and MCP behavior equivalent through a shared Rust library.
7. Use stable, idiomatic, borrow-checker-friendly Rust rather than clever lifetime-heavy designs.
8. Develop test-first. Acceptance tests are contracts, not implementation details to rewrite when code disagrees.

## Project status

**Milestones M1 through M5 are complete. Work on M6 extensions requires measured evidence of need.** JABFA supports incremental indexing, explicit SQLite generations, ranked section and subject retrieval, token-bounded context, bounded link traversal, and constrained read-only SQL. The parser handles typed YAML frontmatter, heading-level sections, tags, tasks, Markdown links, wikilinks, aliases, embeds, and unambiguous Obsidian targets. Indexing excludes `.git`, `.obsidian`, and `.jabfa`; malformed notes produce per-document diagnostics.

The CLI and MCP share the same query library, and their JSON responses are tested for semantic equality with that library. The context pipeline deduplicates evidence, reports omissions, truncates oversized sections, and enforces a citation-inclusive evidence budget. SQL rejects mutation, schema changes, pragmas, attachments, and file functions while enforcing row, byte, and execution-time limits.

The current suite passes 55 tests, including acceptance tests AT-01 through AT-24 and six MCP tests. Additional regressions cover the CLI, writes, long sections, frontmatter, literal queries, punctuation/backslash paths, passage priority, and `llm-brain` writes. The structured-write canary passed against the real `llm-brain` vault; the `brain-scribe` agent now uses `brain-create-learning` for new notes. Real-vault writes still require separate explicit approval; MCP remains read-only.

## Development

Run the complete quality gate:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo test --doc
```

## Documentation

- [Product brief](docs/PRODUCT_BRIEF.md) — users, scope, requirements, architecture, and success criteria.
- [Delivery plan](docs/PLAN.md) — milestones, vertical slices, and exit gates.
- [CLI contract](docs/CLI.md) — versioned JSON fields, streams, diagnostics, and exit codes.
- [Test plan](docs/TEST_PLAN.md) — acceptance-test contracts and quality gates.
- [Database schema](docs/DATABASE.md) — implemented SQLite schema v1 and query examples.
- [Retrieval baseline](docs/BENCHMARK.md) — repeatable fixture, citation coverage, size, and latency measurements.
- [Index operations](docs/OPERATIONS.md) — generations, watch behavior, doctor, and recovery.
- [MCP server](docs/MCP.md) — local stdio registration, tools, limits, and read-only dogfooding.
- [Structured write design](docs/WRITES.md) — M5 validation, source-first transactions, and recovery.
- [llm-brain migration](docs/MIGRATION.md) — staged read-only dogfooding, write rehearsal, canary, and rollback.
- [CI](docs/CI.md) — Forgejo Actions quality gate.
- [TODO](TODO.md) — current actionable work queue.
- [Agent instructions](AGENTS.md) — mandatory engineering and TDD workflow for coding agents.

## License

JABFA is available under the [MIT License](LICENSE).
