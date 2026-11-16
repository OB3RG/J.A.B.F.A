# JABFA Test Plan

## Purpose

This plan defines the initial acceptance contracts and testing strategy. JABFA is developed test-first: tests express the requirement before production code exists.

## Immutable acceptance-test policy

1. Write acceptance tests before implementing the behavior they cover.
2. Run each new test and verify that it fails for the expected missing behavior, not because the test harness is broken.
3. During implementation, do not edit, rename, delete, skip, ignore, weaken, or change expected values in an acceptance test to make code pass.
4. Do not alter acceptance fixtures or snapshots to accommodate implementation output.
5. Add new tests for additional requirements or regressions; do not fold new behavior into an old case by weakening its original assertions.
6. If a requirement is genuinely wrong, stop and obtain explicit user approval before changing its test. Record the requirement change in the relevant documentation and commit message.
7. Never mark a milestone complete with failing, ignored, or disabled acceptance tests.

Internal helper refactors are allowed only when they preserve every assertion, fixture, input, and expected result. When in doubt, treat the test as immutable and ask.

## Test levels

### Unit tests

Test pure parsing, path resolution, ranking, budget accounting, and normalization logic. Unit tests supplement acceptance tests; they do not replace them.

### Integration/acceptance tests

Exercise public library, CLI, database, and MCP behavior through stable interfaces. Name tests with their AT identifier so failures map back to this document.

### Property/fuzz tests, as useful

Later add generated Markdown and mutation sequences for parser robustness, path handling, link resolution, and incremental/full equivalence. These must be deterministic in normal CI using recorded seeds/corpus cases.

### Performance benchmarks

Measure indexing and retrieval separately from correctness tests. Establish a baseline before making strict latency targets. Benchmarks must not require network access or a user's real vault.

## Initial acceptance cases

These are requirement identifiers. Write the corresponding executable tests before implementing each case.

Current executable coverage: **AT-01 through AT-16, the AT-14 regression, two M1, four M2, and three M3 CLI tests pass (26 total)**. Behavioral cases through AT-11 were observed failing before implementation. AT-12 passed on its first run as verification of already implemented read-only behavior and required no production change. No existing assertions, fixtures, inputs, or expected values were weakened; formatting and test-only type aliases preserved test semantics. The M2 CLI tests verify semantic equality with the shared search/context/SQL library responses, human output, stderr separation, and source-vault immutability. While authoring AT-09, two new expectations initially confused `documents.title` with the frontmatter title; with explicit user approval, only those new expectations were corrected to `Alpha`, the fixture's first level-one heading and documented `documents.title`. AT-10 freezes the initial success-object fields with `output_schema_version: 1`, separates diagnostics from stdout, and covers exit codes 0, 1, and 2. AT-11 covers exact source ranges, outgoing link resolution, deterministic bidirectional related traversal, missing-link diagnostics, and row/node/byte bounds. AT-12 checks every read-only CLI command in both output modes and verifies the complete vault manifest after each invocation. AT-13 compares all normalized tables and FTS rows after initial/add/change/delete incremental operations with full rebuilds and checks unchanged document identity. AT-14 covers path/citation/link updates and includes a regression proving identity preservation does not depend on SQLite rowid reuse. AT-15 verifies quiet-window debounce, streaming committed reports, and concurrent readers observing only complete generations. AT-16 covers healthy, changed, missing, unindexed, broken, and ambiguous states through library and CLI reports. Its new healthy-control setup initially used a fixture with an unresolved link; with continuation approval, the setup was corrected to a dedicated temporary healthy note without changing assertions or existing fixtures. A separate `rmcp` 3.10 smoke test validates local stdio initialization, server identity, tool capability, and the pre-integration empty tool list. AT-17 verifies exact structured semantic equality across MCP, CLI, and library for search/context/get/related; SQL/status have equivalent adapter coverage. AT-18 covers malformed/oversized requests, unknown tools, unsafe/excessive SQL, and connection continuity. Four MCP tests plus the 26 acceptance/CLI tests pass (30 total). AT-19 verifies traversal/raw/stale/missing-target rejection without source or index changes. AT-20 uses a user-approved SQLite `BEGIN IMMEDIATE` fault setup to force post-source index failure and verifies explicit recovery; a separate source-path fault leaves source/index unchanged. Additional M5 tests cover valid create/patch, generated catalog/log output, and mandatory CLI `--allow-write`. All 49 acceptance/CLI tests plus six MCP tests pass (55 total). AT-23 and AT-24 were observed failing for missing passage composition and the required-path investigation tool. Follow-up regressions for latest-milestone priority, distinct bold-label facets, four-facet coverage, and truncating an oversized highest-priority passage before older evidence were also observed failing before implementation; the final review reported no findings. AT-21 and AT-22 were observed failing for the missing subject/filter APIs and MCP tool before implementation. Review follow-up regressions for punctuation-normalized title classification and literal Unix backslash paths were also observed failing before their fixes. Their contracts add unique subject grouping, durable title ranking, generated-log demotion, selected-path context, exact CLI/library/MCP semantic equality, traversal rejection, and selection bounds without changing the existing raw-search or unfiltered-context contracts. Brain-write tests cover schema-valid learning/index/log creation, registry preflight, CLI opt-in, and preservation of following-section spacing. The real-vault canary was backup-protected and verified by isolated diffs, doctor, and MCP retrieval. Two dogfood regressions cover narrow recovery of agent-style origin/description scalar colons and annotated updated fields without accepting the malformed control fixture. All write tests use temporary vaults.

