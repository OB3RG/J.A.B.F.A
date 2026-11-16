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
- [x] Select the MIT license and add the license file.
- [x] Add Forgejo Actions CI now that the hosted remote is selected (workflow pushed in `e69f78a`; hosted result remains unobserved).

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
- [x] Write AT-16, then implement `doctor` for stale entries and broken links.
- [x] Test concurrent readers during indexing.
- [x] Compare incremental query results with clean full rebuilds across mutation sequences.
- [x] Publish index-generation semantics and operational recovery documentation.

## M4: MCP adapter

- [x] Select the maintained Rust MCP SDK and validate it with a minimal protocol test before integration.
- [x] Write AT-17, then expose shared `search`, `context`, get, and related operations over stdio MCP.
- [x] Add bounded read-only `query_sql` and `index_status` MCP tools.
- [x] Write AT-18, then implement protocol-safe validation and resource limits.
- [x] Test CLI/MCP semantic equivalence and failure behavior.
- [x] Document client registration and security constraints.

## M5: opt-in structured writes

- [x] Finalize the source/index transaction and recovery design before implementation.
- [x] Write AT-19, then implement validated path/citation and stale-content checks.
- [x] Write AT-20, then implement failure recovery and consistency verification.
- [x] Add narrow `create_learning` and `patch_section` operations against temporary vault tests.
- [x] Add configured catalog/log generation only after source-write safety is proven.
- [x] Plan a separate explicit migration for the existing llm-brain workflow.
- [x] Never run write acceptance tests against a real user vault (all write tests use temporary directories; keep this invariant).

## Dogfood follow-up — measured retrieval gap

- [x] Add failing regressions for long sections with bold milestone/topic markers and repeated terms, then preserve the snippet-matched passage when truncation is required.
- [x] Verify citation-supported coverage on long learning pages and prefer durable pages over redundant log-only evidence in the recall agent.
- [x] Measure a preliminary direct-subagent latency baseline (MCP 65.392 s versus legacy 50.896 s; single run, LLM-dominated, no end-to-end speedup claim).
- [x] Fix the dogfood `read-only` FTS5 failure with a literal-query regression; all 44 tests and quality gates pass.
- [x] Recover provider-reported usage for a clean equivalent agent pair: MCP 21,713 versus legacy 22,877 total tokens (5.1% fewer, not the 80% target); wall time was 32.628 s versus 21.732 s in one sample.
- [x] Implement AT-21/AT-22 subject discovery and selected-path context through library, CLI, and MCP, including punctuation-normalized title matching and literal Unix backslash-path regressions; all 49 tests and quality gates pass.
- [x] Run a preliminary subject-first comparison: 19,087 total tokens and 25.008 s versus 21,713 tokens and 32.628 s for previous MCP (12.1% fewer tokens, 23.4% faster); one question, with a residual historical tool-count summarization caveat.
- [x] Implement all four recall optimizations: bold-milestone passage composition (AT-23), required-path `investigate` (AT-24), a condensed four-tool/200-word recall agent, and snippet-only single-fact answers; all 55 tests and quality gates pass.
- [x] Measure the combined complex-question improvement: 17,618 tokens and 16.769 s versus 19,087 tokens and 25.008 s for the prior subject-first workflow (7.7% fewer tokens, 32.9% faster), with required MIT/wrapper/canary coverage. A simple license query skipped investigation and used 10,214 tokens in 5.888 s.
- [ ] Measure subject-first recall against the previous MCP-first and legacy workflows across representative questions before claiming the 80% target or changing migration defaults.

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
- [x] Complete AT-16 doctor diagnostics and M3 operational documentation; all 26 tests and quality gates pass.
- [x] Select official `rmcp` 3.10 and validate local stdio initialization/tool listing; all 27 tests and quality gates pass.
- [x] Complete M4 seven-tool stdio MCP adapter, AT-17/AT-18, and client/security documentation; all 30 tests and quality gates pass.
- [x] Complete M5 validated writes, recovery, generated outputs, CLI opt-in, and migration plan; all 35 tests and quality gates pass.
- [x] Select MIT and enable the global read-only llm-brain MCP plus brain-recall/brain-scribe integrations with live dogfood verification.
- [x] Fix dogfood long-section truncation with snippet-anchored excerpts and verify durable-page recall; all 37 tests and quality gates pass.
- [x] Recover narrow agent-frontmatter compatibility cases and index the full brain with zero diagnostics; all 39 tests and quality gates pass.
- [x] Add schema-valid brain learning/registry/log writes and complete the real-vault canary; all 43 tests and quality gates pass.
- [x] Treat search input as literal FTS5 terms, fix hyphenated queries, and measure provider-reported agent tokens; all 44 tests and quality gates pass.
