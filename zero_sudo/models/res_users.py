# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later

import secrets

from odoo import api, fields, models
from odoo.addons.distributed_redis_cache.redis_cache import notify_model_invalidation


class ResUsersZeroSudo(models.Model):
    _inherit = "res.users"

    security_log_ids = fields.One2many(
        "zero_sudo.security.log",
        "user_id",
        string="Security Logs",
    )

    is_service_account = fields.Boolean(
        # [@ANCHOR: zero_sudo:COMM_is_service_account_field]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_is_service_account_field]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_story_login_blocking]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_journey_service_account_lifecycle]
        string="Is Service Account",
        default=False,
        help=(
            "Flags this user as an internal service account. "
            "Prevents interactive web logins."
        ),
        groups="base.group_system",
    )

    @api.model_create_multi
    def create(self, vals_list):
        # [@ANCHOR: zero_sudo:COMM_service_account_password_generation]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_COMM_test_service_account_password]
        for vals in vals_list:
            if vals.get("is_service_account"):
                # Ensure no password for service accounts
                vals.pop("password", None)
                vals["password"] = secrets.token_hex(32)
        return super().create(vals_list)

    # [@ANCHOR: zero_sudo:res_users_write]
    def write(self, vals):
        # Bug-hunt fix: previously only forced a random password when
        # is_service_account was being set to a TRUTHY value (becoming a
        # service account). De-designating one (is_service_account: False)
        # while ALSO supplying a real password in the SAME call skipped
        # this branch (its value isn't truthy) and skipped the elif branch
        # below too (it requires "is_service_account" not in vals at all)
        # -- falling through to a plain super().write(vals) that applied
        # the caller-chosen password as-is, silently restoring a loginable
        # credential on a record that had been a service account moments
        # earlier. Any transition of this field, in either direction, now
        # forces a fresh random password in the same write, matching
        # create()'s own existing behavior. Setting a REAL password for a
        # newly-human account is a deliberate follow-up write, not
        # something this call should ever silently allow to ride along.
        if "is_service_account" in vals:
            vals = dict(vals)
            vals["password"] = secrets.token_hex(32)
            res = super().write(vals)
            # Bug-hunt fix, 2026-09-13 (found during the parallel security_utils.py bug-hunt
            # dispatch, deferred here to avoid a same-day edit collision on this file, now closed
            # separately): `ir.http._is_service_account_cached` (zero_sudo/models/ir_http.py) is
            # `@distributed_cache()`-decorated -- an L1, process-lifetime cache checked BEFORE
            # Redis's own 24h TTL is ever consulted, so a worker that already cached this uid's
            # OLD `is_service_account` value would keep serving it indefinitely, not just for up
            # to 24h. This write is the ONLY place that field's real value changes, and until this
            # fix nothing here ever invalidated that cache -- promoting a compromised user to a
            # service account (the exact incident-response action this flag exists to support)
            # would NOT actually revoke their already-cached "not a service account" verdict on
            # any worker that had already resolved it, letting them keep using the interactive Web
            # UI (`ir_http._authenticate`'s own gate, which reads this exact cached value) past
            # the moment an admin believed the account was locked down. `notify_model_invalidation`
            # both clears this worker's own local+Redis entries (deferred to postcommit, so a
            # write that later rolls back never incorrectly evicts a still-valid cached value) and
            # signals every OTHER worker via the real, already-fixed pg_notify path (see
            # security_utils.py's own `_notify_cache_invalidation` claim for why that's the
            # correct, listened-to channel).
            notify_model_invalidation(self.env, "ir.http")
            return res
        elif "password" in vals:
            if self.ids:
                # Read the flag with SQL, not self.filtered("is_service_account"): that field is
                # groups="base.group_system", so filtering through the ORM raised AccessError
                # whenever a non-system caller wrote a password. That includes the narrow service
                # accounts this module exists to promote, e.g. ham_base.user_manager_service
                # handing a squatted account to its callsign's licensee (ham_onboarding's LoTW
                # takeover), found 2026-09-16. The same raw read as
                # ir.http._is_service_account_cached; flush first so an is_service_account change
                # still pending in this transaction is seen.
                # Verified by [@ANCHOR: test_write_password_as_a_non_system_service_account_splits_by_flag]
                self.flush_recordset(["is_service_account"])
                self.env.cr.execute(  # Tested by [@ANCHOR: test_write_password_as_a_non_system_service_account_splits_by_flag]
                    "SELECT id FROM res_users WHERE id = ANY(%s) AND is_service_account",
                    (list(self.ids),),
                )
                service_accounts = self.env["res.users"].browse([row[0] for row in self.env.cr.fetchall()])
                if service_accounts:
                    regular_accounts = self - service_accounts

                    res = True
                    if regular_accounts:
                        res = super(ResUsersZeroSudo, regular_accounts).write(vals)

                    vals_no_pw = vals.copy()
                    vals_no_pw.pop("password", None)
                    if vals_no_pw:
                        res2 = super(ResUsersZeroSudo, service_accounts).write(vals_no_pw)
                        res = res and res2
                    return res
        return super().write(vals)
