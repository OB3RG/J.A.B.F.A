# JABFA CLI Contract

Status: initial pre-release machine-output contract, covered by AT-10. Human-readable output is for terminals and is not a stable automation interface.

## JSON success output

With `--json`, each successful non-watch command writes exactly one JSON object followed by a newline to stdout. Watch mode writes one such object per committed indexing batch as JSON Lines. JABFA adds no color or terminal decoration. Every success object contains:

- `output_schema_version`: integer `1`.

This identifies the shared machine-readable result schema. It is distinct from `status.schema_version`, which is the SQLite schema version stored in the index.

The initial fields are frozen as follows; every listed top-level object also includes `output_schema_version`:

| Command | Top-level fields | Nested fields |
|---|---|---|
| `index` | `files`, `documents_indexed`, `sections_indexed`, `generation`, `diagnostics` | Each diagnostic: `path`, `kind`, `message` |
| `status` | `generation`, `schema_version`, `state`, `documents`, `sections` | None |
| `subjects` | `generation`, `results` | Each result: `path`, `title`, `entry_type`, `match_source`, `matched_sections`, `section_id`, `heading_path`, `start_line`, `end_line`, `content_hash`, `score`, `snippet` |
| `search` | `generation`, `results` | Each result: `section_id`, `path`, `heading_path`, `start_line`, `end_line`, `content_hash`, `score`, `snippet` |
| `context` | `generation`, `context`, `estimated_tokens`, `included`, `omitted`, `deduplicated`, `truncated` | None |
| `sql` | `columns`, `rows`, `bytes` | Rows are positional arrays of typed JSON values corresponding to `columns` |
| `get` | `generation`, `path`, `start_line`, `end_line`, `content`, `content_hash`, `bytes` | None |
| `links` | `generation`, `path`, `links` | Each link: `kind`, `raw_target`, `target_heading`, `resolution`, `line_number`, `target_path` |
| `related` | `generation`, `root`, `nodes`, `diagnostics`, `bytes` | Nodes: `path`, `depth`; diagnostics: `path`, `line_number`, `raw_target`, `resolution`, `message` |
| `doctor` | `generation`, `healthy`, `diagnostics` | Each diagnostic: `path`, `kind`, `message` |
| `create-learning` | `path`, `content_hash`, `bytes`, `generation` | Requires `--allow-write`; optional catalog/log configuration |
| `patch-section` | `path`, `content_hash`, `bytes`, `generation` | Requires `--allow-write` and a current section citation |
| `brain-create-learning` | `path`, `content_hash`, `bytes`, `generation` | Requires `--allow-write`; creates a schema-valid brain learning and updates registry/log |

`context` accepts repeated `--path <vault-relative-path>` options to restrict evidence to at most 20 selected subjects. Without `--path`, it retains whole-vault behavior. For oversized selected sections, bold milestone/finding/consequences/link blocks can be composed into up to four complementary passages: the latest sufficiently relevant milestone is prioritized, distinct bold-label query facets are retained, and the highest-priority passage is truncated before any older supporting passage. Unfiltered context keeps the original continuous snippet-anchored truncation. `subjects` groups up to 100 ranked section candidates by document and ranks title matches, then aliases, then content matches using case-insensitive alphanumeric token normalization, with generated `log.md`/`index.md` entries demoted within each match class.

The library response types serialize to the same semantic objects as their CLI counterparts. Field names, types, and meanings must not be changed silently; changes require acceptance coverage and deliberate schema-version handling.

## Stdout, stderr, and diagnostics

- Normal results go to stdout.
- Fatal operational and query diagnostics go to stderr as plain text prefixed by `jabfa:`; failed commands do not emit a JSON success object to stdout.
- Per-document index diagnostics remain structured in the JSON `diagnostics` array and are also written to stderr as `jabfa: <path>: <message>`. An index that commits valid documents despite scoped note failures is still successful.
- Successful non-index commands have empty stderr. Successful indexing has empty stderr when no note diagnostics occurred.
- JSON mode never adds terminal decoration to stdout or stderr.

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Success, including a committed index with scoped per-document diagnostics. |
| `1` | Operational, indexing, retrieval, SQL-safety, or resource-limit failure. |
| `2` | Command-line usage error reported by Clap, such as missing required arguments, omitted `--allow-write`, or an unknown command. |

There is currently no JSON error-envelope contract; errors are plain stderr diagnostics. The versioned success schemas above are the initial automation contract.
