# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.addons.base.models.res_groups import ResGroups
from odoo.exceptions import AccessError
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestConfigSettings(HamsTransactionCase):
    """
    night_shift_todo.md "saving ANY Settings page can crash with an
    AccessError", full design writeup right after that entry: this file used
    to test a `user_websites_administrators_ids` Many2many field on
    res.config.settings whose get_values()/set_values() overrides read/wrote
    group_user_websites_administrator.user_ids directly, on every single
    Settings save, company-wide. That field and its overrides have been
    removed entirely (see res_config_settings.py's own comment for the full
    reasoning) rather than patched, because managing an arbitrary
    res.groups record's membership from inside res.config.settings was the
    wrong mechanism twice over: it fires unconditionally on every unrelated
    Settings save (the actual crash mechanism), and it required granting
    group_user_websites_administrator -- a content-moderation-tier role that
    does not imply base.group_system -- model-level access to the ENTIRE
    shared res.config.settings model to make the field reachable at all,
    which handed that role read/write on every OTHER module's settings too
    (proven concretely in test_04 below). These tests now guard the
    replacement design instead: group membership is managed through Odoo's
    own already-correctly-scoped Groups UI, and res.config.settings grants
    nothing beyond base.group_system.
    """

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

        self.websites_admin_user = self.env["res.users"].create(
            {
                "name": "Websites Admin Test",
                "login": "websitesadmintest",
                "email": "websitesadmintest@example.com",
                "website_slug": "websitesadmintest",
                "group_ids": [(6, 0, [self.admin_group.id])],
            }
        )

    def test_01_user_websites_administrators_ids_field_removed(self):
        """
        The field that used to drive the unconditional-every-save group
        write no longer exists on the shared res.config.settings model at
        all -- not just unused, genuinely gone, so no future module can
        collide with it or rediscover the same footgun by accident.
        """
        self.assertNotIn(
            "user_websites_administrators_ids",
            self.env["res.config.settings"]._fields,
            "user_websites_administrators_ids must be removed from "
            "res.config.settings, not merely deprecated -- it was the "
            "field driving the unconditional group_ids write.",
        )

    def test_02_admin_group_membership_still_manageable_via_res_groups(self):
        """
        The replacement path: a System Administrator manages
        group_user_websites_administrator's membership the same way Odoo
        expects ANY group's membership to be managed -- directly via
        res.groups (the "Groups" technical view, or the per-user Access
        Rights tab, since group_user_websites_administrator and
        group_user_websites_user share one res.groups.privilege). This is
        not new capability added by this fix; it already worked, and
        continues to, precisely because it was never routed through
        res.config.settings.set_values() in the first place.
        """
        self.assertNotIn(self.user_admin_test, self.admin_group.user_ids)

        self.admin_group.write({"user_ids": [(4, self.user_admin_test.id)]})
        self.assertIn(
            self.user_admin_test,
            self.admin_group.user_ids,
            "a System Administrator must still be able to grant this role "
            "directly via res.groups, the correctly-scoped replacement for "
            "the removed Settings field.",
        )

        self.admin_group.write({"user_ids": [(3, self.user_admin_test.id)]})
        self.assertNotIn(
            self.user_admin_test,
            self.admin_group.user_ids,
            "...and revoke it the same way.",
        )

    def test_03_settings_save_never_touches_group_membership_anymore(self):
        """
        Direct proof the crash mechanism is now structurally impossible, not
        just usually skipped (the previous, rejected patch's own "only
        write if changed" approach still called res.groups.write() on a
        real admin-list change -- see this test's own historical
        counterpart, test_03_set_values_still_writes_when_the_admin_list_
        actually_changes, deleted along with the mechanism it exercised).
        No installed module's set_values()/get_values() has any reason to
        call ResGroups.write() at all anymore; the same autospec/side_effect
        approach test_02_set_values_is_a_noop_when_the_admin_list_is_
        unchanged (this file's own prior version) had to use, and for the
        same reason (a real res.config.settings.set_values() call chains
        through every installed module, dwarfing a plain query-count
        ceiling).
        """
        write_mock = self.safe_patch_object(
            ResGroups,
            "write",
            autospec=True,
            new_callable=None,
            side_effect=ResGroups.write,
        )

        settings = self.env["res.config.settings"].create(
            {"global_website_page_limit": 250}
        )
        settings.set_values()

        write_mock.assert_not_called()

    def test_04_websites_admin_cannot_read_or_write_other_modules_settings(self):
        """
        The concrete severity proof behind this fix's own design writeup:
        before the access-csv row was deleted, group_user_websites_
        administrator -- a content-moderation role, not a System
        Administrator -- held model-level read/write on the ENTIRE shared
        res.config.settings model (ir.model.access.csv grants are per
        (model, group), never per field), which meant it could read and
        overwrite real credentials belonging to completely unrelated
        modules. distributed_redis_cache and cloudflare are both real
        dependencies of user_websites (see its own __manifest__.py), so
        their fields are guaranteed present on res.config.settings in this
        exact test run -- this is not a hypothetical field, it is
        distributed_redis_cache's actual Redis password field.
        """
        with self.assertRaises(
            AccessError,
            msg="a non-sysadmin User Websites Administrator must not be "
            "able to even instantiate res.config.settings, let alone "
            "read/write an unrelated module's redis_password field",
        ):
            self.env["res.config.settings"].with_user(
                self.websites_admin_user
            ).create({"redis_password": "attacker-supplied-value"})
