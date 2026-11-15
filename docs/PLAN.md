# JABFA Delivery Plan

## Status

**M1 and M2 complete; M3 incremental indexing in progress.** AT-01 through AT-15, two M1 CLI tests, four M2 CLI tests, and two M3 CLI tests pass with formatting, Clippy, and documentation tests. The `index`/`status` CLI supports human and JSON output with tested error codes. `search` and `context` expose the shared ranked-retrieval and bounded-context library operations through human and JSON CLI output. `sql` uses SQLite authorization, read-only connections, and row/byte/time limits in both library and CLI forms. All successful JSON responses now have frozen initial fields and `output_schema_version: 1`; diagnostics and exit codes are covered by AT-10. `get`, `links`, and `related` now provide bounded exact-source and graph retrieval through shared library and CLI interfaces. Whole-vault immutability and a repeatable six-document retrieval baseline are recorded; the legacy-agent comparison remains unmeasured. AT-13 add/change/delete equivalence and incremental-by-default CLI indexing are complete. AT-14 now preserves identity for unambiguous identical-content renames. AT-15 adds debounced filesystem watching, streaming CLI reports, and concurrent-reader coverage. Next is doctor diagnostics and operational recovery documentation. License selection and hosted CI remain pending.

This plan describes delivery order. [TODO.md](../TODO.md) is the actionable queue. [TEST_PLAN.md](TEST_PLAN.md) defines acceptance contracts. [AGENTS.md](../AGENTS.md) defines the mandatory development process.

## Delivery strategy

Build narrow, working vertical slices rather than implementing every subsystem before exposing a usable command. Each feature follows this sequence:

1. Select or define acceptance cases in the test plan.
2. Write the tests and fixtures before feature implementation.
3. Run the tests and confirm they fail for the expected missing behavior.
4. Implement the smallest coherent behavior that satisfies them.
5. Do not modify, delete, skip, or weaken acceptance cases to make implementation pass.
6. Refactor only while preserving the tests and public behavior.
7. Run the complete quality gate and update documentation/TODO status.

Trivial Cargo scaffolding and test fixtures may precede feature code. Production behavior may not.

## Milestone M0 — reproducible foundation

### Deliverables

- Product brief, delivery plan, test plan, TODO, and agent instructions.
- Stable Rust toolchain using the standard rustup installation.
- Minimal Cargo package with library, CLI binary, and later MCP binary target.
- Formatting, linting, test, and coverage commands documented.
- Immutable Markdown fixtures under `tests/fixtures/`.
- Initial CI configuration if the project gains a hosted remote.
- License decision before public distribution.

### Exit gate

- `cargo fmt --check` passes.
- `cargo clippy --all-targets --all-features -- -D warnings` passes.
- `cargo test --all-targets --all-features` runs and reports the expected initial failing acceptance test before implementation.
- No feature implementation exists before its test has been written.

## Milestone M1 — read-only index core

### Deliverables

- Vault configuration and safe file discovery.
- Owned document/section model.
- Frontmatter, heading, tag, task, Markdown-link, and wikilink parsing.
- Path-based Obsidian link resolution, including aliases and heading targets.
- SQLite schema, migrations, and transactional full indexing.
- FTS5 section index.
- `jabfa index` and `jabfa status`.
- Deterministic diagnostics that do not abort an entire index because one note is malformed.

### Tests first

Implement AT-01 through AT-06 and AT-12 from the test plan before the corresponding production behavior.

### Exit gate

- A representative fixture vault indexes successfully.
- Documents and heading-level sections are queryable directly in SQLite.
- Wikilinks and backlinks resolve according to fixtures.
- Malformed notes produce scoped diagnostics.
- Source fixture hashes remain unchanged.
- A second full rebuild produces equivalent normalized content.

## Milestone M2 — useful CLI retrieval

### Deliverables

- `jabfa search` with filters, limits, snippets, and citations.
- `jabfa get` for exact document/section ranges.
- `jabfa links` and bounded `related` traversal.
- `jabfa context` with deterministic ranking, deduplication, budget enforcement, and omitted-result reporting.
- `jabfa sql` through a constrained read-only connection.
- Human and stable machine-readable output.

### Tests first

Implement AT-07 through AT-11 plus the SQL safety cases before production behavior. Freeze the initial JSON envelope through contract tests.

### Exit gate

