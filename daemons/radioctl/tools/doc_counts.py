#!/usr/bin/env python3
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Keeps the numbers in README.md and docs/manual/*.md equal to the data they come from.

A number that can drift from the repository (how many Hamlib models there are, how many have a
description, how many are hardware-verified) is written in prose between two markers:

    <!--count:described-->283<!--/count-->

This tool recomputes each named number from `docs/RIG_SUPPORT.md` (itself generated and checked by
`rig_support_matrix.py`) and `rigs/*.toml` (the number of facts, and per capability the number of
descriptions listing it, as `cap_read_frequency` for `read-frequency`), and compares it with the text between the markers.

Usage:
    doc_counts.py            rewrite every marked number that is wrong
    doc_counts.py --check    exit 1, naming each stale or unknown marker, and change nothing
"""
from __future__ import annotations

import argparse
import re
import sys
import tomllib
from collections import Counter
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
MARKER = re.compile(r"<!--count:([a-z_]+)-->(.*?)<!--/count-->")

# Each count: the line of the generated RIG_SUPPORT.md that carries it, with the number in group 1.
SUPPORT_LINES = {
    "models": r"^- Hamlib [0-9.]+ models: \*\*(\d+)\*\*",
    "not_applicable": r"^- Not applicable \(.*\): \*\*(\d+)\*\*",
    "real_radios": r"^- Real radios \(.*\): \*\*(\d+)\*\*",
    "described": r"with a radioctl description: \*\*(\d+)\*\*",
    "description_files": r"\((\d+) description files\)",
    "hardware_verified": r"^- Hardware-verified: \*\*(\d+)\*\*",
}


# [@ANCHOR: radioctl_doc_counts_compute]
def compute(root: Path = ROOT) -> dict[str, str]:
    """Every count a marker may name, as text; raises ValueError when RIG_SUPPORT.md lacks one."""
    support = (root / "docs" / "RIG_SUPPORT.md").read_text()
    counts: dict[str, str] = {}
    for name, pattern in SUPPORT_LINES.items():
        found = re.search(pattern, support, re.MULTILINE)
        if not found:
            raise ValueError(f"docs/RIG_SUPPORT.md has no line for the count '{name}'")
        counts[name] = found.group(1)
    # The number of facts is read from the descriptions themselves.
    facts = sum(p.read_text().count("\n[[facts]]") for p in (root / "rigs").glob("*.toml"))
    counts["facts"] = str(facts)
    # How many descriptions list each capability: `cap_<name>` with the hyphens of the name as underscores.
    capabilities: Counter[str] = Counter()
    for path in (root / "rigs").glob("*.toml"):
        capabilities.update(tomllib.loads(path.read_text()).get("capabilities", []))
    for name, number in capabilities.items():
        counts["cap_" + name.replace("-", "_")] = str(number)
    return counts


# [@ANCHOR: radioctl_doc_counts_pages]
def pages(root: Path = ROOT) -> list[Path]:
    """The pages that may carry markers: the README and every manual chapter."""
    return [root / "README.md", *sorted((root / "docs" / "manual").glob("*.md"))]


# [@ANCHOR: radioctl_doc_counts_check]
def check(root: Path = ROOT) -> list[str]:
    """One message per marker that is stale or names an unknown count."""
    counts = compute(root)
    problems = []
    for page in pages(root):
        for number, line in enumerate(page.read_text().splitlines(), 1):
            for name, text in MARKER.findall(line):
                where = f"{page.relative_to(root)}:{number}"
                if name not in counts:
                    problems.append(f"{where}: unknown count '{name}'")
                elif text != counts[name]:
                    problems.append(f"{where}: '{name}' is {text}, the data says {counts[name]}")
    return problems


# [@ANCHOR: radioctl_doc_counts_rewrite]
def rewrite(root: Path = ROOT) -> list[Path]:
    """Corrects every marked number in place; returns the pages that changed."""
    counts = compute(root)
    changed = []
    for page in pages(root):
        old = page.read_text()
        new = MARKER.sub(
            lambda m: f"<!--count:{m.group(1)}-->{counts.get(m.group(1), m.group(2))}<!--/count-->", old
        )
        if new != old:
            page.write_text(new)
            changed.append(page)
    return changed


# [@ANCHOR: radioctl_doc_counts_main]
def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("--check", action="store_true", help="exit 1 if a marked number is stale")
    args = parser.parse_args(argv)
    try:
        if args.check:
            problems = check()
            for problem in problems:
                print(problem, file=sys.stderr)
            return 1 if problems else 0
        for page in rewrite():
            print(f"rewrote {page.relative_to(ROOT)}")
        return 0
    except ValueError as error:
        print(error, file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
