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

## Project dogfood smoke

A release-build CLI run against the JABFA repository itself indexed 34 Markdown documents into 194 sections in generation 1. The query `MCP network` returned five ranked results and three context sections totaling 446 estimated tokens. A before/after hash manifest confirmed that every Markdown source was unchanged. One scoped diagnostic was expected from `tests/fixtures/malformed/Broken.md`. This smoke used the public project vault only; no personal vault was accessed and no write operation was performed. Latency was not measured in this run.

## Live brain MCP versus legacy retrieval

A read-only test used the public JABFA topic `JABFA MCP dogfooding` in the actual brain vault. Twenty warm MCP `context` calls followed startup with incremental indexing. Fifteen legacy runs used `scripts/search.sh`; the conservative legacy payload was its search output plus exactly the same cited source section returned by MCP, with no additional page reads. Token estimates for both sides use `ceil(UTF-8 bytes / 4)`.

| Metric | JABFA MCP | Legacy targeted retrieval |
|---|---:|---:|
| Warm request median | 0.718 ms | 762.576 ms |
| Warm request min–max | 0.653–1.423 ms | 739.421–773.676 ms |
| Model-visible structured payload | 948 bytes / 237 estimated tokens | 1,502 bytes / 376 estimated tokens |
| Evidence alone | 201 estimated tokens | Included in legacy payload |

MCP startup including incremental indexing was 29.579 ms. Warm retrieval was approximately 1,062× faster in process-latency terms. The model-visible payload was 37.0% smaller than the conservative legacy search-plus-evidence payload. Legacy search alone was 207 estimated tokens but contained no evidence text, so that alone is not an equivalent retrieval result.

These measurements exclude LLM inference, response generation, and Pi harness scheduling. They do not establish the product brief's 80% reduction target against a full legacy-agent workflow. The initial run selected a log section rather than the durable learning, and a broader current-setup query truncated a long single-H1 learning section before its latest bold-marked milestone. Follow-up regressions now require query-centered truncation anchored to the exact search-snippet fragment, including repeated terms. A fresh real-brain MCP run of the broader query retained MIT, `brain-recall`, and the startup wrapper within the same 1,000-token budget. The recall agent now prefers durable pages over redundant logs for settled knowledge, and a live recall returned the current setup from the durable learning. An attempted full Pi CLI agent benchmark timed out after 240 seconds and produced no usable measurement, so it is not counted. A subsequent direct-subagent comparison used the same current-setup question and model with JABFA MCP-first retrieval versus explicitly forced legacy search/read. Both responses returned the correct MIT/wrapper/agent safeguards with durable citations. Wall time was 65.392 seconds for MCP and 50.896 seconds for legacy, so the MCP run was 28.5% slower end to end in this single sample. Provider-reported total tokens were unavailable through the subagent result; only the model-visible payload estimate above is available. This preliminary run demonstrates that LLM/provider time dominates total latency and that raw retrieval speedup must not be presented as end-to-end speedup. No source files were modified by the measured retrieval calls.
