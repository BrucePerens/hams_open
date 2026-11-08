#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Builds the pinned, isolated Python environment the bench server uses for the Glasgow Interface Explorer.

    nice -n 10 python3 -I install_glasgow.py --dest ~/.local/share/bench_mcp/glasgow-70cb38c9

What it does, in order, and nothing else:
 1. creates a venv at DEST (Python 3.13 or newer; Glasgow needs it);
 2. installs requirements.txt with `pip install --require-hashes --only-binary=:all:` (every package at an exact version, any file
    whose sha256 is not listed is refused);
 3. downloads Glasgow's source archive for the pinned commit, REFUSES it unless its sha256 equals ARCHIVE_SHA256, and installs it
    without dependencies and without build isolation (the build tool, pdm-backend, is in requirements.txt);
 4. writes DEST/INSTALLED.json (what was installed) and runs `glasgow --version`.
It never touches another venv, the system Python or /etc. The toolchain (YoWASP yosys and nextpnr) comes with the requirements; the
gateware bitstreams are built on first use of each applet configuration (about 5 to 20 s each on a 32-core box) and cached under
DEST/cache (the bench server sets XDG_CACHE_HOME there). Source of the archive: GitHub GlasgowEmbedded/glasgow (0BSD OR Apache-2.0).
"""
import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tarfile
import urllib.request
from pathlib import Path

COMMIT = "70cb38c9b03471acd2eabb76c82308891bec2ca5"
ARCHIVE_URL = f"https://codeload.github.com/GlasgowEmbedded/glasgow/tar.gz/{COMMIT}"
ARCHIVE_SHA256 = "d20f1910669c1a011ca020c4cf4eaa51135ea5abe7414c9f3b7c5cb6ba708139"
ARCHIVE_BYTES = 51826741
VERSION = "0.1.dev0+g70cb38c"
HERE = Path(__file__).resolve().parent


def sha256(path):
    # [@ANCHOR: glasgow_install_sha256]
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def run(cmd, **kw):
    # [@ANCHOR: glasgow_install_run]
    print("+", " ".join(map(str, cmd)), flush=True)
    subprocess.run(cmd, check=True, **kw)


def main(argv=None):
    # [@ANCHOR: glasgow_install_main]
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--dest", required=True, type=Path, help="directory to create (must not exist)")
    ap.add_argument("--python", default=sys.executable, help="a Python 3.13+ interpreter")
    ap.add_argument("--archive", type=Path, help="a copy of the source archive already on disk (still checked against its sha256)")
    a = ap.parse_args(argv)
    dest = a.dest.expanduser().resolve()
    if dest.exists():
        sys.exit(f"{dest} exists; remove it first (this script never reuses or modifies an existing environment)")
    try:
        os.nice(10)
    except OSError:
        pass
    dest.mkdir(parents=True)
    archive = a.archive
    if archive is None:
        archive = dest / "glasgow.tar.gz"
        print(f"downloading {ARCHIVE_URL} ({ARCHIVE_BYTES} bytes expected)", flush=True)
        with urllib.request.urlopen(ARCHIVE_URL, timeout=300) as r, open(archive, "wb") as f:
            shutil.copyfileobj(r, f)
    got = sha256(archive)
    if got != ARCHIVE_SHA256:
        shutil.rmtree(dest, ignore_errors=True)
        sys.exit(f"REFUSED: {archive} has sha256 {got}, expected {ARCHIVE_SHA256}. Nothing from it was installed. "
                 "GitHub does not promise stable archive bytes: a person must re-verify the commit and update the pin.")
    run([a.python, "-m", "venv", str(dest / "venv")])
    pip = [str(dest / "venv/bin/python"), "-m", "pip", "--disable-pip-version-check"]
    run(pip + ["install", "--require-hashes", "--only-binary=:all:", "-r", str(HERE / "requirements.txt")])
    src = dest / "src"
    with tarfile.open(archive) as t:
        t.extractall(src, filter="data")
    software = src / f"glasgow-{COMMIT}" / "software"
    env = dict(os.environ, PDM_BUILD_SCM_VERSION=VERSION)
    run(pip + ["install", "--no-deps", "--no-build-isolation", str(software)], env=env)
    (dest / "cache").mkdir()
    out = subprocess.run([str(dest / "venv/bin/glasgow"), "--version"], capture_output=True, text=True, check=True).stdout.strip()
    (dest / "INSTALLED.json").write_text(json.dumps({
        "glasgow_commit": COMMIT, "glasgow_version": out, "archive_sha256": got, "requirements_sha256": sha256(HERE / "requirements.txt"),
        "python": subprocess.run([str(dest / "venv/bin/python"), "--version"], capture_output=True, text=True).stdout.strip(),
    }, indent=2) + "\n")
    print(f"installed: glasgow {out} at {dest}")


if __name__ == "__main__":
    main()
