# JABFA Agent Instructions

These instructions apply to the entire repository. JABFA (**Just A Brain For AI**) is a local-first Rust knowledge engine for Obsidian vaults and structured Markdown, with CLI and MCP adapters over a shared query library.

## Read first

Before changing behavior, read the relevant documents:

1. `docs/PRODUCT_BRIEF.md` — scope, requirements, safety, and non-goals.
2. `docs/PLAN.md` — current milestone and delivery order.
3. `docs/TEST_PLAN.md` — acceptance contracts and test rules.
4. `TODO.md` — current actionable work.

Do not implement later-milestone features merely because they are mentioned in the brief. Keep work focused on the current vertical slice.

## Non-negotiable test-first workflow

For every production behavior:

1. Select or define the acceptance case and its fixture.
2. Write the test before production implementation.
3. Run it and confirm it fails for the expected missing behavior.
4. Implement the smallest coherent behavior that satisfies it.
5. Run the narrow test, then the complete relevant suite and quality gate.
6. Refactor with behavior and tests unchanged.
7. Update TODO/documentation only after checks pass.

Trivial Cargo scaffolding, fixtures, and test harness setup may precede feature code. Production behavior may not.

### Test cases are contracts

- Never edit, rename, delete, skip, ignore, weaken, or change expected values in an acceptance test to make implementation pass.
- Never regenerate fixtures/snapshots or relax assertions to accommodate incorrect output.
- Add regression tests for newly discovered bugs before fixing them.
- If a test requirement is genuinely wrong, stop and obtain explicit user approval before changing it; document why.
- Internal test helpers may be refactored only if every input, assertion, fixture, and expected result remains unchanged.
- Do not claim a milestone is complete with failing, ignored, or disabled tests.
- If a command cannot run, report **not run** with the reason. Never claim unexecuted tests pass.

## Borrow-checker-friendly idiomatic Rust

Rust's borrow checker cannot and should not be disabled. Design so it remains mostly unobtrusive:

- Prefer owned `String`, `PathBuf`, `Vec`, and domain structs at parser, storage, CLI, and MCP boundaries.
- Borrow with `&str`, `&Path`, and slices for short-lived read-only operations.
- Keep ownership graphs simple and function scopes short.
- Avoid self-referential structs, complicated lifetime parameters, global mutable state, and long-lived references into databases or parsers.
- Use iterators and idiomatic standard-library combinators where they remain readable.
- Clone deliberately at ownership boundaries; do not clone indiscriminately in hot paths merely to silence errors.
- Do not introduce `Rc<RefCell<_>>`, `Arc<Mutex<_>>`, or `unsafe` just to bypass ownership constraints. Use shared state only when the architecture actually requires it and document why.
- Keep the core synchronous. Introduce async only at watcher/MCP boundaries when required, rather than making every core API async.
- Prefer small, concrete types and explicit domain errors over clever abstraction.

## Common Rust practices

- Use stable Rust and standard `rustfmt` defaults unless the project later adopts a documented configuration.
- Run Clippy with warnings denied and address causes rather than adding blanket allows.
- Use `thiserror`-style domain errors in the library and `anyhow`-style context at binary boundaries if/when those dependencies are added.
- Avoid `unwrap()` and `expect()` in production paths; use them in tests where they improve diagnostics.
- Do not panic on malformed user Markdown, metadata, paths, queries, or MCP requests.
- Keep public APIs documented and machine-output schemas versioned.
- Commit `Cargo.lock` for reproducible application builds.
- Minimize dependencies. Justify each new dependency against a current requirement; do not add frameworks for speculative future features.
- Prefer standard CLI conventions: stdout for results, stderr for diagnostics, meaningful exit codes, and no color/decoration in JSON mode.

## Architecture boundaries

- Markdown source files are authoritative; SQLite is derived and rebuildable.
- Parser output uses an owned domain model independent of SQLite and transport types.
- The query/context service is shared by CLI and MCP. Do not duplicate ranking, filtering, budget, or citation logic in adapters.
- Keep CLI terminal formatting out of core result types.
- Keep MCP protocol concerns out of parser/indexer code.
- Use schema migrations and explicit index generations.
- Start with SQLite FTS5. Do not add embeddings, custom query languages, extra database backends, or web services without an approved plan and measured need.

## Data and user-vault safety

- Read-only operations must leave the entire source vault byte-for-byte unchanged.
- Never use `~/Documents/llm-brain` or another real user vault as a mutable test fixture.
- Keep checked-in fixtures immutable; copy them to temporary directories for mutation tests.
- Treat any `raw/` directory in a knowledge vault as immutable.
- Restrict configured roots and reject path traversal outside them.
- Do not index `.git`, `.obsidian`, or `.jabfa` artifacts by default.
- Core tests and retrieval must not require network access.
- Do not expose secrets in output, logs, tests, commits, or memory notes.
- Raw SQL must be read-only and bounded.
- Write support is out of scope until M5 and then only through narrow, validated operations against temporary test vaults.

## CLI/MCP compatibility

- Shared semantic results must be equivalent across library, CLI, and MCP.
- Include path, heading, line range, content hash, and index generation in evidence citations where required.
- Enforce documented result, byte/token, query-time, and graph-depth limits.
- Contract tests must compare semantic payloads, not terminal or protocol-specific decoration.
- Public output changes require new tests and explicit documentation; do not silently change existing contracts.

## Required quality gate

Once the Rust toolchain exists, run the relevant complete gate before declaring work done:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
cargo test --doc
```

Also run the narrow acceptance test for the current case with output visible when diagnosing a failure. If the project later adds a single task runner command, keep these underlying checks equivalent.

## Working style

- Keep changes scoped and reviewable.
- Do not rewrite the project structure or adopt a new architecture without user agreement.
- Do not modify product requirements or acceptance cases silently.
- Keep `TODO.md` current and check off work only when its gate is met.
- Update README examples when planned commands become implemented behavior.
- Record meaningful design decisions and their rationale in project documentation.
- Do not create a Git commit unless the user asks for one; leave changes available for review.
