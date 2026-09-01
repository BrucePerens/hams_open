# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.user_websites.hooks import post_init_hook


@tagged("post_install", "-at_install")
class TestWebsitePageNameDelegation(HamsTransactionCase):
    """
    Regression coverage for docs/proposals/
    COMPLIANCE_PAGE_NAME_LOST_WITH_USER_WEBSITES.md: stock website.page
    delegates `name` to `view_id.name` via `_inherits`; this module's own
    `name` field (via user_websites.owned.mixin) shadows that delegation,
    which -- confirmed via direct SQL against a real module install --
    silently loses a page's name two different ways depending on when the
    record was created relative to this module's own install turn. Both
    ways are covered here, matching the two-part fix in
    website_page.py::create() and hooks.py::post_init_hook.
    """

    # Tests [@ANCHOR: test_website_page_name_backfills_from_view_id]
    def test_create_without_name_backfills_from_linked_view(self):
        view = self.env["ir.ui.view"].create(
            {
                "name": "A Real View Name",
                "type": "qweb",
                "arch": "<t/>",
                "key": "user_websites.test_name_backfill_view",
            }
        )
        page = self.env["website.page"].create(
            {
                "url": "/test-name-backfill",
                "view_id": view.id,
            }
        )
        self.assertEqual(
            page.name,
            "A Real View Name",
            "website.page.create() should backfill `name` from the linked "
            "view when the caller doesn't supply one -- otherwise it falls "
            "back to this model's own unrelated default (self._description, "
            "i.e. literally \"Page\").",
        )

    def test_create_with_explicit_name_is_not_overridden_by_the_view(self):
        view = self.env["ir.ui.view"].create(
            {
                "name": "Internal Template Name",
                "type": "qweb",
                "arch": "<t/>",
                "key": "user_websites.test_name_explicit_view",
            }
        )
        page = self.env["website.page"].create(
            {
                "url": "/test-name-explicit",
                "view_id": view.id,
                "name": "Explicit Page Name",
            }
        )
        self.assertEqual(
            page.name,
            "Explicit Page Name",
            "An explicitly-given `name` must win over the linked view's "
            "own name -- the backfill should only apply when `name` is "
            "omitted entirely.",
        )

    # Tests [@ANCHOR: test_website_page_name_backfill_post_init_hook]
    def test_post_init_hook_backfills_existing_rows_with_empty_name(self):
        view = self.env["ir.ui.view"].create(
            {
                "name": "Pre-Existing View Name",
                "type": "qweb",
                "arch": "<t/>",
                "key": "user_websites.test_name_hook_view",
            }
        )
        page = self.env["website.page"].create(
            {
                "url": "/test-name-hook-backfill",
                "view_id": view.id,
                "name": "Will Be Wiped",
            }
        )
        # Simulate the real-world broken state this hook targets: a row
        # whose local `name` column never got populated at all, because it
        # was created by a module that loaded before user_websites in this
        # install's own module graph (see the doc above for the confirmed
        # mechanism) -- direct SQL, since going through the ORM would just
        # re-trigger this module's own create()/write() backfill logic.
        self.env.cr.execute(
            "UPDATE website_page SET name = NULL WHERE id = %s", (page.id,)
        )
        page.invalidate_recordset(["name"])
        self.assertFalse(page.name)

        post_init_hook(self.env)
        page.invalidate_recordset(["name"])
        self.assertEqual(
            page.name,
            "Pre-Existing View Name",
            "post_init_hook should backfill any website_page row with an "
            "empty local `name` from its linked view's own name.",
        )

    def test_post_init_hook_does_not_touch_rows_that_already_have_a_name(self):
        view = self.env["ir.ui.view"].create(
            {
                "name": "View's Own Name",
                "type": "qweb",
                "arch": "<t/>",
                "key": "user_websites.test_name_hook_noop_view",
            }
        )
        page = self.env["website.page"].create(
            {
                "url": "/test-name-hook-noop",
                "view_id": view.id,
                "name": "Deliberately Different Page Name",
            }
        )
        post_init_hook(self.env)
        page.invalidate_recordset(["name"])
        self.assertEqual(
            page.name,
            "Deliberately Different Page Name",
            "post_init_hook must only backfill rows with an empty local "
            "`name`, never overwrite one that's already set.",
        )
