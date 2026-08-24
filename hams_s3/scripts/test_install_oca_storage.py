#!/usr/bin/env python3
# SPDX-License-Identifier: AGPL-3.0-or-later

"""
Unit tests for install_oca_storage.py's pure patch-application logic
(replace_in_file, replace_regex). Zero tests before this.

Scoped deliberately: main()'s own git-clone orchestration needs real
network access to github.com and isn't attempted here, matching this
codebase's own established caution about not building tests around
unverified network I/O. What's actually worth testing is the patch
logic itself -- this script's whole job is applying a fixed list of
text/regex replacements to real vendored OCA source (replacing .sudo()/
with_user(1) calls, adding audit-ignore/burn-ignore markers, adding
missing view name= attributes) so the result passes this platform's own
linters. A regex that silently fails to match (whitespace drift in a
new OCA release, for instance) would leave a real, unpatched .sudo()
call sitting in vendored code with nothing catching it -- exactly the
kind of thing worth pinning down with a real test.

replace_regex() used to be defined inside main() itself, unreachable
without either calling main() (which clones real repos over the
network) or re-implementing the function by hand in this test file
(which could silently drift from the real one). Hoisted to module level
in install_oca_storage.py itself as part of adding this coverage --
it's a pure function with no closure over main()'s locals, so the move
is a plain refactor, not a behavior change, and it's what makes real
import-based testing possible at all.
"""

import os
import tempfile
import unittest

import install_oca_storage as script


class ReplaceInFileTests(unittest.TestCase):
    def setUp(self):
        self.tmp_dir = tempfile.mkdtemp()
        self.path = os.path.join(self.tmp_dir, "sample.py")

    def test_applies_all_replacements_in_order(self):
        with open(self.path, "w") as f:
            f.write('backend = self.backend.sudo()\nother = self.backend.with_user(1)\n')

        script.replace_in_file(self.path, {
            "backend = self.backend.sudo()": "backend = self.backend",
            "other = self.backend.with_user(1)": "other = self.backend",
        })

        with open(self.path) as f:
            content = f.read()
        self.assertNotIn(".sudo()", content)
        self.assertNotIn("with_user(1)", content)
        self.assertEqual(content, "backend = self.backend\nother = self.backend\n")

    def test_a_pattern_that_does_not_match_leaves_the_file_unchanged_and_does_not_raise(self):
        # The real risk this script carries: a regex/string pattern that
        # silently fails to match (e.g. after an upstream OCA release
        # reformats the target line) leaves the "patch" a no-op with no
        # error anywhere. Confirmed as real, current behavior here --
        # not a bug this test is asserting should be fixed, a documented
        # fact this codebase's operators should know about the script
        # they're running.
        original = "backend = self.backend  # already clean\n"
        with open(self.path, "w") as f:
            f.write(original)

        script.replace_in_file(self.path, {"self.backend.sudo()": "self.backend"})

        with open(self.path) as f:
            self.assertEqual(f.read(), original)

    def test_a_missing_file_warns_and_does_not_raise(self):
        missing = os.path.join(self.tmp_dir, "does_not_exist.py")
        script.replace_in_file(missing, {"x": "y"})  # must not raise
        self.assertFalse(os.path.exists(missing))


class ReplaceRegexTests(unittest.TestCase):
    def setUp(self):
        self.tmp_dir = tempfile.mkdtemp()
        self.path = os.path.join(self.tmp_dir, "view.xml")

    def test_injects_a_name_field_after_each_matched_record_tag(self):
        with open(self.path, "w") as f:
            f.write(
                '<record id="view_storage_backend_form" model="ir.ui.view">\n'
                "    <field name=\"model\">storage.backend</field>\n"
                "</record>\n"
            )

        script.replace_regex(
            self.path,
            r'<record\s+id="([^"]+)"\s+model="ir\.ui\.view">',
            r'<record id="\1" model="ir.ui.view">\n        <field name="name">\1</field>\n        <!-- audit-ignore-view -->',
        )

        with open(self.path) as f:
            content = f.read()
        self.assertIn('<field name="name">view_storage_backend_form</field>', content)
        self.assertIn("<!-- audit-ignore-view -->", content)

    def test_multiple_matches_each_get_their_own_captured_id(self):
        with open(self.path, "w") as f:
            f.write(
                '<record id="view_one" model="ir.ui.view">\n</record>\n'
                '<record id="view_two" model="ir.ui.view">\n</record>\n'
            )

        script.replace_regex(
            self.path,
            r'<record\s+id="([^"]+)"\s+model="ir\.ui\.view">',
            r'<record id="\1" model="ir.ui.view">\n<field name="name">\1</field>',
        )

        with open(self.path) as f:
            content = f.read()
        self.assertIn('<field name="name">view_one</field>', content)
        self.assertIn('<field name="name">view_two</field>', content)


if __name__ == "__main__":
    unittest.main()
