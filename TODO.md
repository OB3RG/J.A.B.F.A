# JABFA TODO

This is the actionable work queue. Keep it ordered and update it only after the stated acceptance gate is met. See [docs/PLAN.md](docs/PLAN.md) for milestone rationale and [docs/TEST_PLAN.md](docs/TEST_PLAN.md) for immutable test contracts.

## Now — M0: reproducible foundation

- [x] Install stable Rust with rustup and verify `rustc`, `cargo`, `cargo fmt`, and `cargo clippy` are available.
- [x] Choose and document the supported Rust version/edition in a toolchain file.
- [x] Scaffold a minimal Cargo package with a reusable library and `jabfa` binary; reserve `jabfa-mcp` for M4.
- [x] Add standard formatting/lint/test commands or a justfile without adding unnecessary tooling.
- [x] Generate `Cargo.lock` and keep it included for the binary application (Git commit still awaits user request).
- [x] Create minimal read-only fixtures under `tests/fixtures/basic/`.
- [x] Write AT-01 and AT-02 as failing integration tests before implementing discovery/parsing.
- [x] Run AT-01 and AT-02 and record that they fail for the expected missing behavior.
- [x] Implement only enough M1 behavior to satisfy AT-01 and AT-02.
- [x] Run the full quality gate and review the initial owned data model.
- [ ] Decide on a license before publishing the repository.
- [ ] Add CI when a hosted repository/remote is selected.

## Next — M1: read-only index core

Write CLI acceptance tests before implementing the remaining `index`/`status` commands. Then begin M2 with AT-07.

- [x] Write AT-03, then implement typed frontmatter, tag, and task extraction.
- [x] Write AT-04, then implement Obsidian wikilink/alias/heading/embed resolution and backlinks.
- [x] Write AT-05, then implement SQLite schema v1, FTS5, and transactional repeatable full indexing.
- [x] Write AT-06, then implement per-document diagnostics and failure isolation.
- [x] Implement safe root discovery and default exclusions without modifying source notes.
- [x] Implement `jabfa index` and `jabfa status` with clear errors and exit codes, tests first.
- [x] Verify full-rebuild normalized equivalence and source-vault immutability.
- [x] Document the initial SQLite schema and public Rust model.

## M2: useful CLI retrieval

- [x] Write AT-07, then implement ranked section search with citations and snippets.
- [x] Write AT-08, then implement deterministic `context`, budget accounting, deduplication, and omission reporting.
- [x] Write CLI tests, then expose `search` and `context` through the shared library.
- [x] Write AT-09, then implement constrained read-only SQL with resource limits.
- [x] Write AT-10, then freeze the initial versioned JSON output and stderr/exit-code behavior.
- [x] Write AT-11, then implement bounded `get`, `links`, and `related` operations.
- [x] Write AT-12 across every read-only CLI command and fix any source mutation.
- [x] Add representative retrieval fixtures and measure baseline latency/context size.
- [x] Update README with tested commands and examples.

## M3: incremental indexing and watching

- [x] Write AT-13, then implement content-hash-based add/change/delete indexing.
- [x] Write AT-14, then implement rename handling and citation/link updates.
- [x] Write AT-15, then implement debounced atomic watch updates.
- [ ] Write AT-16, then implement `doctor` for stale entries and broken links.
- [x] Test concurrent readers during indexing.
- [x] Compare incremental query results with clean full rebuilds across mutation sequences.
- [ ] Publish index-generation semantics and operational recovery documentation.

## M4: MCP adapter

- [ ] Select the maintained Rust MCP SDK and validate it with a minimal protocol test before integration.
- [ ] Write AT-17, then expose shared `search`, `context`, get, and related operations over stdio MCP.
- [ ] Add bounded read-only `query_sql` and `index_status` MCP tools.
- [ ] Write AT-18, then implement protocol-safe validation and resource limits.
- [ ] Test CLI/MCP semantic equivalence and failure behavior.
- [ ] Document client registration and security constraints.

## M5: opt-in structured writes

- [ ] Finalize the source/index transaction and recovery design before implementation.
- [ ] Write AT-19, then implement validated path/citation and stale-content checks.
- [ ] Write AT-20, then implement failure recovery and consistency verification.
- [ ] Add narrow `create_learning` and `patch_section` operations against temporary vault tests.
- [ ] Add configured catalog/log generation only after source-write safety is proven.
- [ ] Plan a separate explicit migration for the existing llm-brain workflow.
- [ ] Never run write acceptance tests against a real user vault.

## Later — only with measured need

- [ ] Local embeddings and hybrid lexical/vector retrieval.
- [ ] Claim-level metadata, contradiction detection, and validity intervals.
- [ ] Query/answer caches tied to index generations.
- [ ] Asset indexing, OCR, and image metadata.
- [ ] Generated Obsidian catalog views.
- [ ] Obsidian plugin, web UI, and visualization outputs.
- [ ] Packaging/release binaries and shell completions.

## Completed

- [x] Select **JABFA — Just A Brain For AI** as the project name.
- [x] Initialize `~/Work/jabfa` as a Git repository on `main`.
- [x] Write the README and product positioning.
- [x] Write the product brief and architecture boundaries.
- [x] Write the phased delivery plan.
- [x] Define initial acceptance cases and test-first quality gates.
- [x] Define immutable acceptance-test and borrow-checker-friendly Rust instructions for agents.
- [x] Add a Rust/project `.gitignore`.
- [x] Implement the AT-01/AT-02 vertical slice with discovery, parsing, and transactional SQLite persistence.
- [x] Verify `cargo fmt --check`, Clippy with denied warnings, all-target tests, and documentation tests.
- [x] Complete AT-03 through AT-06 without weakening acceptance contracts.
- [x] Add typed frontmatter, tags, tasks, Obsidian link resolution, backlinks, FTS5, index generations, and per-document diagnostics.
- [x] Complete M1 CLI `index`/`status` with JSON/human output and tested errors.
- [x] Complete AT-07 ranked citations and AT-08 bounded context; all 10 tests and quality gates pass.
- [x] Expose `search` and `context` through the CLI with human/JSON output; all 12 tests and quality gates pass.
- [x] Complete AT-09 and `jabfa sql` with authorization and row/byte/time limits; all 14 tests and quality gates pass.
- [x] Complete AT-10 versioned JSON schemas, diagnostics, and exit-code contracts; all 15 tests and quality gates pass.
- [x] Complete AT-11 bounded source retrieval, links, and related traversal with CLI adapters; all 17 tests and quality gates pass.
- [x] Complete AT-12 whole-vault immutability checks and the repeatable retrieval baseline; all 18 tests and quality gates pass.
- [x] Complete AT-13 incremental add/change/delete and CLI `--full`; all 20 tests and quality gates pass.
- [x] Complete AT-14 unambiguous identical-content rename identity; all 22 tests and quality gates pass.
- [x] Complete AT-15 debounced atomic watching, CLI streaming, and concurrent-reader coverage; all 24 tests and quality gates pass.
