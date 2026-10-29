#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Start a new bench_mcp instrument from the working skeleton.

Run from daemons/bench_mcp:  python3 tools/new_instrument.py acme

It copies the skeleton (src/example, its manual pages, its fixture and
its hardware-in-the-loop test) under the new name, then prints the
edits it leaves to you in src/config.rs and src/main.rs. It does not
touch any existing file, so it cannot break another instrument.
Guide: manual page concepts/extending, or docs/EXTENDING.md.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def names(word):
    return {
        "example": word,
        "Example": word.capitalize(),
        "EXAMPLE": word.upper(),
    }


def renamed(text, table):
    for old, new in table.items():
        text = text.replace(old, new)
    return text


def copy_tree(src, dst, table):
    for path in sorted(src.rglob("*")):
        if path.is_dir():
            continue
        rel = renamed(str(path.relative_to(src)), table)
        target = dst / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        text = path.read_text(encoding="utf-8")
        target.write_text(renamed(text, table), "utf-8")


def plan(word):
    return [
        (ROOT / "src/example", ROOT / f"src/{word}"),
        (
            ROOT / "manual/instruments/example",
            ROOT / f"manual/instruments/{word}",
        ),
        (
            ROOT / "tests/example_fixtures",
            ROOT / f"tests/{word}_fixtures",
        ),
    ]


def main(argv):
    if len(argv) != 2 or not re.fullmatch(
        r"[a-z][a-z0-9]{1,15}", argv[1]
    ):
        sys.exit(
            "usage: new_instrument.py NAME  (2 to 16 lower-case "
            "letters or digits, starting with a letter)"
        )
    word = argv[1]
    if word in ("example", "bench", "manual", "audio", "usb"):
        sys.exit(
            f"{word!r} is taken or reserved; "
            "choose the vendor or model name"
        )
    table = names(word)
    for src, dst in plan(word):
        if dst.exists():
            sys.exit(f"{dst} exists; nothing was copied")
    for src, dst in plan(word):
        copy_tree(src, dst, table)
    pages = ROOT / "manual/tools"
    for page in sorted(pages.glob("bench_example_*.md")):
        text = renamed(page.read_text("utf-8"), table)
        out = pages / renamed(page.name, table)
        out.write_text(text, encoding="utf-8")
    hil = ROOT / "tests/hil_example.rs"
    if hil.exists():
        text = renamed(hil.read_text("utf-8"), table)
        target = ROOT / f"tests/hil_{word}.rs"
        target.write_text(text, encoding="utf-8")
    print(NEXT.format(w=word, W=word.capitalize()))


NEXT = """Copied the skeleton as '{w}': src/{w}/,
manual/instruments/{w}/,
manual/tools/bench_{w}_*.md, tests/{w}_fixtures/, tests/hil_{w}.rs.

Now make these edits by hand (one line each, all marked by the
existing 'example' entries):
  src/config.rs  DRIVERS: add "{w}"
  src/config.rs  enum Kind: add {W}
  src/config.rs  struct Instrument: add  pub {w}: crate::{w}::Config,
  src/config.rs  parse(): driver match arm  "{w}" => Kind::{W},
  src/config.rs  parse(): the field, next to the example field
  src/config.rs  parse(): host/serial check, if your link needs one
  src/main.rs    mod {w};
  src/main.rs    connect(): arm for Kind::{W}
  src/main.rs    call(): if name.starts_with("bench_{w}_") {{ ... }}
  src/main.rs    tool_defs(): let {w} = {w}::tool_defs(insts); .chain
  src/main.rs    all_tool_defs(): one entry for driver "{w}"
Then: rewrite src/{w}/mod.rs for the real instrument, replace the
fixture with a scrubbed capture, rewrite every page (delete the
'model for' sentences), add a README row and a whats-new line, and
run the checks in concepts/extending/checks.
"""

if __name__ == "__main__":
    main(sys.argv)
