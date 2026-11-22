#!/usr/bin/env python3
"""Fails CI on any CodeQL finding that is not waived (D-0015, D-0185).

Two waivers exist:
  - D-0015 amendment (owner-approved): rust/hard-coded-cryptographic-value in files under a
    tests/ directory, where published test vectors use fixed values by design.
  - D-0185: single reviewed source lines listed in .github/codeql-waivers.json, matched by rule,
    file and the exact (trimmed) text of the flagged line. A new or edited line fails again.

Usage: codeql-gate.py <sarif> [<repository root>]
"""

import json
import re
import sys
from pathlib import Path

TESTS_RULE = "rust/hard-coded-cryptographic-value"
TESTS_DIR = re.compile(r"(^|/)tests/")


def location(result):
    loc = (result.get("locations") or [{}])[0].get("physicalLocation", {})
    uri = loc.get("artifactLocation", {}).get("uri", "?")
    line = loc.get("region", {}).get("startLine", 1)
    return uri, line


def source_line(root, uri, line):
    try:
        lines = (root / uri).read_text(encoding="utf-8").splitlines()
    except OSError:
        return None
    return lines[line - 1].strip() if 0 < line <= len(lines) else None


def main():
    if len(sys.argv) not in (2, 3):
        print(__doc__, file=sys.stderr)
        return 2
    sarif = json.loads(Path(sys.argv[1]).read_text(encoding="utf-8"))
    root = Path(sys.argv[2] if len(sys.argv) == 3 else ".")
    waivers = json.loads((root / ".github/codeql-waivers.json").read_text(encoding="utf-8"))
    listed = {(w["rule"], w["path"], w["line"]) for w in waivers["waivers"]}
    used = set()
    failed = 0
    for run in sarif.get("runs", []):
        for r in run.get("results") or []:
            rule = r.get("ruleId", "?")
            # An annotation is one line: CodeQL's messages list related locations on more.
            text = " ".join(r.get("message", {}).get("text", "").split())
            uri, line = location(r)
            if rule == TESTS_RULE and TESTS_DIR.search(uri):
                print(f"::notice file={uri},line={line}::waived (D-0015, tests/ only): {rule}: {text}")
                continue
            key = (rule, uri, source_line(root, uri, line))
            if key in listed:
                used.add(key)
                print(f"::notice file={uri},line={line}::waived (D-0185, reviewed line): {rule}: {text}")
                continue
            print(f"::error file={uri},line={line}::{rule}: {text}")
            failed += 1
    # A waiver whose line no longer exists is dead weight: say so, without failing.
    for rule, path, text in sorted(listed - used):
        print(f"::warning file={path}::unused CodeQL waiver ({rule}): {text}")
    if failed:
        print(f"CodeQL reported {failed} finding(s).", file=sys.stderr)
        return 1
    print("CodeQL: no unwaived findings.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
