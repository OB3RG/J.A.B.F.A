# JABFA Product Brief

## Summary

**JABFA — Just A Brain For AI** is a local-first Rust backend that turns an Obsidian vault or a folder of structured Markdown into a queryable knowledge database. It preserves Markdown as the authoritative storage format while providing deterministic indexing, SQL access, ranked section retrieval, and token-bounded context through a CLI and MCP.

JABFA is inspired by MarkdownDB's folder-to-database model, but it is designed primarily for AI knowledge work rather than website generation. Its distinguishing features are Obsidian-aware parsing, heading-level sections, incremental indexing, bounded evidence bundles, and agent-native interfaces.

## Problem

Personal Markdown wikis become more useful as they grow, but naive LLM access becomes increasingly expensive:

- Agents scan directories and repeatedly read large catalogs.
- Retrieval is often document-level when only one section is relevant.
- Full-text search, metadata filtering, and link traversal are re-derived by an LLM instead of executed by software.
- A separate agent may be consumed for routine recall or index bookkeeping.
- Context size and retrieval cost are difficult to predict.
- Agent-maintained indexes can drift from the underlying notes.

The storage format is not the main problem. The problem is using an LLM as a filesystem crawler and query engine.

## Target users

### Primary

- People with an Obsidian vault who use coding agents or other MCP-capable LLM clients.
- Developers and researchers maintaining personal or project knowledge in Git-backed Markdown.
- Users who want local, inspectable retrieval without adopting a hosted RAG product.

### Secondary

- Teams with structured Markdown documentation who need CLI and SQL access.
- Static-site or digital-garden authors who want a Rust alternative to MarkdownDB.

## Value proposition

> Keep writing Markdown. Let JABFA compile it into a local knowledge index and give any LLM exactly the passages it needs, with citations and a predictable context budget.

## Product goals

1. Index an ordinary Markdown folder without requiring changes to its notes.
2. Understand common Obsidian constructs, including frontmatter, tags, wikilinks, heading links, embeds, and aliases.
3. Address documents and heading-level sections by stable citations containing path and source lines.
4. Provide fast exact and full-text retrieval through SQLite.
5. Provide a single `context` operation that returns a deduplicated, token-bounded evidence bundle.
6. Expose equivalent CLI and MCP interfaces over a shared Rust query library.
7. Support safe, read-only SQL for advanced metadata and relationship queries.
8. Reindex only changed content while preserving full-rebuild equivalence.
9. Keep core operation local, deterministic, and usable without network access.
10. Make source-vault immutability a tested guarantee for all read-only commands.

## Non-goals for the initial product

- Replacing Markdown, Obsidian, or Git.
- Building a hosted collaboration platform.
- Creating a new general-purpose query language.
- Supporting multiple SQL database backends.
- Requiring embeddings, a vector database, or a local model.
- Building a web UI or Obsidian plugin before the core engine is proven.
- Automatically rewriting notes during read-only operations.
- Fine-tuning or training models.
- Exposing unrestricted filesystem writes through MCP.

## Core use cases

### UC-1: Agent recall

An agent asks why a previous decision was made. JABFA returns a small set of ranked note sections with path/line citations and the index generation. The agent answers without scanning the vault.

### UC-2: Structured documentation query

A user asks for all learning notes updated after a date, all notes with a tag, or all backlinks to a project. The CLI executes a metadata or graph query against SQLite.

### UC-3: Token-bounded research context

A user requests evidence for a question with a 2,000-token budget. JABFA searches, ranks, deduplicates, optionally follows essential links, and returns only the selected evidence.

### UC-4: Automation

A script runs `jabfa index`, consumes JSON or JSON Lines from `jabfa search`, and uses stable schemas in a pipeline without an LLM.

### UC-5: MCP client integration

An MCP-capable client starts `jabfa-mcp` for a vault and calls `search`, `context`, `get_section`, `related`, `query_sql`, or `index_status`.

### UC-6: Safe knowledge writes, later

A later release provides structured operations such as `create_learning` and `patch_section`. JABFA validates paths and citations, applies a patch, and updates the derived index transactionally.

## Functional requirements

### P0 — required for the MVP

- Configure a vault root and include/exclude patterns.
- Recursively discover Markdown while respecting ignore rules and excluding internal index files.
- Parse UTF-8 Markdown, YAML frontmatter, headings, paragraphs, tags, tasks, and ordinary links.
- Parse Obsidian wikilinks, heading references, aliases, and embeds.
- Split notes into heading-aware sections with source line ranges.
- Store documents, sections, metadata, links, tags, tasks, and content hashes in SQLite.
- Build an FTS5 index over section headings and text.
- Support full indexing and deterministic schema migrations.
- Provide CLI commands for `index`, `search`, `context`, `get`, `links`, `sql`, and `status`.
- Provide human-readable and stable JSON output for agent-facing commands.
- Enforce search result and context-size limits.
- Include source path, heading path, line range, content hash, and index generation in evidence citations.
- Make SQL query mode read-only and apply execution/result limits.
- Return actionable errors for invalid roots, malformed metadata, missing targets, and stale citations.
- Never modify source Markdown during MVP commands.

### P1 — after the read-only vertical slice

- Incremental indexing based on file/content hashes.
- Filesystem watch mode.
- Rename and deletion handling.
- MCP stdio server exposing the shared query service.
- CLI/MCP semantic equivalence tests.
- Recursive link traversal with bounded depth and result counts.
- Index consistency checks through `doctor`.
- Deterministic structured write operations.
- Git commit/index-generation correlation.

