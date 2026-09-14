# SPDX-License-Identifier: AGPL-3.0-or-later
# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from odoo import models, fields


class ResConfigSettings(models.TransientModel):
    _inherit = "res.config.settings"

    # night_shift_todo.md "saving ANY Settings page can crash with an
    # AccessError" (2026-09-13), full design writeup and investigation in the
    # follow-up section right after it: this module used to also carry a
    # `user_websites_administrators_ids` Many2many(res.users) field here,
    # with get_values()/set_values() overrides that read/wrote
    # `group_user_websites_administrator.user_ids` directly on every single
    # Settings save, company-wide, regardless of which page's fields
    # actually changed. That was removed entirely, not patched, because the
    # investigation converged on it being the wrong mechanism twice over:
    # (1) `res.config.settings` is one TransientModel Odoo's own MRO chains
    # through unconditionally on every installed module's own
    # get_values()/set_values() on every save -- writing a *security group's
    # membership* as a side effect of that shared, unconditional call chain
    # is what let an unrelated field's write (e.g. Redis config) cascade
    # into `mail`'s own discuss-channel resubscription and crash on a
    # zero-sudo secret-read AccessError; every other Settings-contributing
    # module in both repos only ever persists plain scalar
    # `config_parameter` values here, with no side effects on other models,
    # and that is the actual, coherent contract this model supports; and
    # (2) managing an arbitrary `res.groups` record's membership already has
    # a purpose-built, correctly-scoped Odoo mechanism -- the "Users &
    # Companies > Groups" technical view (`base.action_res_groups`) and the
    # per-user "Access Rights" tab (`group_user_websites_administrator` and
    # `group_user_websites_user` already share one `res.groups.privilege`,
    # so a System Administrator can already pick "Administrator" for the
    # "User Websites / Website Access" privilege straight from any user's
    # own form, no code required) -- neither of which is wired through
    # `set_values()`'s "fires on every unrelated save" contract, so neither
    # can reproduce this crash. Reusing that existing mechanism, instead of
    # re-inventing a narrower one inside Settings, was the actual fix.
    #
    # This also closed a second, independently real security gap the
    # investigation surfaced: `user_websites/security/ir.model.access.csv`
    # used to grant `group_user_websites_administrator` (a
    # content-moderation-tier role -- it does NOT imply `base.group_system`)
    # full model-level read/write/create/unlink access to
    # `res.config.settings` itself, so that role could reach the Settings
    # form at all. Odoo's `ir.model.access.csv` grants are per (model,
    # group), never per field, so that grant was never actually scoped to
    # this module's own 3 fields -- it handed out read/write on every field
    # any installed module merges onto this one shared TransientModel
    # (confirmed empirically in test_config_settings.py: a
    # `group_user_websites_administrator`-only user could read and
    # overwrite `distributed_redis_cache`'s `redis_password` and
    # `cloudflare`'s `cloudflare_api_token`, real credentials belonging to
    # entirely unrelated modules). The "General Settings" menu item itself
    # independently requires `base.group_system`
    # (`base_setup.menu_general_settings`), so this was never reachable
    # through normal UI navigation, but `ir.model.access.csv` is enforced by
    # the ORM regardless of menu visibility -- a direct RPC call from a
    # `group_user_websites_administrator` account (not a System
    # Administrator) could still reach it. That access-csv row has been
    # deleted; this role now has no access to `res.config.settings` at all,
    # matching every other non-`base.group_system` group in both repos.
    global_website_page_limit = fields.Integer(
        string="Global Page Limit",
        config_parameter="user_websites.global_website_page_limit",
        default=100,
        help="Default maximum number of web pages a standard user can create.",
    )

    company_abuse_email = fields.Char(
        string="Abuse Reporting Email",
        config_parameter="user_websites.company_abuse_email",
        help="Email address where content violation reports will be sent.",
    )
