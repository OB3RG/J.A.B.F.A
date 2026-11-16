# llm-brain Dogfooding and Migration Plan

This is a separate, explicit rollout plan for `~/Documents/llm-brain`. No step authorizes writes to that vault by itself. Each stage requires evidence from the previous stage; source Markdown and Git remain the rollback mechanism.

## Stage 0 — baseline and backup

1. Ensure the vault is backed up and its Git working state is known.
2. Record a recursive source manifest and representative recall questions with acceptable citations and required facts.
3. Copy selected fixtures to temporary directories for all write tests. Never use the real vault as a mutable test fixture.
4. Keep the derived database outside the vault, for example `/tmp/llm-brain-jabfa.sqlite3`.

## Stage 1 — read-only CLI dogfooding

Available now:

```bash
cargo build --release
target/release/jabfa index ~/Documents/llm-brain --database /tmp/llm-brain-jabfa.sqlite3
target/release/jabfa subjects ~/Documents/llm-brain "representative question" --database /tmp/llm-brain-jabfa.sqlite3 --json
target/release/jabfa search ~/Documents/llm-brain "representative question" --database /tmp/llm-brain-jabfa.sqlite3 --json
target/release/jabfa context ~/Documents/llm-brain "representative question" --database /tmp/llm-brain-jabfa.sqlite3 --max-tokens 2000 --json
target/release/jabfa context ~/Documents/llm-brain "representative question" --database /tmp/llm-brain-jabfa.sqlite3 --path wiki/learnings/example.md --max-tokens 1200 --json
target/release/jabfa doctor ~/Documents/llm-brain --database /tmp/llm-brain-jabfa.sqlite3 --json
```

Compare citation coverage, evidence size, latency, and answer usefulness with the existing `brain-recall` workflow. Recompute the source manifest after commands. Keep the existing workflow as fallback. A separate `jabfa index --watch` process can keep the derived index current.

## Stage 2 — read-only MCP dogfooding

**Initial enablement complete:** the global Pi `jabfa` server is connected with nine direct read-only tools. A user-level wrapper refreshes `/tmp/llm-brain-jabfa.sqlite3` on startup; `brain-recall` discovers subjects first and uses required-path `investigate` for bounded passage-aware evidence, with targeted fallback, and `brain-scribe` uses MCP deduplication plus a derived-index refresh after existing wiki writes. Live recall and scribe dogfoods passed. This wrapper behavior is specific to the configured user setup; the stock MCP server itself still has no indexing tool.

Available now. Register `jabfa-mcp` locally as described in [MCP.md](MCP.md), using the absolute vault and `/tmp` database paths. Ask the same representative questions through the agent and compare the structured results with Stage 1. Do not enable remote transport. The MCP server does not index automatically, so continue the watcher or index after edits.

Exit criteria: required citation coverage is maintained, no source bytes change, no network transport is used, and failures fall back cleanly to the existing recall agent.

## Stage 3 — write rehearsal on a temporary copy

1. Copy a representative fixture subset to a temporary vault.
2. Use `create-learning` and `patch-section` with `--allow-write` only against that copy.
3. Exercise stale citations, path traversal, duplicate destinations, immutable `raw/`, malformed proposed content, and SQLite lock failures.
4. Configure catalog/log paths in the copy and inspect their deterministic entries and links.
5. Verify `doctor`, incremental/full normalized equivalence, and documented `WriteRecoveryRequired` recovery.

Real-vault writes remain disabled and no MCP write tools are exposed.

## Stage 4 — explicit real-vault write canary

**Initial canary complete:** `brain-create-learning` created `wiki/learnings/2026-11-15-jabfa-structured-write-canary.md` and updated the registry/log in generation 33. A spacing regression was fixed and a citation-checked patch restored the index boundary in generation 34. Backup diffs contain only the intended canary additions, doctor reports no changed/missing/unindexed entries, and MCP retrieves the new note. `brain-scribe` now uses this operation for new learning pages; existing-page edits remain direct.

This stage had separate, explicit user approval before execution.

1. Confirm backup, clean Git state, and a read-only baseline.
2. Create exactly one low-risk learning note through the CLI with `--allow-write` and configured catalog/log outputs.
3. Inspect the source diff, catalog/log diff, index generation, search citation, and doctor report.
4. Commit or revert that source change deliberately; do not hide it in an automated batch.
5. Rehearse one stale patch rejection and one valid patch on a noncritical temporary note before considering broader use.

## Stage 5 — gradual cutover and rollback

1. Prefer JABFA for routine recall when its measured citation coverage and latency are at least as reliable as `brain-recall`.
2. Keep `brain-scribe` until real-vault write canaries and recovery behavior have been reviewed repeatedly.
3. Replace scribe work only through the narrow learning/patch operations, not arbitrary writes or an unrestricted MCP tool.
4. To roll back, disable the MCP registration and write opt-in, stop the watcher, and continue using Markdown/Git with the original agents. The SQLite database can be discarded.

## Unresolved measurements

The initial synthetic retrieval benchmark is not an llm-brain baseline. One clean, same-question live-agent pair retained citation coverage and measured 21,713 provider-reported total tokens with MCP versus 22,877 with forced legacy retrieval: a 5.1% reduction, not the product brief's 80% target. MCP wall time was 32.628 seconds versus 21.732 seconds in that single sample. A subsequent subject-first run used 19,087 total tokens (12.1% less than section-first MCP and 16.6% less than legacy) and took 25.008 seconds (23.4% faster than section-first MCP but still 15.1% slower than legacy). It retained the required facts with selected-path context, although a stale historical tool-count statement remained. Combined passage composition, required-path investigation, a condensed four-tool/200-word recall prompt, and snippet-only simple answers then measured 17,618 tokens and 16.769 seconds on the complex question, with the required MIT/wrapper/canary facts correct. A simple license question skipped investigation and used 10,214 tokens in 5.888 seconds. These sequential single-question runs are preliminary. Repeat token/latency and citation-coverage measurements across representative questions before claiming the target or changing migration defaults. No automatic migration is scheduled without those measurements and explicit approval.
