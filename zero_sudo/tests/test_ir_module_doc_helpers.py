# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The three small `ir.module.module` helpers the documentation loader uses, called on the model itself."""
from odoo.tests.common import tagged

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestIrModuleDocHelpers(HamsTransactionCase):
    # Tests [@ANCHOR: zero_sudo:ir_module_is_internal_doc_path]
    # Tests [@ANCHOR: zero_sudo_ir_module_module_module]
    def test_stories_journeys_and_runbooks_directories_are_internal_paths(self):
        Module = self.env["ir.module.module"]
        for path in ("docs/stories/a.md", "docs/Journeys/a.md", "x\\runbooks\\a.md", "stories/deep/er/a.md"):
            self.assertTrue(Module._is_internal_doc_path(path), path)
        for path in ("data/documentation.html", "docs/guide.md", "stories.md", "docs/storiesx/a.md"):
            self.assertFalse(Module._is_internal_doc_path(path), path)

    # Tests [@ANCHOR: zero_sudo:ir_module_doc_audience_for]
    def test_the_model_classifies_a_manifest_entry_like_the_module_function(self):
        Module = self.env["ir.module.module"]
        self.assertEqual(Module._doc_audience_for("data/guide.html", {"name": "Guide", "public": True}), "member")
        self.assertEqual(Module._doc_audience_for("docs/stories/a.md", {"name": "A", "public": True}), "internal")
        self.assertEqual(Module._doc_audience_for("data/guide.html", {"name": "Guide"}), "internal")

    # Tests [@ANCHOR: zero_sudo:ir_module_markdown_doc_to_html]
    def test_markdown_becomes_sanitized_html(self):
        html = self.env["ir.module.module"]._markdown_doc_to_html(
            "# Title\n\nUse the `<h2>` tag.\n\n- one\n- two\n\n<script>window.bad=1</script>\n"
        )
        self.assertIn("<h1", html)
        self.assertIn("<li>one</li>", html)
        self.assertIn("&lt;h2&gt;", html)
        self.assertNotIn("<script", html)
