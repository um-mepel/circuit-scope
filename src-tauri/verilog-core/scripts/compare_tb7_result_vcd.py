#!/usr/bin/env python3
"""
Compare top-level TestBench7 `Result [10:0]` between two VCDs.

Resolves the $var in scope stack [TestBench7] only (not ffc's Result).

  python3 compare_tb7_result_vcd.py P7.vcd circuit_scope.vcd
"""
from __future__ import annotations

import re
import sys
from pathlib import Path


def find_result_id(vcd: str, want_stack: tuple[str, ...]) -> str:
    """
    `want_stack` e.g. ("TestBench7",) for the pad output, or ("TestBench7", "ffc") for the DUT Result.
    """
    stack: list[str] = []
    for line in vcd.splitlines():
        t = line.strip()
        if t.startswith("$scope module "):
            name = t.replace("$scope module ", "").replace(" $end", "").strip()
            stack.append(name)
        elif t == "$upscope $end" and stack:
            stack.pop()
        elif (
            t.startswith("$var ")
            and tuple(stack) == want_stack
            and "Result" in t
        ):
            parts = t.split()
            if len(parts) >= 5 and parts[4] == "Result":
                return parts[3]
    raise ValueError(f"Result not found for stack {want_stack!r}")


def find_tb7_result_id(vcd: str) -> str:
    return find_result_id(vcd, ("TestBench7",))


def collect_result_changes(vcd: str, code: str) -> list[tuple[int, int, int]]:
    """List of (time, int_value, line_1based) in file order for b... <code> after $enddefinitions."""
    out: list[tuple[int, int, int]] = []
    t = 0
    started = False
    for i, line in enumerate(vcd.splitlines(), 1):
        s = line.strip()
        if not started:
            if s.startswith("$enddefinitions"):
                started = True
            continue
        if s.startswith("#"):
            m = re.match(r"#(\d+)", s)
            if m:
                t = int(m.group(1))
            continue
        if s.startswith("b") and s.count(" ") >= 1:
            sp = s.split()
            if len(sp) >= 2 and sp[1] == code:
                bits = sp[0][1:]
                if bits and all(c in "01" for c in bits):
                    out.append((t, int(bits, 2), i))
    return out


def all_timestamps(vcd: str) -> set[int]:
    times: set[int] = set()
    started = False
    for line in vcd.splitlines():
        s = line.strip()
        if not started:
            if s.startswith("$enddefinitions"):
                started = True
            continue
        if s.startswith("#"):
            m = re.match(r"#(\d+)", s)
            if m:
                times.add(int(m.group(1)))
    return times


def collapse(changes: list[tuple[int, int, int]]) -> list[tuple[int, int]]:
    """Order by (time, line); keep last value at each time (multiple b lines at same #)."""
    ch = sorted(changes, key=lambda x: (x[0], x[2]))
    out: list[tuple[int, int]] = []
    for t, v, _ in ch:
        if out and out[-1][0] == t:
            out[-1] = (t, v)
        else:
            out.append((t, v))
    return out


def value_after_timeline(timeline: list[tuple[int, int]], t_query: int) -> int | None:
    """Last value whose event time is <= t_query (monotone times in timeline)."""
    last: int | None = None
    for t, v in timeline:
        if t <= t_query:
            last = v
        else:
            break
    return last


def run_compare(
    name: str, t1: str, t2: str, c1: str, c2: str, a_name: str, b_name: str
) -> int:
    tl1 = collapse(collect_result_changes(t1, c1))
    tl2 = collapse(collect_result_changes(t2, c2))
    if not tl1 or not tl2:
        print(f"{name}: error: one VCD has no b-format changes (ids {c1!r} / {c2!r})", file=sys.stderr)
        return 1

    times = all_timestamps(t1) | all_timestamps(t2)
    mism: list[tuple[int, int | None, int | None]] = []
    for t in sorted(times):
        v1 = value_after_timeline(tl1, t)
        v2 = value_after_timeline(tl2, t)
        if v1 is None or v2 is None:
            continue
        if v1 != v2:
            mism.append((t, v1, v2))

    print(
        f"[{name}] {a_name} id={c1!r} vs {b_name} id={c2!r} | #times union={len(times)}; "
        f"change events: {len(tl1)} / {len(tl2)} | mismatches: {len(mism)}"
    )
    if mism:
        for row in mism[:40]:
            print(f"  t={row[0]}\t{a_name}={row[1]}\t{b_name}={row[2]}")
        if len(mism) > 40:
            print(f"  ... {len(mism) - 40} more")
        return 1
    return 0


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: compare_tb7_result_vcd.py <P7.vcd> <circuit_scope.vcd>", file=sys.stderr)
        return 2
    p1, p2 = Path(sys.argv[1]), Path(sys.argv[2])
    t1, t2 = p1.read_text(), p2.read_text()

    rc = 0
    c1, c2 = find_tb7_result_id(t1), find_tb7_result_id(t2)
    rc |= run_compare("TestBench7.Result", t1, t2, c1, c2, p1.name, p2.name)

    try:
        f1, f2 = find_result_id(t1, ("TestBench7", "ffc")), find_result_id(t2, ("TestBench7", "ffc"))
        rc |= run_compare("ffc.Result", t1, t2, f1, f2, p1.name, p2.name)
    except ValueError as e:
        print(f"ffc.Result: skip ({e})")

    if rc == 0:
        print("OK: all compared Result signals match on every # timestep (both had prior b-change).")
    return rc


if __name__ == "__main__":
    raise SystemExit(main())