AT-08 defines the initial budget estimator as `ceil(UTF-8 bytes / 4)` over the complete Markdown evidence payload, including citation headers, hashes, line ranges, separators, and truncation markers. The JSON transport envelope is not part of this evidence-budget number. Deduplication occurs before budget selection, and an oversized first section retains its complete citation while its body is truncated at a UTF-8 boundary with `…`. Dogfood regressions additionally require the excerpt to retain a late matching passage and to use the search snippet's location when a term occurs more than once. A literal-query regression requires the original `read-only` dogfood query to succeed through both search and context while preserving the hyphenated snippet term; user input is quoted as individual FTS5 phrases rather than evaluated as FTS5 syntax.

| ID | Test case | Required assertion |
|---|---|---|
| AT-01 | `index_discovers_documents_without_modifying_sources` | Discovers included Markdown, honors exclusions, and leaves a recursive source hash/manifest unchanged. |
| AT-02 | `parser_builds_heading_sections_with_source_ranges` | Builds the expected heading hierarchy, text spans, and 1-based inclusive source line ranges. |
| AT-03 | `frontmatter_tags_and_tasks_are_queryable` | Preserves typed frontmatter and extracts expected tags/tasks without treating code-block examples as real metadata. |
| AT-04 | `obsidian_links_resolve_to_documents_and_sections` | Resolves wikilinks, aliases, headings, embeds, relative Markdown links, backlinks, and duplicate-name behavior defined by fixtures. |
| AT-05 | `full_index_is_transactional_and_repeatable` | Commits a complete generation, supports a clean rebuild, and produces equivalent normalized content without duplicate rows. |
| AT-06 | `malformed_note_isolated_from_other_documents` | Reports a scoped diagnostic and indexes valid notes; malformed input never causes a panic or partial document transaction. |
| AT-07 | `search_returns_ranked_section_citations` | Returns relevant heading-level results with path, heading path, source lines, content hash, score, snippet, and index generation. |
| AT-08 | `context_respects_budget_and_deduplicates_evidence` | Includes citations in budget accounting, deduplicates overlapping evidence, reports omissions, and handles a single oversized section according to a documented truncation rule. |
| AT-09 | `read_only_sql_cannot_mutate_or_escape` | Rejects writes, schema changes, unsafe pragmas, attachment, and excessive results while allowing bounded SELECT queries. |
| AT-10 | `cli_json_schema_is_stable_and_stderr_is_separate` | Emits valid versioned output on stdout, diagnostics on stderr, documented exit codes, and no terminal decoration in JSON mode. |
| AT-11 | `get_and_related_enforce_bounds` | Retrieves exact source ranges and limits graph depth, nodes, bytes, and missing-link diagnostics. |
| AT-12 | `read_only_commands_preserve_entire_vault` | Hash all source files before and after every read-only command, including search, context, SQL, status, get, and links. |
| AT-13 | `incremental_add_change_delete_matches_full_rebuild` | Applies each mutation to a temporary vault and matches normalized results from a clean full rebuild without stale rows. |
| AT-14 | `rename_handling_preserves_identity_where_configured` | Handles a rename with identical content, updates citations/links, and does not retain the old path. |
| AT-15 | `watch_updates_are_atomic_and_debounced` | Coalesces filesystem events and exposes only complete index generations to concurrent readers. |
| AT-16 | `doctor_detects_staleness_and_broken_links` | Reports changed/missing files and unresolved targets with actionable, structured diagnostics. |
| AT-17 | `mcp_and_cli_return_equivalent_semantic_results` | Runs equivalent operations through both interfaces and compares the shared semantic payload, ignoring transport-only fields. |
| AT-18 | `mcp_rejects_invalid_and_oversized_requests_cleanly` | Enforces limits and returns protocol-appropriate errors without panic or connection loss. |
| AT-19 | `structured_write_rejects_stale_or_outside_root_changes` | Refuses stale hashes and path traversal and never writes outside a temporary test vault. |
| AT-20 | `failed_write_has_consistent_recovery_behavior` | Injected source/index failures follow the documented transaction/recovery contract without silent divergence. |
| AT-21 | `subject_search_groups_matches_into_investigable_entries` | Groups ranked section matches into unique document/subject entries, prioritizes durable title matches over generated-log occurrences, and retains best-section citations and bounded CLI/library-equivalent output. |
| AT-22 | `context_can_be_restricted_to_selected_subjects` | Retrieves token-bounded evidence only from selected vault-relative subject paths through equivalent library, CLI, and MCP interfaces while rejecting traversal and enforcing selection limits. |
| AT-23 | `selected_context_composes_relevant_passages_from_long_sections` | Composes complementary bold-milestone passages from an oversized selected section within the existing evidence budget, retaining required current and historical facts while excluding distractors and preserving raw-section behavior. |
| AT-24 | `investigate_requires_selected_subjects_and_matches_cli` | Requires investigation paths at the MCP boundary, uses a 1,200-token default with the same bounded passage-aware context as CLI/library, rejects excessive selections, and preserves connection health. |

