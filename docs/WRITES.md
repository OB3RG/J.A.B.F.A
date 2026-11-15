# Structured Write Safety and Recovery

Status: M5 implemented and covered by AT-19/AT-20 plus success, generated-output, and CLI opt-in tests. Write operations are deliberately absent from the read-only MCP server.

## Scope

The only planned operations are:

- `create_learning`: create one new Markdown learning note with a validated title, body, and tags.
- `patch_section`: replace one exact indexed section range after validating its path, source lines, and expected section content hash.

There is no arbitrary file write, delete, rename, shell execution, or generic text patch API. Tests use only temporary copies of fixtures, never a real user vault.

## Validation before source changes

1. Require a nonempty vault-relative `.md` path with no root, prefix, or parent traversal.
2. Reject paths containing a `raw` component and paths whose canonical destination would escape the configured vault.
3. For `create_learning`, require a nonexistent destination, a nonempty single-line title, and valid UTF-8 content.
4. For `patch_section`, require an existing indexed section with the exact path/start/end citation and the caller's expected section content hash. A missing or different hash is a stale-citation error.
5. Parse the proposed complete document before committing it. Invalid proposed Markdown/frontmatter aborts without changing the source or index.

## Source-first transaction protocol

Markdown is authoritative and SQLite is derived. A filesystem and SQLite transaction cannot form one atomic cross-resource transaction, so JABFA uses an explicit recoverable protocol:

1. Validate and render the complete proposed document.
2. Write the complete content to a uniquely named temporary file in the destination directory, flush it, and atomically rename it to the destination. The destination directory is created only within the validated vault for a new learning note.
3. After source commit, run incremental indexing and obtain a committed generation.
4. Return the path, new source hash, byte count, and committed generation only after indexing succeeds.

A crash or error before the source rename leaves the previous destination unchanged and may leave only an ignored temporary artifact for later cleanup. A source-rename failure aborts before indexing.

A crash or indexing failure after the rename can leave the new authoritative source with an older index. This is **not silent divergence**: the operation returns `WriteRecoveryRequired` containing the path and new source hash. The committed source remains intact; never automatically destroy it to conceal an index failure. `doctor` reports the source as changed, and running incremental indexing completes recovery. The caller can verify the resulting generation and source hash afterward.

A concurrent watcher may index the atomic source rename before the operation's own indexing call. That is safe because both use the same content hash and transactional incremental path; the operation still verifies that the stored source hash matches before reporting success.

## Failure tests

AT-19 covers path traversal, immutable `raw/` paths, missing targets, and stale hashes with byte-for-byte source/index non-modification. AT-20 uses temporary-vault fault scenarios for source failure and post-source index failure, verifies explicit recovery signaling and source preservation, runs doctor, and confirms normalized recovery with a full rebuild.

## Generated catalog and log

`IndexConfig::with_generated_outputs` and CLI `--catalog-path`/`--log-path` configure optional Markdown outputs for `create_learning`. The catalog appends a resolvable wikilink and title; the log appends a path and title. Existing entries are not duplicated. Each generated file follows the same validated atomic-write and indexing path, and the returned learning response carries the latest committed generation. A generated-output failure is explicit and recoverable under the same source-first rules.

## Brain learning workflow

`create_brain_learning` / `brain-create-learning` is a narrow llm-brain adapter for new learning pages. It validates `wiki/learnings/` paths, title/finding/context/consequences/origin, strict creation date, and proposed Markdown before writing. It preflights the Learnings index section and final log section, creates the note, then uses citation-checked patches for both registry updates. A missing registry fails before any source commit. Index insertion preserves the blank line before the following category. Real-vault canary and spacing regression are complete; MCP remains read-only.

## Opt-in rollout

- Write acceptance tests run only against temporary test vaults. CLI `create-learning` and `patch-section` require the literal `--allow-write` flag; omission is a usage error before any source change.
- The existing MCP server remains read-only until write acceptance and recovery tests pass.
- Any later MCP write exposure requires a separate explicit opt-in flag and updated tool annotations/security documentation.
- Catalog/log generation and migration of the existing llm-brain workflow are separate tasks after source-write safety is proven.
