#!/usr/bin/env python3
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for fix_worktree_symlinks.py. Hermetic: builds real throwaway git repositories (a repo with a
real submodule, then a real `git worktree add` of it) under a tempdir, never the real trees.

Run: python3 -m unittest test_fix_worktree_symlinks   (from the hams_open root)"""
import contextlib
import io
import os
import shutil
import subprocess
import tempfile
import unittest

import fix_worktree_symlinks as fws

GIT = ["git", "-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "protocol.file.allow=always"]


def run(args, cwd):
    return subprocess.run(GIT + args, cwd=cwd, capture_output=True, text=True, check=True).stdout.strip()


class FixWorktreeTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.tmp, True)
        shared = os.path.join(self.tmp, "shared_origin")
        os.makedirs(shared)
        run(["init", "-q", "--initial-branch=main"], shared)
        os.makedirs(os.path.join(shared, "tools"))
        with open(os.path.join(shared, "tools", "test.py"), "w") as handle:
            handle.write("MARK = 1\n")
        run(["add", "."], shared)
        run(["commit", "-q", "-m", "shared"], shared)

        self.main = os.path.join(self.tmp, "hams_open")
        os.makedirs(self.main)
        run(["init", "-q", "--initial-branch=main"], self.main)
        run(["submodule", "add", "-q", shared, "hams_shared"], self.main)
        os.symlink("hams_shared/tools", os.path.join(self.main, "tools"))  # like the real repo
        with open(os.path.join(self.main, "README.md"), "w") as handle:
            handle.write("x\n")
        run(["add", "."], self.main)
        run(["commit", "-q", "-m", "open"], self.main)

        self.worktree = os.path.join(self.main, ".claude", "worktrees", "w1")
        run(["worktree", "add", "-q", "-b", "w1", self.worktree], self.main)

    def test_a_fresh_worktree_has_an_empty_submodule_and_the_fix_makes_it_work(self):
        # Tests [@ANCHOR: fix_worktree_symlinks:fix_worktree]
        # Tests [@ANCHOR: fix_worktree_symlinks:main_checkout_root]
        # Tests [@ANCHOR: fix_worktree_symlinks:is_empty_dir]
        # Tests [@ANCHOR: fix_worktree_symlinks:is_populated_checkout]
        # Tests [@ANCHOR: fix_worktree_symlinks:link]
        # Tests [@ANCHOR: fix_worktree_symlinks:recorded_sha]
        # Tests [@ANCHOR: fix_worktree_symlinks:git]
        self.assertEqual(os.listdir(os.path.join(self.worktree, "hams_shared")), [])
        self.assertFalse(os.path.exists(os.path.join(self.worktree, "tools", "test.py")))
        fixed, messages = fws.fix_worktree(self.worktree)
        self.assertEqual(fixed, 1, messages)
        link = os.path.join(self.worktree, "hams_shared")
        self.assertTrue(os.path.islink(link))
        self.assertEqual(os.path.realpath(link), os.path.realpath(os.path.join(self.main, "hams_shared")))
        self.assertTrue(os.path.isfile(os.path.join(self.worktree, "tools", "test.py")))  # tools -> hams_shared/tools

    def test_the_link_is_skip_worktree_so_it_cannot_be_committed(self):
        fws.fix_worktree(self.worktree)
        self.assertTrue(run(["ls-files", "-v", "hams_shared"], self.worktree).startswith("S "))
        with open(os.path.join(self.worktree, "README.md"), "a") as handle:
            handle.write("change\n")
        run(["commit", "-q", "-a", "-m", "change"], self.worktree)
        self.assertEqual(run(["diff", "--name-only", "HEAD~1", "HEAD"], self.worktree), "README.md")
        self.assertEqual(run(["ls-tree", "HEAD", "hams_shared"], self.worktree).split()[0], "160000")

    def test_running_it_twice_changes_nothing(self):
        # Tests [@ANCHOR: fix_worktree_symlinks:fix_worktree]
        fws.fix_worktree(self.worktree)
        fixed, _ = fws.fix_worktree(self.worktree)
        self.assertEqual(fixed, 0)

    def test_the_main_checkout_is_left_alone(self):
        fixed, messages = fws.fix_worktree(self.main)
        self.assertEqual(fixed, 0)
        self.assertIn("main checkout", messages[0])
        self.assertFalse(os.path.islink(os.path.join(self.main, "hams_shared")))

    def test_a_main_checkout_with_no_populated_submodule_is_reported_not_guessed(self):
        shutil.rmtree(os.path.join(self.main, "hams_shared"))
        os.makedirs(os.path.join(self.main, "hams_shared"))
        fixed, messages = fws.fix_worktree(self.worktree)
        self.assertEqual(fixed, 0)
        self.assertTrue(any("not a populated submodule" in m for m in messages), messages)
        self.assertFalse(os.path.islink(os.path.join(self.worktree, "hams_shared")))

    def test_a_populated_submodule_in_the_worktree_is_not_replaced(self):
        run(["submodule", "update", "--init"], self.worktree)
        self.assertTrue(os.path.exists(os.path.join(self.worktree, "hams_shared", ".git")))
        fixed, _ = fws.fix_worktree(self.worktree)
        self.assertEqual(fixed, 0)
        self.assertFalse(os.path.islink(os.path.join(self.worktree, "hams_shared")))

    def test_main_fixes_the_worktree_it_is_run_in_and_reports_it(self):
        # Tests [@ANCHOR: fix_worktree_symlinks:main]
        old = os.getcwd()
        self.addCleanup(os.chdir, old)
        os.chdir(self.worktree)
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            self.assertEqual(fws.main(), 0)
        self.assertIn("Fixed hams_shared", out.getvalue())
        self.assertTrue(os.path.islink(os.path.join(self.worktree, "hams_shared")))


if __name__ == "__main__":
    unittest.main()
