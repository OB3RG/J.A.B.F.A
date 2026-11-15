# JABFA SQLite Schema — Version 1

Status: initial pre-release schema. The database is a derived artifact and can always be rebuilt from Markdown. Schema and output stability guarantees begin with a published release.

## Metadata

`index_metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL)` contains:

- `schema_version`: currently `1`.
- `generation`: decimal full-index generation, starting at `1` and incrementing on each successful rebuild of the same database.
- `state`: `indexing` inside the transaction and `complete` in a committed database.
- `document_hash:<path>`: BLAKE3 source-content hash used by incremental indexing. These per-document keys are derived metadata and do not change SQLite schema version 1.

A failed rebuild rolls back content and metadata together, preserving the previous committed generation.

## Documents

`documents` stores:

- `id`: local integer identity.
- `path`: vault-relative Markdown path using `/` in the index.
- `title`: first level-one heading, when present.
- `frontmatter`: YAML frontmatter converted to a JSON document; `NULL` when absent.

Frontmatter is queryable with SQLite JSON functions, for example:

```sql
SELECT path, json_extract(frontmatter, '$.title')
FROM documents
WHERE json_extract(frontmatter, '$.draft') = 0;
```

## Sections

`sections` stores heading-aware source regions:

- `document_id`: owning document.
- `heading`: section heading, or `NULL` for a non-empty preamble.
- `heading_path`: hierarchy headings separated by newlines.
- `level`: `1`–`6` for headings; `0` for a preamble.
- `start_line`, `end_line`: 1-based inclusive source lines. A heading section includes its heading and trailing lines up to the next heading.
- `text`: direct body text after the heading, trimmed at the boundaries. Child sections are stored separately.
- `content_hash`: BLAKE3 hash of the heading, heading path, and text, used in search citations and stale-content checks.

## Tags and tasks

`tags(document_id, tag)` combines unique frontmatter tags and inline Obsidian-style tags. Code blocks and inline code are excluded from inline-tag extraction.

`tasks(document_id, text, completed, line_number)` stores Markdown task-list items. Task-like text inside code blocks is excluded.

## Links

`links` stores forward links; backlinks are queried by target:

- `source_document_id`: document containing the link.
- `target_document_id`: resolved target, or `NULL` for unresolved/external links.
- `kind`: `markdown`, `wiki`, or `embed`.
- `raw_target`: target text extracted from Markdown.
- `target_heading`: text after `#`, when present.
- `resolution`: `resolved`, `broken`, `ambiguous`, or `external`.
- `line_number`: 1-based source line.

Wikilinks resolve by vault-relative path, unique file stem, or frontmatter alias. Multiple matching stems produce `ambiguous` rather than selecting an arbitrary note. Ordinary Markdown links resolve relative to the source document and may not escape the vault root.

Backlink example:

```sql
SELECT sources.path, links.line_number
FROM links
JOIN documents sources ON sources.id = links.source_document_id
JOIN documents targets ON targets.id = links.target_document_id
WHERE targets.path = 'projects/a/Shared.md'
ORDER BY sources.path, links.line_number;
```

## Full-text search

`section_search` is an FTS5 virtual table containing section `heading` and `text`, plus unindexed `document_id` and `section_id` references. It is populated in the same transaction as the relational rows.

```sql
SELECT documents.path, sections.heading, sections.start_line, sections.end_line
FROM section_search
JOIN sections ON sections.id = section_search.section_id
JOIN documents ON documents.id = sections.document_id
WHERE section_search MATCH 'relevant';
```

The `search_sections` library API uses BM25 ranking with path/line tie-breaking and returns complete citations. `build_context` retrieves full section text, deduplicates normalized content, and applies the documented evidence budget. Direct SQL remains available for low-level inspection through the constrained interface below.

## Read-only SQL

`query_sql` and `jabfa sql` accept one SQL statement on a connection opened read-only with SQLite `query_only` enabled. An SQLite authorizer allows reads, SELECT evaluation, recursive common-table expressions, and ordinary non-file functions. It denies writes, schema changes, transactions, all pragmas, attachment/detachment, and the `load_extension`, `readfile`, and `writefile` functions. Multiple statements are rejected.

Results contain column names and positional typed JSON rows. SQL `NULL`, integers, finite reals, and UTF-8 text map to their corresponding JSON values; BLOBs are `0x`-prefixed hexadecimal strings. `bytes` is the UTF-8 size of the JSON-encoded `rows` array and excludes column names and transport envelopes.

Every call has explicit row, result-byte, and execution-time limits. Requests are additionally capped at 1,000 rows, 1 MiB of JSON-encoded rows, and five seconds; SQLite also limits individual values to 1 MiB and SQL text to 16 KiB. Exceeding a limit returns an error rather than partial results. CLI defaults are 100 rows, 65,536 bytes, and 1,000 milliseconds, configurable with `--max-rows`, `--max-bytes`, and `--timeout-ms`.

## Incremental behavior

`index_vault_incremental` and default `jabfa index` compare current source hashes with `document_hash:<path>` metadata. Added, changed, deleted, unreadable, and newly malformed documents invalidate old derived rows. Only invalidated documents are replaced; unchanged document and section identities remain stable. When one deleted path and one added path have the same unique source hash, the rename updates the existing document row in place and preserves its ID. Ambiguous duplicate-content matches use ordinary delete/add handling. Link rows are re-resolved for the current document set in the same transaction because aliases, stems, and relative targets can change globally. `jabfa index --full` and `index_vault` retain full-rebuild behavior. Both paths share insertion and link-resolution helpers and use explicit generations.

## Rebuild behavior

A full index performs one SQLite transaction that:

1. Enables foreign keys before starting the transaction.
2. Creates schema v1 structures if absent.
3. Determines the next generation.
4. Replaces derived document, section, tag, task, link, and FTS rows.
5. Marks the generation complete and commits.

Per-document read/parse failures are reported in `IndexReport.diagnostics` and do not insert partial rows. Vault discovery failures and database failures abort the rebuild.