## Fixture design

Create small, explicit vaults under `tests/fixtures/` before implementing the parser:

- `basic/`: frontmatter types, nested headings, Unicode, tags, tasks, code blocks, and ordinary Markdown links.
- `obsidian/`: wikilinks, heading links, aliases, embeds, duplicate stems, broken links, and images.
- `malformed/`: invalid YAML, unterminated constructs, unusual heading sequences, and invalid UTF-8 handling as defined by the loader contract.
- `retrieval/`: known relevant, overlapping, and distractor sections for ranking and context-budget tests.

Rules:

- Keep fixtures minimal and readable.
- Treat expected content and line numbers as part of the acceptance contract.
- Never mutate checked-in fixtures in place.
- Copy fixtures into a temporary directory for index/watch/write tests.
- Do not point tests at `~/Documents/llm-brain` or any other real user vault.

## Test-first execution loop

For each acceptance case:

1. Add or select its fixture and expected result.
2. Write the failing test with the AT identifier in its name.
3. Run the narrow test and record the expected failure.
4. Implement only the required production behavior.
5. Re-run the narrow test without changing its contract.
6. Run the complete suite and quality checks.
7. Refactor with tests unchanged.
8. Update TODO and documentation only after all required checks pass.

Useful commands once Rust is installed:

```bash
cargo test TEST_NAME_SUBSTRING -- --exact --nocapture
cargo test --all-targets --all-features
cargo test --doc
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
```

The exact narrow-test invocation may need a fully qualified test name; do not weaken filtering or ignore failing tests to obtain a green run.

## Rust testing conventions

- Use the built-in `#[test]` framework for ordinary cases.
- Use temporary directories with automatic cleanup for filesystem tests.
- Keep tests deterministic; no wall-clock-dependent expected values, network access, or reliance on developer home-directory state.
- Use `unwrap`/`expect` in tests when it improves failure diagnostics; avoid them in production error paths.
- Assert behavior and meaningful errors, not internal implementation details.
- Keep test parallelism compatible with isolated temporary databases and directories.
- Snapshot testing is not part of the initial strategy because snapshots must not be casually regenerated to accept changed behavior.

## Retrieval benchmark

Before optimizing retrieval, create a repeatable set of representative questions with:

- Expected source sections or acceptable citation set.
- Required factual coverage.
- Maximum evidence budget.
- Legacy agent token/latency baseline where available.
- JABFA retrieval latency, returned size, and citation coverage.

Do not turn a benchmark into a green test by deleting difficult questions or loosening expected citations without explicit requirement approval.

## Definition of a valid test run

A claim that tests pass must identify the command actually run. If Rust or another dependency is unavailable, report tests as **not run**, never as passing. A valid milestone run includes the full relevant suite, formatting, Clippy, and documentation tests where applicable.
