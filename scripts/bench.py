#!/usr/bin/env python3
"""Benchmark fitd over every example and fixture.

Reports wall time, CPU time (all threads), solver checks, and chosen body font per file as a
Markdown table. With --check BASELINE it fails when solver checks, which are deterministic,
grow more than 25% over the baseline; with --time it also gates CPU time, which only makes
sense on the machine that recorded the baseline. --write BASELINE records a new one.

Usage: scripts/bench.py [--runs N] [--check FILE [--time]] [--write FILE]
"""
import argparse
import os
import re
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FITD = ROOT / "target" / "release" / "fitd"
LIMIT = 1.25


def run(src: Path, out: Path) -> dict:
    start = time.perf_counter()
    proc = subprocess.Popen([str(FITD), str(src), "-o", str(out)], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
    log = proc.stderr.read()
    _, status, usage = os.wait4(proc.pid, 0)
    wall = time.perf_counter() - start
    font = re.search(r"body font ([\d.]+)pt", log)
    checks = re.search(r"(\d+) checks", log)
    return {
        "wall": wall,
        "cpu": usage.ru_utime + usage.ru_stime,
        "checks": int(checks.group(1)) if checks else 0,
        "font": float(font.group(1)) if font else None,
        "ok": os.waitstatus_to_exitcode(status) == 0,
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--runs", type=int, default=3)
    ap.add_argument("--check")
    ap.add_argument("--time", action="store_true")
    ap.add_argument("--write")
    args = ap.parse_args()
    subprocess.run(["cargo", "build", "--release", "--quiet"], cwd=ROOT, check=True)
    files = sorted((ROOT / "examples").glob("*.fitd")) + sorted((ROOT / "tests" / "fixtures").glob("*.fitd"))
    rows = {}
    with tempfile.TemporaryDirectory() as tmp:
        for f in files:
            runs = [run(f, Path(tmp) / f"{f.stem}.svg") for _ in range(args.runs)]
            r = runs[0] | {"wall": statistics.median(x["wall"] for x in runs), "cpu": statistics.median(x["cpu"] for x in runs)}
            rows[f"{f.parent.name}/{f.name}"] = r
    print("| File | Wall s | CPU s | Checks | Font pt |")
    print("| --- | ---: | ---: | ---: | ---: |")
    for name, r in rows.items():
        font = f"{r['font']:.1f}" if r["ok"] and r["font"] else "fails (by design)" if not r["ok"] else "-"
        print(f"| `{name}` | {r['wall']:.2f} | {r['cpu']:.2f} | {r['checks']:,} | {font} |")
    if args.write:
        lines = [f"{n}\t{r['checks']}\t{r['cpu']:.3f}\n" for n, r in rows.items()]
        Path(args.write).write_text("# file\tchecks\tcpu_seconds\n" + "".join(lines))
    if args.check:
        bad = []
        for line in Path(args.check).read_text().splitlines():
            if line.startswith("#") or not line.strip():
                continue
            name, checks, cpu = line.split("\t")
            r = rows.get(name)
            if r is None:
                bad.append(f"{name}: missing")
                continue
            if r["checks"] > LIMIT * int(checks):
                bad.append(f"{name}: {r['checks']} checks, baseline {checks}")
            if args.time and r["cpu"] > LIMIT * float(cpu):
                bad.append(f"{name}: {r['cpu']:.2f} CPU s, baseline {cpu}")
        if bad:
            print("regressions over 25%:\n  " + "\n  ".join(bad), file=sys.stderr)
            return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
