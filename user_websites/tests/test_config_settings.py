# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.addons.base.models.res_groups import ResGroups
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestConfigSettings(HamsTransactionCase):

    def setUp(self):
        super(TestConfigSettings, self).setUp()
        self.admin_group = self.env.ref(
            "user_websites.group_user_websites_administrator"
        )

        self.user_admin_test = self.env["res.users"].create(
            {
                "name": "Settings Admin Test",
                "login": "settingsadmin",
                "email": "settingsadmin@example.com",
                "website_slug": "settingsadmin",
                "group_ids": [(6, 0, [])],
            }
        )

    def test_01_settings_sync_with_group(self):
        """
        Ensure that setting values in the ResConfigSettings TransientModel properly
        updates the underlying res.groups mapping, and vice-versa.
        """
        # Step 1: Add user via settings
        settings = self.env["res.config.settings"].create(
            {"user_websites_administrators_ids": [(4, self.user_admin_test.id)]}
        )
        # Tests [@ANCHOR: user_websites:COMM_settings_set_values]
        settings.set_values()

        # Verify user is now in the security group
        self.assertIn(
            self.user_admin_test,
            self.admin_group.user_ids,
            "User should be added to the Administrator group via settings.",
        )

        # Step 2: Read values back via settings
        new_settings = self.env["res.config.settings"].create({})
        # Tests [@ANCHOR: user_websites:COMM_settings_get_values]
        retrieved_values = new_settings.get_values()

        self.assertIn(
            self.user_admin_test.id,
            retrieved_values.get("user_websites_administrators_ids", [])[0][2],
            "get_values should accurately pull users from the Administrator group.",
        )

        # Step 3: Remove user via settings
        clear_settings = self.env["res.config.settings"].create(
            {"user_websites_administrators_ids": [(3, self.user_admin_test.id)]}
        )
        clear_settings.set_values()

        self.assertNotIn(
            self.user_admin_test,
            self.admin_group.user_ids,
            "User should be removed from the Administrator group via settings.",
        )

    def test_02_set_values_is_a_noop_when_the_admin_list_is_unchanged(self):
        """
        night_shift_todo.md "saving ANY Settings page can crash with an
        AccessError": res.groups.write() on user_ids unconditionally cascades
        through mail's own discuss-channel resubscription, which needs a real
        secret a service account is correctly never granted -- crashing the
        WHOLE settings-save request, for every Settings page, every time
        set_values() runs (i.e. constantly, since Odoo's own set_values()
        chains through every installed module's override on every single
        Settings save). This test proves the actual, most common case (no
        admin-list change at all) no longer calls write() -- the narrower,
        already-safe half of that finding's own recorded fix; the real
        architecture question (what to do when the list DOES change) is
        still open, and is NOT what this test covers.
        """
        # Tests [@ANCHOR: user_websites:COMM_settings_set_values]
        settings = self.env["res.config.settings"].create(
            {"user_websites_administrators_ids": [(4, self.user_admin_test.id)]}
        )
        settings.set_values()
        self.assertIn(self.user_admin_test, self.admin_group.user_ids)

        # A real, previously-tried, dead end worth recording so it isn't
        # retried: `self.env.cr.sql_log_count` before/after (the "prove zero
        # queries" pattern `safe_patch_object`'s own docstring suggests, and
        # `test_performance_regressions.py`'s own established precedent in
        # this exact module) does NOT isolate this specific write -- a real
        # `res.config.settings.set_values()` call chains through EVERY
        # installed module's own override (confirmed directly: 598 real
        # queries for one call on this test database, dwarfing anything a
        # reasonable ceiling could distinguish `res.groups.write()`'s own
        # contribution from). Patching `ResGroups.write` directly instead,
        # to prove the specific method was (or wasn't) called.
        #
        # `autospec=True` is required, not just `wraps=` -- without it,
        # patching an UNBOUND method on the class produces a plain Mock
        # that isn't a real descriptor, so `admin_group.write(vals)` never
        # gets `admin_group` bound as `self` before reaching `wraps`, and
        # the real `vals` argument silently lands in `self`'s own position
        # instead (`ResGroups.write() missing 1 required positional
        # argument: 'vals'`, confirmed by hitting this for real). But this
        # project's own `safe_patch_object` unconditionally defaults
        # `new_callable=DiagnosticMock` whenever `new`/`new_callable` isn't
        # already a kwarg, and `mock.patch.object` refuses `autospec` and a
        # real `new_callable` together -- passing `new_callable=None`
        # explicitly (satisfying the helper's own "already in kwargs" check
        # so it doesn't inject its default, and satisfying mock.patch's own
        # `new_callable is not None` guard) is what actually unlocks
        # `autospec=True` through this helper without bypassing it, also
        # confirmed by hitting the alternative failure first.
        write_mock = self.safe_patch_object(
            ResGroups,
            "write",
            autospec=True,
            new_callable=None,
            side_effect=ResGroups.write,
        )

        # Same admin list, set a second time -- the real-world common case
        # (every OTHER Settings page save, which still runs this method).
        same_settings = self.env["res.config.settings"].create(
            {"user_websites_administrators_ids": [(4, self.user_admin_test.id)]}
        )
        same_settings.set_values()

        write_mock.assert_not_called()
        self.assertIn(
            self.user_admin_test,
            self.admin_group.user_ids,
            "membership must still be correct even though no write happened",
        )

    def test_03_set_values_still_writes_when_the_admin_list_actually_changes(self):
        """Non-vacuousness for test_02 above: the skip-when-unchanged guard must
        never mask a real membership change."""
        # Tests [@ANCHOR: user_websites:COMM_settings_set_values]
        self.assertNotIn(
            self.user_admin_test,
            self.admin_group.user_ids,
            "sanity check: this user must not already be an admin before set_values() runs",
        )

        settings = self.env["res.config.settings"].create(
            {"user_websites_administrators_ids": [(4, self.user_admin_test.id)]}
        )
        settings.set_values()

        self.assertIn(
            self.user_admin_test,
            self.admin_group.user_ids,
            "a real admin-list change must still reach res.groups.write()",
        )