- One `context` call returns enough cited evidence for a representative recall question.
- Output budget and result limits are enforced.
- SQL cannot mutate the database or access files outside configured capabilities.
- No command requires an LLM or network access.

## Milestone M3 — incremental indexing and watching

### Deliverables

- File/content-hash change detection.
- Added, changed, deleted, and renamed file handling.
- Transactional per-change index updates.
- `jabfa index --watch` with debounced events.
- `jabfa doctor` for stale/missing entries and broken links.
- Explicit index-generation reporting.

### Tests first

Add mutation tests against temporary copies of fixtures. Verify normalized incremental results against a clean full rebuild after every operation type.

### Exit gate

- Incremental indexing is equivalent to full rebuilding.
- Watch events cannot produce partially visible transactions.
- Reads during indexing remain valid.
- Deleted or renamed notes do not leave stale search results.

## Milestone M4 — MCP adapter

### Deliverables

- `jabfa-mcp` using local stdio transport.
- Tools for `search`, `context`, `get_section`, `get_document`, `related`, `query_sql`, and `index_status`.
- Structured responses sharing CLI result types.
- Time, result, byte, depth, and output limits.
- Documentation for registering the server with MCP clients.

### Tests first

Use protocol-level tests with an in-memory or local test client. Compare semantic responses with CLI/library results; do not compare transport-specific formatting.

### Exit gate

- An MCP client can complete a full search-to-evidence workflow.
- CLI and MCP semantic equivalence tests pass.
- Invalid requests and oversized results fail cleanly without a server crash.

## Milestone M5 — deterministic knowledge writes

### Deliverables

- Explicit, opt-in structured operations such as `create_learning` and `patch_section`.
- Path/citation validation and stale-content detection.
- Transactional source patch plus index update strategy with recoverable failure behavior.
- Generated catalog/log updates where configured.
- Integration plan for replacing routine `brain-recall` and `brain-scribe` work.

### Tests first

Test only against temporary vaults. Cover invalid paths, stale hashes, conflicting edits, malformed templates, interrupted operations, and rollback/recovery.

### Exit gate

- Writes never affect files outside the configured vault.
- Failed operations do not silently leave source and index state inconsistent.
- Real user vaults remain untouched until separate explicit opt-in acceptance testing.

## Milestone M6 — measured extensions

Only schedule features whose need is demonstrated by benchmarks or user workflows:

- Local embeddings and hybrid ranking.
- Claim-level extraction and contradiction tracking.
- Query caches tied to index generations.
- Asset/OCR indexing.
- Obsidian plugin, web UI, Marp, or visualization generation.
- Optional integrations with other knowledge clients.

## First implementation slice

The first coding session should do only the following, in order:

1. Install and verify stable Rust.
2. Scaffold the minimal Cargo project without implementing vault behavior.
3. Add the basic fixture vault.
4. Write AT-01 (`index_discovers_documents_without_modifying_sources`) and AT-02 (`parser_builds_heading_sections_with_source_ranges`) as failing tests.
5. Run them and record the expected failure.
6. Implement just enough discovery, parsing, and persistence for those cases.
7. Run the complete quality gate.
8. Stop, review the public model/schema, and update TODO before expanding the feature set.

## Engineering policies

### Dependencies

Start small. Proposed dependencies must solve a concrete current requirement. Avoid adding async runtimes, vector databases, plugin systems, or web frameworks before an interface requires them. Commit `Cargo.lock` for reproducible binary builds.

### Rust design

Prefer a synchronous core with owned values and short-lived borrows. Keep async code at watcher/MCP boundaries if required. Do not use `unsafe`, global mutable state, self-referential structures, or shared interior mutability merely to silence the compiler.

### Compatibility

Treat CLI JSON, MCP structured responses, and the SQLite schema as public contracts once released. Version changes deliberately and keep migration/contract tests.

### Documentation

Update README examples when interfaces become real. Mark implemented, experimental, and planned behavior accurately. Record significant design changes in the brief/plan rather than letting documentation drift.

## Release checkpoints

- **0.1.0:** M1–M2 read-only full indexing and CLI retrieval.
- **0.2.0:** M3 incremental/watch support.
- **0.3.0:** M4 MCP support.
- **0.4.0:** M5 opt-in structured writes.

Version numbers are planning targets, not commitments to dates.
