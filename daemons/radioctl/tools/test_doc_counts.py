#!/usr/bin/env python3
# Copyright © Bruce Perens K6BP. All Rights Reserved.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Tests for doc_counts.py, including the check that the real README and manual are current."""
import tempfile
import unittest
from pathlib import Path

import doc_counts as dc

SUPPORT = """- Hamlib 4.6.2 models: **313**
- Not applicable (not a radio): **30**
- Real radios (models less not applicable): **283**; with a radioctl description: **281** (287 description files), **99%**
- Hardware-verified: **1**
"""


def tree(readme: str) -> Path:
    root = Path(tempfile.mkdtemp())
    (root / "docs" / "manual").mkdir(parents=True)
    (root / "rigs").mkdir()
    (root / "docs" / "RIG_SUPPORT.md").write_text(SUPPORT)
    (root / "rigs" / "a.toml").write_text('id = "a"\n\n[[facts]]\nkey = "x"\n\n[[facts]]\nkey = "y"\n')
    (root / "README.md").write_text(readme)
    return root


class DocCountsTests(unittest.TestCase):
    # Tests [@ANCHOR: radioctl_doc_counts_compute]
    def test_counts_are_read_from_the_generated_support_list_and_the_rig_files(self):
        counts = dc.compute(tree(""))
        self.assertEqual(
            counts,
            {"models": "313", "not_applicable": "30", "real_radios": "283", "described": "281",
             "description_files": "287", "hardware_verified": "1", "facts": "2"},
        )

    # Tests [@ANCHOR: radioctl_doc_counts_compute]
    def test_a_support_list_without_a_count_is_an_error(self):
        root = tree("")
        (root / "docs" / "RIG_SUPPORT.md").write_text("- Hamlib 4.6.2 models: **313**\n")
        with self.assertRaises(ValueError):
            dc.compute(root)

    # Tests [@ANCHOR: radioctl_doc_counts_pages]
    def test_the_pages_are_the_readme_then_the_chapters_in_order(self):
        root = tree("")
        (root / "docs" / "manual" / "b.md").write_text("")
        (root / "docs" / "manual" / "a.md").write_text("")
        self.assertEqual([p.name for p in dc.pages(root)], ["README.md", "a.md", "b.md"])

    # Tests [@ANCHOR: radioctl_doc_counts_check]
    def test_check_names_a_stale_and_an_unknown_marker_and_accepts_a_current_one(self):
        root = tree(
            "ok <!--count:models-->313<!--/count-->\n"
            "old <!--count:described-->287<!--/count--> and <!--count:nonsense-->1<!--/count-->\n"
        )
        problems = dc.check(root)
        self.assertEqual(len(problems), 2)
        self.assertIn("README.md:2: 'described' is 287, the data says 281", problems[0])
        self.assertIn("README.md:2: unknown count 'nonsense'", problems[1])

    # Tests [@ANCHOR: radioctl_doc_counts_rewrite]
    def test_rewrite_corrects_stale_markers_and_leaves_unknown_ones(self):
        root = tree("a <!--count:models-->1<!--/count--> b <!--count:nonsense-->9<!--/count-->\n")
        changed = dc.rewrite(root)
        self.assertEqual([p.name for p in changed], ["README.md"])
        self.assertEqual(
            (root / "README.md").read_text(),
            "a <!--count:models-->313<!--/count--> b <!--count:nonsense-->9<!--/count-->\n",
        )
        self.assertEqual(dc.rewrite(root), [])

    # Tests [@ANCHOR: radioctl_doc_counts_main]
    # Tests [@ANCHOR: radioctl_doc_counts_check]
    def test_the_committed_readme_and_manual_carry_current_numbers(self):
        # Fails when RIG_SUPPORT.md or a rig file changed and a marked number in the prose did not.
        self.assertEqual(dc.check(), [])
        self.assertEqual(dc.main(["--check"]), 0)


if __name__ == "__main__":
    unittest.main()
