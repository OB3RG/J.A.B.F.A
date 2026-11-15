# JABFA

**Just A Brain For AI**

JABFA is a local-first Rust knowledge engine for Obsidian vaults and ordinary folders of structured Markdown. It compiles human-readable notes into a disposable SQLite index, then exposes fast, token-bounded retrieval through both a CLI and the Model Context Protocol (MCP).

The core idea is simple:

> Markdown is the source of truth. Rust handles indexing and retrieval. The LLM receives only the evidence it needs.

## Why

LLM agents often waste time and tokens walking directories, rereading catalogs, and loading complete notes just to find a few relevant passages. JABFA moves that work into deterministic local software.

- **Markdown-native:** keep Obsidian compatibility, wikilinks, frontmatter, Git history, and direct human access.
- **Queryable:** index documents, heading-level sections, metadata, tags, links, and tasks in SQLite.
- **Agent-efficient:** return ranked sections or a bounded evidence bundle instead of a vault scan.
- **Dual interface:** use the same query engine from a scriptable CLI and an MCP server.
- **Local-first:** no cloud service or network access is required for core indexing and search.

## Interfaces

The CLI supports indexing, status, ranked section search, token-bounded context, and constrained read-only SQL with an explicit database path:

```bash
jabfa index ~/Documents/my-vault --database /tmp/my-vault.sqlite3
jabfa index ~/Documents/my-vault --database /tmp/my-vault.sqlite3 --full
jabfa index ~/Documents/my-vault --database /tmp/my-vault.sqlite3 --watch --debounce-ms 100
jabfa status ~/Documents/my-vault --database /tmp/my-vault.sqlite3 --json
jabfa search ~/Documents/my-vault "PETG adhesion" --database /tmp/my-vault.sqlite3 --limit 5 --json
jabfa context ~/Documents/my-vault "Why were the brain agents slow?" --database /tmp/my-vault.sqlite3 --max-tokens 2000
jabfa sql ~/Documents/my-vault "SELECT path, title FROM documents ORDER BY path LIMIT 10" --database /tmp/my-vault.sqlite3 --max-rows 10 --max-bytes 65536 --timeout-ms 1000 --json
jabfa get ~/Documents/my-vault projects/PETG.md --database /tmp/my-vault.sqlite3
jabfa get ~/Documents/my-vault projects/PETG.md --database /tmp/my-vault.sqlite3 --start-line 20 --end-line 35 --json
jabfa links ~/Documents/my-vault projects/PETG.md --database /tmp/my-vault.sqlite3 --json
jabfa related ~/Documents/my-vault projects/PETG.md --database /tmp/my-vault.sqlite3 --max-depth 2 --max-nodes 50 --json
```

Human context output is the Markdown evidence payload; `--json` returns the evidence together with budget, deduplication, omission, and truncation counts. SQL output is tab-separated in human mode and typed JSON rows in JSON mode. Every successful JSON object contains `output_schema_version: 1`; see the [CLI contract](docs/CLI.md) for frozen fields, diagnostics, and exit codes. MCP remains planned:

```bash
jabfa-mcp --vault ~/Documents/my-vault
```

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

**M1 and M2 complete; M3 incremental indexing is in progress.** The library discovers Markdown while excluding `.git`, `.obsidian`, and `.jabfa`; parses typed YAML frontmatter, heading-level sections, tags, tasks, Markdown links, wikilinks, aliases, and embeds; resolves unambiguous Obsidian targets; and persists relational rows plus FTS5 search data transactionally in SQLite. Full rebuilds have explicit generations, and malformed notes produce per-document diagnostics.

The `jabfa index`, `jabfa status`, `jabfa search`, `jabfa context`, and `jabfa sql` commands work in human and JSON modes. Runtime and usage exit codes are tested; search/context/SQL JSON is tested for semantic equality with the shared library. `search_sections` returns ranked, cited sections with BLAKE3 content hashes. `build_context` deduplicates evidence, reports omissions, truncates oversized sections, and enforces a citation-inclusive evidence budget. `query_sql` rejects mutation, schema changes, pragmas, attachments, and file functions while enforcing row, byte, and execution-time limits.

AT-01 through AT-15 plus eight CLI tests pass. No acceptance assertions or expected values were weakened during implementation.

The project pins Rust **1.100.0** in `rust-toolchain.toml` and uses Rust edition 2024. If rustup is not on your shell's `PATH`, source `~/.cargo/env` before running Cargo.

```bash
cargo test --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

## Documentation

- [Product brief](docs/PRODUCT_BRIEF.md) — users, scope, requirements, architecture, and success criteria.
- [Delivery plan](docs/PLAN.md) — milestones, vertical slices, and exit gates.
- [CLI contract](docs/CLI.md) — versioned JSON fields, streams, diagnostics, and exit codes.
- [Test plan](docs/TEST_PLAN.md) — acceptance-test contracts and quality gates.
- [Database schema](docs/DATABASE.md) — implemented SQLite schema v1 and query examples.
- [Retrieval baseline](docs/BENCHMARK.md) — repeatable fixture, citation coverage, size, and latency measurements.
- [TODO](TODO.md) — current actionable work queue.
- [Agent instructions](AGENTS.md) — mandatory engineering and TDD workflow for coding agents.

## Name

**JABFA** expands to **Just A Brain For AI**. The project and binary use lowercase `jabfa`; the MCP binary uses `jabfa-mcp`.
