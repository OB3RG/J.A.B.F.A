# Retrieval baseline fixture

Question: **Why were the brain agents slow?**

Query: `brain agents`

Required citation set after normalized-text deduplication:

- `agent-architecture.md`, heading `Agent architecture > Recall workflow`, for the full-catalog scanning cause.
- `local-index.md`, heading `Local index > Retrieval decision`, for the local ranked-section remedy.

`duplicate-architecture.md` intentionally repeats the first section and must be counted as deduplicated. The printing, cooking, and garden notes are distractors. The benchmark uses a 1,000-token evidence budget and requires both unique citations without requiring a particular BM25 tie order.

Run from the repository root after building the CLI:

```bash
cargo build --bin jabfa
python3 benchmarks/retrieval/run.py
```

The runner copies the immutable vault to a temporary directory, builds its index, checks citation coverage and deduplication, and measures CLI process wall time for search and context. It prints JSON and uses only the Python standard library. This is a small regression baseline, not a representative-vault performance certification or a legacy-agent comparison.