### P2 — only after measurement shows a need

- Local embeddings and hybrid lexical/vector ranking.
- Claim-level metadata, confidence, and validity intervals.
- Query and answer caches tied to an index snapshot.
- Asset indexing, OCR, and image metadata.
- Generated Obsidian catalog views.
- Web UI, Obsidian plugin, or visualization outputs.

## Interface principles

### CLI

- Use kebab-case commands and flags.
- Write normal results to stdout and diagnostics to stderr.
- Return meaningful nonzero exit codes.
- Offer `--json` for stable machine output; use JSON Lines for streaming results where appropriate.
- Keep machine schemas versioned and independent of terminal formatting.
- Require explicit limits for expensive graph and SQL operations.

### MCP

- MCP is an adapter over the same public query service used by the CLI.
- Start with local stdio transport, not a network server.
- Return structured content with the same citations and index generation as the CLI.
- Bound query time, rows, bytes, link depth, and context size.
- Treat raw SQL as an advanced read-only tool; common users call higher-level tools.
- Add narrow validated write tools later rather than generic arbitrary-file writes.

## Architecture

```text
Markdown vault
     │
     ▼
Discovery → Parsing → Owned document model → SQLite indexing
                                               │
                                               ▼
                                       Query/context service
                                          │           │
                                          ▼           ▼
                                         CLI         MCP
```

### Source layer

Markdown and assets are authoritative. The `.jabfa/` index directory is derived and may be deleted and rebuilt. Read-only operations must not alter source files.

### Core model

Use owned Rust values at parser and persistence boundaries. Documents contain sections and metadata; sections carry heading paths, text, and source ranges. Explicit IDs and content hashes support citations and change detection.

### Storage

SQLite is the only planned MVP backend. FTS5 provides lexical search, JSON functions support frontmatter queries, and relational tables support tags and links. Schema migrations are deterministic and recorded in the database.

### Retrieval

The initial pipeline is deterministic:

1. Apply metadata filters.
2. Run lexical search.
3. Rank and deduplicate sections.
4. Optionally traverse a bounded number of links.
5. Select evidence within the configured budget.
6. Return citations and index-generation metadata.

For subject-first recall, `search_subjects` groups a bounded set of ranked section candidates into unique document entries. It exposes title, entry type, match source, candidate-section count, and the best citation so an agent can choose what to investigate. `build_context_for_paths` then restricts the normal bounded-evidence pipeline to selected subject paths. For oversized selected sections, a deterministic composer can select up to four complementary bold-milestone passages, prioritizing the latest relevant milestone and distinct query-heading facets while preserving the evidence budget. MCP `investigate` makes the selected paths required and defaults to 1,200 tokens, preventing agents from accidentally falling back to whole-vault context. This deterministic layer does not claim to perform entity extraction or count every mention in the vault.

The initial context budget uses a deterministic conservative estimator: `ceil(UTF-8 bytes / 4)` over the complete Markdown evidence payload, including citations and separators. The JSON transport envelope is reported separately from this evidence-budget number. Exact normalized-text duplicates are removed before selection; budget-limited results are counted as omitted; and an oversized first section keeps its full citation while its body is truncated at a UTF-8 boundary with ellipses. When a search match would otherwise be truncated away, the excerpt is anchored to the search snippet's exact fragment, falling back to the longest matching query term and then the section start.

Embeddings are a later optional ranking signal, not an MVP dependency.

## Privacy and safety

- Core indexing and search make no network requests.
- Index only configured roots; prevent path traversal outside the vault.
- Exclude `.git`, `.obsidian`, and JABFA's own database artifacts by default.
- Do not print or transmit secrets discovered in notes; provide configurable exclusions.
- Run SQL through a read-only connection with resource limits.
- Keep raw source fixtures immutable during tests.
- Never test write functionality against a user's real vault.

## Success metrics

- Representative recall requires no LLM-driven vault scan.
- Evidence responses never exceed the documented output budget behavior.
- Read-only commands leave a before/after hash of the source vault unchanged.
- Incremental and full indexes produce equivalent normalized query results.
- CLI and MCP return equivalent semantic results for the same query.
- Local retrieval latency and indexing throughput are measured on a reference vault before performance targets are tightened.
- A representative benchmark measures token reduction and answer coverage against the legacy full-index agent workflow; target at least 80% fewer retrieved/context tokens without losing citation-supported coverage.

## Key risks and mitigations

| Risk | Mitigation |
|---|---|
| Obsidian syntax differs from strict CommonMark | Maintain dedicated fixtures and a path-based link resolver. |
| Chunking loses surrounding meaning | Use heading-aware sections, parent context, and bounded link expansion. |
| Index becomes stale | Content hashes, explicit generations, watch mode, and `doctor`. |
| Token budgets exclude important evidence | Deduplicate, diversify sources, and report omitted result counts. |
| MCP and CLI drift | Share one query service and test semantic equivalence. |
| SQLite SQL tool is abused | Read-only connections, timeouts, row/byte caps, and root restrictions. |
| Rust design becomes lifetime-heavy | Owned boundary models, short borrows, simple module APIs, and no self-referential structures. |
| Tests are weakened to accommodate code | Immutable acceptance-test policy with explicit user approval for requirement changes. |

## Naming and packaging

- Product: **JABFA**
- Expansion: **Just A Brain For AI**
- Rust package/library: `jabfa`
- CLI binary: `jabfa`
- MCP binary: `jabfa-mcp`
- Configuration: `jabfa.toml`
- Derived index directory: `.jabfa/`
