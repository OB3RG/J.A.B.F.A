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

The evidence payload is 71.5% smaller than the total fixture source bytes. This is only a source-size comparison, not the product brief's target measurement of at least 80% fewer retrieved/context tokens relative to the legacy full-index agent workflow. The synthetic runner does not invoke an LLM; the live-agent pair below measures that separate target. The small fixture also does not establish throughput or latency targets for a representative full vault.

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

These measurements exclude LLM inference, response generation, and Pi harness scheduling. They do not establish the product brief's 80% reduction target against a full legacy-agent workflow. The initial run selected a log section rather than the durable learning, and a broader current-setup query truncated a long single-H1 learning section before its latest bold-marked milestone. Follow-up regressions now require query-centered truncation anchored to the exact search-snippet fragment, including repeated terms. A fresh real-brain MCP run of the broader query retained MIT, `brain-recall`, and the startup wrapper within the same 1,000-token budget. The recall agent now prefers durable pages over redundant logs for settled knowledge, and a live recall returned the current setup from the durable learning. An attempted full Pi CLI agent benchmark timed out after 240 seconds and produced no usable measurement, so it is not counted. An initial direct-subagent comparison used the same current-setup question and model with JABFA MCP-first retrieval versus explicitly forced legacy search/read. Both responses returned the correct MIT/wrapper/agent safeguards with durable citations. Wall time was 65.392 seconds for MCP and 50.896 seconds for legacy, so the MCP run was 28.5% slower end to end in this single sample. That MCP run also exposed an FTS5 parser failure for the ordinary hyphenated term `read-only`. A failing regression reproduced `no such column: only`; search now quotes each whitespace-separated input term as a literal FTS5 phrase, preserving implicit multi-term AND behavior without evaluating user input as FTS5 syntax. Search and context pass with the original query, and the existing repeated-match snippet regression remains green.

A clean post-fix direct-subagent pair repeated the identical question with the same `k3-256k` model. Each run made three assistant calls, encountered no tool errors, and returned the required MIT/wrapper/agent safeguards with durable citations. Provider usage was summed from the nested assistant messages persisted in the direct-subagent results. `totalTokens` includes input, output, cache reads, and cache writes; cache writes were zero in both runs.

| Metric | JABFA MCP-first | Forced legacy retrieval |
|---|---:|---:|
| Wall time | 32.628 s | 21.732 s |
| Input tokens (excluding cache reads) | 5,567 | 6,781 |
| Output tokens | 967 | 883 |
| Cache-read tokens | 15,179 | 15,213 |
| Total provider-reported tokens | 21,713 | 22,877 |

MCP reduced non-cache input by 17.9% and total provider-reported tokens by 5.1%, but produced 9.5% more output tokens and took 50.1% longer end to end. Therefore the 80% token-reduction target is **not met in this measured case**. This is one question and one run per mode, not a representative multi-question benchmark or evidence that the target is universally unattainable. LLM/provider time dominates total latency, and raw retrieval speedup must not be presented as end-to-end speedup. No source files were modified by the measured retrieval calls.

The post-fix synthetic baseline also retained the exact two required citations and one deduplicated section, with a 787-byte/197-estimated-token evidence payload. Its measured medians were 2.622 ms for search and 2.945 ms for context over 30 iterations.

## Subject-first discovery follow-up

After AT-21/AT-22, the same current-setup question was run through the updated brain-recall procedure: one `subjects` call, selection of the durable JABFA learning, then one `context` call restricted to that path. The investigation needed the 2,000-token budget to retain the later canary milestone; 1,200-token attempts could anchor on older milestones in the long single-H1 page. The measured run made three assistant calls and encountered no tool errors.

| Metric | Forced legacy | Previous section-first MCP | Subject-first MCP |
|---|---:|---:|---:|
| Wall time | 21.732 s | 32.628 s | 25.008 s |
| Input tokens (excluding cache reads) | 6,781 | 5,567 | 5,728 |
| Output tokens | 883 | 967 | 815 |
| Cache-read tokens | 15,213 | 15,179 | 12,544 |
| Total provider-reported tokens | 22,877 | 21,713 | 19,087 |

Subject-first retrieval used **12.1% fewer total tokens** and took **23.4% less wall time** than the previous section-first MCP workflow. It used 16.6% fewer tokens than forced legacy but remained 15.1% slower than legacy in this sample. The required MIT, startup-wrapper, agent-workflow, and canary facts were returned with durable citations. One historical seven-tool statement still appeared even though the current server has eight tools, so exact-current-state summarization of long historical pages remains imperfect.

These are sequential single-question dogfood runs, not a controlled repeated benchmark: the retrieval procedure and durable source entry changed between the earlier pair and this follow-up. They show a promising modest improvement, not the 80% target and not a general latency or token guarantee. Representative repeated questions remain necessary.

## Combined passage, prompt, and simple-recall optimizations

AT-23/AT-24 and their follow-up regressions added four-passage bold-milestone composition and required-path `investigate`. The recall agent was also reduced to four JABFA tools (`subjects`, `search`, `investigate`, `get_section`) and a 200-word response target. Multi-part/current-state investigations use a 2,000-token cap; focused investigations use 1,200. The real multi-part evidence contained the current subject-first, MIT/setup, retrieval-benchmark, and completed-canary milestones in 1,816 estimated tokens, with all required MIT/wrapper/canary facts returned correctly.

| Metric | Prior subject-first MCP | Optimized subject-first MCP |
|---|---:|---:|
| Wall time | 25.008 s | 16.769 s |
| Input tokens (excluding cache reads) | 5,728 | 2,677 |
| Output tokens | 815 | 644 |
| Cache-read tokens | 12,544 | 14,297 |
| Total provider-reported tokens | 19,087 | 17,618 |
| Evidence estimated tokens | 2,000 | 1,816 |

The combined changes used **7.7% fewer total tokens** and took **32.9% less wall time** than the immediately preceding subject-first implementation. Relative to the original section-first MCP sample, the reductions were 18.9% tokens and 48.6% wall time; relative to forced legacy, 23.0% tokens and 22.8% wall time. These are combined-effect measurements and do not isolate how much each optimization contributed. The answer was generated before the new nine-tool milestone was recorded and named the previous eight-tool inventory; the required license, wrapper, workflow, and canary facts were correct.

Opportunity 4 was also verified with the single-fact question **“What license does JABFA use?”** The agent answered MIT with a durable citation using only one `subjects` call followed by the final response—no investigation and two model calls total. It took **5.888 seconds** and used **10,214 provider-reported tokens** (828 input, 170 output, 9,216 cache-read). This demonstrates the shorter path directly, but different prompts and questions prevent treating that as a controlled percentage comparison with the complex run. The 80% target remains unmet and representative repeated questions are still required.
