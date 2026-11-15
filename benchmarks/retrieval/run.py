#!/usr/bin/env python3
import json
import os
from pathlib import Path
import re
import shutil
import statistics
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
FIXTURE = ROOT / "benchmarks" / "retrieval" / "vault"
BINARY = Path(os.environ.get("JABFA_BIN", ROOT / "target" / "debug" / "jabfa"))
QUERY = "brain agents"
EXPECTED = {"agent-architecture.md", "local-index.md"}
ITERATIONS = 30


def run(command):
    started = time.perf_counter_ns()
    result = subprocess.run(command, check=False, capture_output=True)
    elapsed_ms = (time.perf_counter_ns() - started) / 1_000_000
    if result.returncode != 0:
        raise RuntimeError(result.stderr.decode("utf-8", errors="replace"))
    return json.loads(result.stdout), elapsed_ms


def summarize(samples):
    return {
        "median_ms": round(statistics.median(samples), 3),
        "min_ms": round(min(samples), 3),
        "max_ms": round(max(samples), 3),
        "iterations": len(samples),
    }


def main():
    if not BINARY.is_file():
        raise SystemExit(f"build the CLI first; binary not found: {BINARY}")
    with tempfile.TemporaryDirectory(prefix="jabfa-retrieval-baseline-") as temporary:
        temporary = Path(temporary)
        vault = temporary / "vault"
        shutil.copytree(FIXTURE, vault)
        database = temporary / "index.sqlite3"
        base = [str(BINARY)]
        index, _ = run(base + ["index", str(vault), "--database", str(database), "--json"])
        if index["documents_indexed"] != 6:
            raise AssertionError(index)

        search_command = base + [
            "search", str(vault), QUERY, "--database", str(database), "--json"
        ]
        context_command = base + [
            "context", str(vault), QUERY, "--database", str(database),
            "--max-tokens", "1000", "--json"
        ]
        for _ in range(3):
            run(search_command)
            run(context_command)
        search_times = [run(search_command)[1] for _ in range(ITERATIONS)]
        context_times = []
        response = None
        for _ in range(ITERATIONS):
            response, elapsed = run(context_command)
            context_times.append(elapsed)

        citations = re.findall(r"Path: `([^`]+)`", response["context"])
        if set(citations) != EXPECTED or len(citations) != 2:
            raise AssertionError({"citations": citations, "expected": sorted(EXPECTED)})
        if response["included"] != 2 or response["deduplicated"] != 1:
            raise AssertionError(response)
        source_bytes = sum(path.stat().st_size for path in FIXTURE.rglob("*.md"))
        context_bytes = len(response["context"].encode("utf-8"))
        report = {
            "query": QUERY,
            "documents": index["documents_indexed"],
            "source_bytes": source_bytes,
            "context_bytes": context_bytes,
            "estimated_tokens": response["estimated_tokens"],
            "included": response["included"],
            "deduplicated": response["deduplicated"],
            "source_bytes_reduced_percent": round(100 * (1 - context_bytes / source_bytes), 1),
            "search_process_latency": summarize(search_times),
            "context_process_latency": summarize(context_times),
            "citations": sorted(citations),
            "legacy_agent_baseline": "not measured",
        }
        print(json.dumps(report, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
