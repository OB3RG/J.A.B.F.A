# Retrieval Baseline

## Repeatable case

`benchmarks/retrieval/` contains a six-document vault for the question **“Why were the brain agents slow?”** The required unique citations are `agent-architecture.md` and `local-index.md`; one archived duplicate must be deduplicated, and three distractor notes must be excluded. The evidence budget is 1,000 estimated tokens.

Run the baseline from the repository root:

```bash
cargo build --bin jabfa
python3 benchmarks/retrieval/run.py
```

The runner copies the checked-in vault to a temporary directory, indexes the copy, verifies citation coverage and deduplication, and prints JSON. Latency is CLI process wall time from a debug build, including process startup and SQLite access. Thirty measured iterations follow three warmup iterations. The script uses only the Python standard library.

## Initial measurement

Measured on the development environment with the debug CLI; these figures are a regression baseline, not portable performance guarantees.

| Metric | Result |
|---|---:|
| Documents | 6 |
| Total Markdown source | 2,766 bytes |
| Evidence payload | 787 bytes / 197 estimated tokens |
| Included sections | 2 |
| Deduplicated sections | 1 |
| Search process latency, median | 2.417 ms |
| Search process latency, min–max | 2.313–2.919 ms |
| Context process latency, median | 2.824 ms |
| Context process latency, min–max | 2.617–3.336 ms |

The evidence payload is 71.5% smaller than the total fixture source bytes. This is only a source-size comparison, not the product brief's target measurement of at least 80% fewer retrieved/context tokens relative to the legacy full-index agent workflow. No legacy-agent latency or token baseline has been measured, so that target remains **not evaluated**. The small fixture also does not establish throughput or latency targets for a representative full vault.
