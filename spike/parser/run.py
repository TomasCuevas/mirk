#!/usr/bin/env python3
"""Bounded driver for the pdfium-vs-lopdf spike (T6's parser decision).

Every measurement is a **process group**, and a run that exceeds its per-document budget has the whole
group killed. That detail is not decoration: stopping a job earlier in this project killed the job
wrapper but left its grandchild burning 6.6 CPU cores. A measurement rig that can leak a process is
worse than no measurement rig.

Ground truth is `pdfinfo` (poppler), an independent implementation, so agreement is evidence rather
than a tautology.

Usage:
    python3 run.py [--budget-seconds N] [--only-fixture]
"""

from __future__ import annotations

import json
import os
import signal
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
BINARY = os.path.join(HERE, "target", "release", "parser-spike")
PDFIUM_LIB_DIR = os.path.join(HERE, "vendor", "lib")
ARMS = ("lopdf", "pdfium")

# A per-document budget. The pathological documents are exactly the ones expected to hit it, so this
# number is the spike's definition of "did not finish", and it is applied identically to both arms.
PER_DOCUMENT_BUDGET_SECONDS = 60

# The user's own library. Read-only, never copied into the repository, and named here rather than
# committed as data: this list is the spike's input, not a project asset.
LIBRARY = "/home/tomas-cuevas/Documents/Mega/Libros"
DOCUMENTS = [
    ("pathological", "Programación/Learning Patterns - Lydia Hallie and Addy Osmani.pdf"),
    ("pathological", "Programación/gentleman-programming-book-es.pdf"),
    ("pathological", "Programación/Cracking the Coding Interview.pdf"),
    ("pathological", "Filosofía/Giovanni Reale/Historia del Pensamiento Filosofico y Cientifico III.pdf"),
    ("healthy", "Psicología/Carl G. Jung/Carl G. Jung - El Hombre y sus Simbolo.pdf"),
    ("healthy", "Programación/Grokking Data Structures.pdf"),
    ("healthy", "Novelas/Albert Camus/El extranjero - Albert Camus.pdf"),
]


def pdfinfo_pages(path: str) -> int | str:
    """The independent ground truth."""
    try:
        out = subprocess.run(["pdfinfo", path], capture_output=True, text=True, timeout=120)
    except subprocess.TimeoutExpired:
        return "pdfinfo-timeout"
    for line in out.stdout.splitlines():
        if line.startswith("Pages:"):
            return int(line.split()[1])
    return "pdfinfo-error"


def run_one(arm: str, path: str, budget: int) -> dict:
    """Runs one measurement in its own process group, killing the group if it overruns."""
    environment = dict(os.environ)
    environment["PDFIUM_LIB_DIR"] = PDFIUM_LIB_DIR
    started = time.time()
    process = subprocess.Popen(
        [BINARY, arm, path],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=environment,
        start_new_session=True,  # its own process group, so the whole tree can be killed
    )
    try:
        stdout, stderr = process.communicate(timeout=budget)
        timed_out = False
    except subprocess.TimeoutExpired:
        try:
            os.killpg(os.getpgid(process.pid), signal.SIGKILL)
        except ProcessLookupError:
            pass
        stdout, stderr = process.communicate()
        timed_out = True
    wall = time.time() - started

    result = {
        "arm": arm,
        "document": os.path.basename(path),
        "kind": kind_of(path),
        "size_mb": round(os.path.getsize(path) / 1e6, 2),
        "wall_s": round(wall, 1),
        "timed_out": timed_out,
    }
    line = (stdout or "").strip().splitlines()
    if timed_out:
        result["outcome"] = "TIMEOUT"
    elif line:
        try:
            parsed = json.loads(line[-1])
        except json.JSONDecodeError:
            result["outcome"] = f"unparseable: {line[-1][:60]}"
        else:
            result["pages"] = parsed.get("pages")
            result["cpu_s"] = round(parsed.get("cpu_ms", 0) / 1000, 1)
            result["peak_rss_mb"] = round(parsed.get("peak_rss_kb", 0) / 1024, 1)
            result["outcome"] = parsed.get("error", "ok")
    else:
        result["outcome"] = f"no output (exit {process.returncode}): {(stderr or '')[:60]}"
    return result


def kind_of(path: str) -> str:
    for kind, relative in DOCUMENTS:
        if os.path.join(LIBRARY, relative) == path:
            return kind
    return "fixture"


def fixture_run() -> None:
    """Cheap self-check of the harness on committed fixtures: no library access, no long work."""
    fixtures = os.path.join(
        HERE, "..", "..", "crates", "oct-scan", "tests", "fixtures"
    )
    for name, expected in (("pages-in-object-stream.pdf", 3), ("with-info-metadata.pdf", 4)):
        path = os.path.abspath(os.path.join(fixtures, name))
        for arm in ARMS:
            result = run_one(arm, path, 30)
            verdict = "OK" if result.get("pages") == expected else "MISMATCH"
            print(
                f"  self-check {verdict:8} {arm:7} {name:28} "
                f"pages={result.get('pages')} expected={expected} "
                f"wall={result['wall_s']}s outcome={result['outcome']}"
            )


def main() -> int:
    budget = PER_DOCUMENT_BUDGET_SECONDS
    arguments = sys.argv[1:]
    if "--budget-seconds" in arguments:
        budget = int(arguments[arguments.index("--budget-seconds") + 1])

    if not os.path.exists(BINARY):
        print(f"harness binary missing at {BINARY}; run: cargo build --release")
        return 1

    print("=== harness self-check on committed fixtures ===")
    fixture_run()

    if "--only-fixture" in arguments:
        return 0

    targets = [(kind, os.path.join(LIBRARY, relative)) for kind, relative in DOCUMENTS]
    missing = [path for _, path in targets if not os.path.exists(path)]
    if missing:
        print("missing inputs:")
        for path in missing:
            print(f"  {path}")
        return 1

    print()
    print(f"=== measured run: {len(targets)} documents x {len(ARMS)} parsers, "
          f"{budget}s budget each ===")
    results = []
    for kind, path in targets:
        expected = pdfinfo_pages(path)
        for arm in ARMS:
            result = run_one(arm, path, budget)
            result["kind"] = kind
            result["pdfinfo_pages"] = expected
            result["agrees_with_pdfinfo"] = result.get("pages") == expected
            results.append(result)
            print(
                f"  {arm:7} {kind:12} {result['size_mb']:7.2f} MB  "
                f"wall={result['wall_s']:5.1f}s cpu={result.get('cpu_s', '-'):>5} "
                f"rss={result.get('peak_rss_mb', '-'):>6} MB  "
                f"pages={result.get('pages', '-'):>5} (pdfinfo {expected})  "
                f"{result['outcome']}",
                flush=True,
            )

    output = os.path.join(HERE, "results.json")
    with open(output, "w") as handle:
        json.dump(results, handle, indent=2)
    print(f"\nwrote {output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
