# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


@tagged("post_install", "-at_install")
class TestPurgeCoversLanguageVariants(RealTransactionCase):
    # [@ANCHOR: cloudflare:test_purge_covers_language_variants]
    # Tests [@ANCHOR: cloudflare:COMM_language_variants]
    def test_unpublishing_a_page_purges_each_language_address(self):
        """A page is cached at the edge per language address; all of them must be purged."""
        french = self.env["res.lang"].with_context(active_test=False).search([("code", "=", "fr_FR")], limit=1)
        french.active = True
        website = self.env["website"].create(
            {
                "name": "Lang purge site",
                "domain": "https://lang-purge.example",
                "language_ids": [(6, 0, [website_lang.id for website_lang in (french | self.env.ref("base.lang_en"))])],
                "default_lang_id": self.env.ref("base.lang_en").id,
            }
        )
        view = self.env["ir.ui.view"].create(
            {"name": "Lang purge view", "type": "qweb", "arch_db": "<div>x</div>", "key": "test.lang_purge_view"}
        )
        page = self.env["website.page"].create(
            {"is_published": True, "url": "/lang-purge", "website_id": website.id, "view_id": view.id}
        )
        queue = self.env["cloudflare.purge.queue"]
        queue.search([], limit=10000).unlink()
        page.write({"is_published": False})
        targets = set(queue.search([("website_id", "=", website.id)]).mapped("target_item"))
        self.assertEqual(
            targets,
            {"https://lang-purge.example/lang-purge", f"https://lang-purge.example/{french.url_code}/lang-purge"},
        )
