#!/usr/bin/env python3
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""
Gives a hams_open git worktree a working `hams_shared`.

`hams_shared` is a git SUBMODULE of hams_open (a mode-160000 entry). `git worktree add` creates
the submodule directory but does not populate it, so inside every worktree under
`.claude/worktrees/<name>/` it is an EMPTY directory. Everything that reaches into it then fails:
the `agents`, `AGENTS.md`, `docs` and `tools` symlinks dangle (they point at `hams_shared/...`),
and a test run dies on `import site_monitor` (pager_duty) and the like, because the shared
modules and test harness are not there.

This script replaces the empty directory with a symlink to the MAIN checkout's `hams_shared`
(found through `git rev-parse --git-common-dir`, so it works however deeply the worktree is
nested), exactly as the hams_com repo's own `fix_worktree_symlinks.py` does for its `hams_shared`
symlink. It also relinks any git-tracked symlink that dangles here but resolves in the main
checkout.

The change is deliberately LOCAL and UNCOMMITTED. The entry is marked `skip-worktree`, so
`git commit -a` / `git add -A` can never turn the submodule into a machine-specific symlink and
commit it (`git update-index --no-skip-worktree hams_shared` undoes it). `git status` may still show
`hams_shared` as changed; do not commit it.

Because the link points at the main checkout's working tree, the worktree sees whatever commit that
checkout has (it may differ from the commit this worktree records for the submodule). The script says
so when they differ. To test against a hams_shared branch, make a worktree of
hams_open/hams_shared and replace the link by hand.

Safe to run from the main checkout (nothing to do) and idempotent. Usage, from anywhere inside the
worktree: python3 fix_worktree_symlinks.py
"""
import os
import subprocess
import sys

GITLINK_MODE = "160000"
SYMLINK_MODE = "120000"


def _git(args, cwd):
    return subprocess.run(
        ["git"] + args, cwd=cwd, capture_output=True, text=True, check=True
    ).stdout.strip()


def main_checkout_root(this_root):
    """The root of the main (non-worktree) checkout that `this_root` belongs to."""
    main_git_dir = _git(["rev-parse", "--git-common-dir"], this_root)
    if not os.path.isabs(main_git_dir):
        main_git_dir = os.path.join(this_root, main_git_dir)
    return os.path.dirname(os.path.normpath(main_git_dir))


def _is_empty_dir(path):
    return os.path.isdir(path) and not os.path.islink(path) and not os.listdir(path)


def _is_populated_checkout(path):
    """A submodule working tree: a directory holding a `.git` file or directory."""
    return os.path.isdir(path) and os.path.exists(os.path.join(path, ".git"))


def _link(this_root, rel_path, target):
    here_path = os.path.join(this_root, rel_path)
    if os.path.islink(here_path):
        os.remove(here_path)
    elif os.path.isdir(here_path):
        os.rmdir(here_path)  # only ever called for an empty directory
    os.symlink(target, here_path)
    # skip-worktree: `git commit -a` / `git add -A` can never commit the absolute, machine-specific
    # target (the same protection the hams_com script gives its symlinks).
    _git(["update-index", "--skip-worktree", rel_path], this_root)


def _recorded_sha(entry_meta):
    return entry_meta.split()[1]


def fix_worktree(this_root):
    """Fix `this_root`. Returns (fixed, messages)."""
    main_root = main_checkout_root(this_root)
    if os.path.normpath(main_root) == os.path.normpath(this_root):
        return 0, ["Running in the main checkout -- nothing to fix."]

    messages = []
    fixed = 0
    # Gitlinks first: the symlinks agents/, tools/, ... resolve through hams_shared.
    entries = [line.partition("\t")[::2] for line in _git(["ls-files", "-s"], this_root).splitlines()]
    for meta, rel_path in sorted(entries, key=lambda e: e[0].split()[0] != GITLINK_MODE):
        mode = meta.split()[0]
        here_path = os.path.join(this_root, rel_path)
        main_path = os.path.join(main_root, rel_path)
        if mode == GITLINK_MODE:
            if not (_is_empty_dir(here_path) or (os.path.islink(here_path) and not os.path.exists(here_path))):
                continue  # populated, or already linked to something that works
            if not _is_populated_checkout(main_path):
                messages.append(f"Skipping {rel_path}: the main checkout's {main_path} is not a populated submodule.")
                continue
            real_target = os.path.realpath(main_path)
            _link(this_root, rel_path, real_target)
            fixed += 1
            messages.append(f"Fixed {rel_path} -> {real_target} (skip-worktree set)")
            try:
                main_sha = _git(["rev-parse", "HEAD"], real_target)
            except subprocess.CalledProcessError:
                main_sha = ""
            if main_sha and main_sha != _recorded_sha(meta):
                messages.append(
                    f"  note: this worktree records {rel_path} at {_recorded_sha(meta)[:9]} but the main "
                    f"checkout has {main_sha[:9]} checked out; you are testing against the latter."
                )
        elif mode == SYMLINK_MODE:
            if os.path.exists(here_path):
                continue
            real_target = os.path.realpath(main_path)
            if not os.path.exists(real_target):
                messages.append(f"Skipping {rel_path}: broken in the main checkout too ({main_path} -> {real_target}).")
                continue
            _link(this_root, rel_path, real_target)
            fixed += 1
            messages.append(f"Fixed {rel_path} -> {real_target} (skip-worktree set)")

    if fixed == 0:
        messages.append("No empty submodules or broken cross-repo symlinks found.")
    else:
        messages.append(f"\nFixed {fixed} entr{'y' if fixed == 1 else 'ies'} locally, uncommitted (do not commit).")
    return fixed, messages


def main():
    this_root = _git(["rev-parse", "--show-toplevel"], os.getcwd())
    _, messages = fix_worktree(this_root)
    print("\n".join(messages))
    return 0


if __name__ == "__main__":
    sys.exit(main())
