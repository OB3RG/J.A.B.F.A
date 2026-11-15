# Index Generations and Recovery

## Generation semantics

- A generation identifies one committed SQLite index transaction.
- A new database starts at generation `1` after its first successful index.
- Every successful full or incremental indexing invocation increments the generation, including an invocation whose source hashes are unchanged. Callers must not interpret a newer generation as proof that content changed.
- Watch mode performs an initial incremental index and then increments once per debounced, relevant filesystem batch. Irrelevant files and `.git`, `.obsidian`, and `.jabfa` Markdown artifacts do not trigger generations.
- A failed transaction does not expose a new generation. Readers opening the database see the previously committed generation with `state = complete`; the transient `indexing` state is inside the write transaction.
- Search, context, source retrieval, links, related traversal, and doctor reports include the generation they read.

Document and section IDs are local SQLite identities, not durable cross-rebuild citations. Incremental indexing preserves unchanged IDs and unambiguous identical-content rename IDs; changed documents and full rebuilds may allocate different IDs. Citations use path, source lines, and content hashes rather than relying on row IDs alone.

## Incremental catch-up and watching

Default `jabfa index` scans configured Markdown sources, compares BLAKE3 content hashes, and updates invalidated documents transactionally. Link resolution is refreshed within the same transaction because a document change can affect aliases and targets elsewhere in the vault.

`jabfa index --watch` installs filesystem notifications before emitting its initial report, debounces relevant events, and emits one human or JSON report per committed batch. JSON watch output is JSON Lines: one versioned index report per line. Ctrl-C requests a clean shutdown. If the watcher stops or the process is offline, the next invocation scans current hashes and catches up; no event log is required.

## Consistency checks

Use:

```bash
jabfa doctor <vault> --database <database> --json
```

Doctor is read-only and reports `changed`, `missing`, `unindexed`, `broken_link`, and `ambiguous_link` diagnostics. `healthy` is true only when there are no diagnostics. Consistency findings are returned in a successful report rather than treated as an operational command failure. Source edits and a subsequent index are required to resolve findings; doctor never repairs notes or the index.

## Recovery

SQLite is derived and rebuildable; Markdown is authoritative.

1. Stop a running watcher.
2. Run `jabfa doctor` to identify current source/index differences.
3. Run default incremental indexing to catch up ordinary changes.
4. If derived state is corrupt or migration behavior is uncertain, run `jabfa index --full` against the same database.
5. If the database itself cannot be opened, move or delete only the derived database and index again into a new path. Never delete source Markdown as part of index recovery.

No recovery operation requires network access. Back up the authoritative vault separately; a JABFA SQLite file is not a substitute for source backups.
