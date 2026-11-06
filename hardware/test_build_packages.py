#!/usr/bin/env python3
# Copyright © Bruce Perens K6BP. All Rights Reserved.
"""Tests of hardware/build_packages.sh: a throwaway git repository holding a copy of the script and one tiny native
package. Checks that the .deb, .dsc and .tar.xz land in the output directory with SHA256SUMS, and that a file git does
not track (an untracked file, or one .gitignore excludes such as a *.patch) is NOT in the source tarball, so a package
that only builds on a machine with leftovers fails or is caught here. Run: python3 -m unittest hardware/test_build_packages.py
"""
import os
import shutil
import subprocess
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "build_packages.sh")


def _write(path, text, mode=0o644):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(text)
    os.chmod(path, mode)


@unittest.skipUnless(shutil.which("dpkg-buildpackage") and shutil.which("git") and shutil.which("dh"), "needs dpkg-buildpackage, debhelper, git")
class TestBuildPackages(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.repo = os.path.join(self.tmp, "repo")
        hw = os.path.join(self.repo, "hardware")
        os.makedirs(hw)
        shutil.copy(SCRIPT, os.path.join(hw, "build_packages.sh"))
        pkg = os.path.join(hw, "demo_laptop")
        _write(os.path.join(pkg, "tool.sh"), "#!/bin/sh\necho hi\n", 0o755)
        _write(os.path.join(pkg, "debian/control"),
               "Source: demo-laptop\nSection: misc\nPriority: optional\nMaintainer: Test <t@example.com>\n"
               "Build-Depends: debhelper-compat (= 13)\nStandards-Version: 4.7.0\nRules-Requires-Root: no\n\n"
               "Package: demo-laptop-support\nArchitecture: all\nDescription: demo\n demo package\n")
        _write(os.path.join(pkg, "debian/rules"), "#!/usr/bin/make -f\n%:\n\tdh $@\n", 0o755)
        _write(os.path.join(pkg, "debian/changelog"),
               "demo-laptop (1.0.0) unstable; urgency=medium\n\n  * Test.\n\n -- Test <t@example.com>  Thu, 05 Nov 2026 00:00:00 +0000\n")
        _write(os.path.join(pkg, "debian/copyright"), "Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/\n\nFiles: *\nCopyright: Test\nLicense: GPL-3+\n")
        _write(os.path.join(pkg, "debian/install"), "tool.sh usr/bin\n")
        _write(os.path.join(pkg, "debian/source/format"), "3.0 (native)\n")
        _write(os.path.join(self.repo, ".gitignore"), "*.patch\n")
        env = dict(os.environ, GIT_AUTHOR_NAME="t", GIT_AUTHOR_EMAIL="t@e.com", GIT_COMMITTER_NAME="t", GIT_COMMITTER_EMAIL="t@e.com")
        subprocess.run(["git", "init", "-q", self.repo], check=True)
        subprocess.run(["git", "-C", self.repo, "add", "."], check=True)
        subprocess.run(["git", "-C", self.repo, "commit", "-q", "-m", "x"], check=True, env=env)
        _write(os.path.join(pkg, "leftover.txt"), "untracked\n")
        _write(os.path.join(pkg, "leak.patch"), "ignored\n")
        self.out = os.path.join(self.tmp, "out")

    def build(self):
        return subprocess.run([os.path.join(self.repo, "hardware/build_packages.sh"), self.out], capture_output=True, text=True)

    def test_builds_binary_and_source_packages_with_checksums(self):
        r = self.build()
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertEqual(sorted(os.listdir(self.out)),
                         ["SHA256SUMS", "demo-laptop-support_1.0.0_all.deb", "demo-laptop_1.0.0.dsc", "demo-laptop_1.0.0.tar.xz"])
        sums = subprocess.run(["sha256sum", "-c", "SHA256SUMS"], cwd=self.out, capture_output=True, text=True)
        self.assertEqual(sums.returncode, 0, sums.stdout + sums.stderr)

    def test_untracked_and_ignored_files_are_not_in_the_source_tarball(self):
        self.assertEqual(self.build().returncode, 0)
        names = subprocess.run(["tar", "-tJf", os.path.join(self.out, "demo-laptop_1.0.0.tar.xz")], capture_output=True, text=True, check=True).stdout
        self.assertIn("tool.sh", names)
        self.assertNotIn("leftover.txt", names)
        self.assertNotIn("leak.patch", names)

    def test_a_directory_without_debian_is_an_error(self):
        os.makedirs(os.path.join(self.repo, "hardware/nodeb"))
        r = subprocess.run([os.path.join(self.repo, "hardware/build_packages.sh"), self.out, "nodeb"], capture_output=True, text=True)
        self.assertNotEqual(r.returncode, 0)
        self.assertIn("has no debian/", r.stderr)


if __name__ == "__main__":
    unittest.main()
