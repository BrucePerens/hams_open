# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

from odoo import models, fields, api

from odoo.addons.distributed_redis_cache.redis_cache import distributed_cache
import logging

_logger = logging.getLogger(__name__)
class ResUsersEdgeRouting(models.Model):
    name = fields.Char(string="Name", required=True)
    """
    Extends res.users with edge.routing.mixin to provide high-performance
    vanity URL routing and slug caching.
    """

    _name = "res.users"
    _inherit = ["res.users", "edge.routing.mixin"]

    # [@ANCHOR: edge_routing:COMM_res_users_get_record_by_slug]
    @api.model
    @distributed_cache()
    def get_record_by_slug(self, slug, override_svc_uid=None):
        res = super().get_record_by_slug(slug, override_svc_uid=override_svc_uid)
        if not res and slug:
            # Virtual Slug Fallback: Check if the URL matches their unique login (e.g. Callsign)
            if override_svc_uid:
                target_env = self.with_user(override_svc_uid).env
            else:
                if self.env.registry.loaded:
                    self.env.cr.execute("SELECT 1 FROM ir_model_data WHERE module=%s AND name=%s", ('edge_routing', 'edge_routing_service_account'))  # Tested by [@ANCHOR: test_edge_routing_service_account_sql_check]
                    if self.env.cr.fetchone():
                        try:
                            # bug-hunt (2026-09-09): savepoint is load-
                            # bearing -- _get_service_uid()'s SQL-backed uid
                            # lookup does a real Postgres `RAISE EXCEPTION`
                            # (zero_sudo_get_service_uid() in
                            # zero_sudo/data/postgres_procedures.xml) on a
                            # missing/disabled/non-service account, which
                            # aborts the current transaction. Without a
                            # savepoint to roll back to, the `target_env =
                            # self.env` fallback below is caught here fine,
                            # but the `target_env["res.users"].search(...)`
                            # call a few lines down would then raise
                            # `InFailedSqlTransaction` uncaught, since every
                            # statement on a poisoned transaction fails
                            # until rolled back. Class 20, one level deeper.
                            with self.env.cr.savepoint():
                                target_env = self.env["zero_sudo.security.utils"]._get_service_env(
                                    "edge_routing.edge_routing_service_account"
                                )
                        except Exception as e:  # audit-ignore-catch-all
                            # bug-hunt (2026-09-09): _get_service_env() raises
                            # AccessError (not KeyError/ValueError) on a bad
                            # xml_id, plus a possible psycopg2 error from the
                            # SQL-backed uid lookup -- narrowed to the wrong
                            # types, this fallback never actually caught a
                            # real resolution failure. Class 20.
                            _logger.warning("Failed to access website settings: %s", e)
                            target_env = self.env
                    else:
                        target_env = self.env
                else:
                    target_env = self.env

            # bug-hunt (2026-09-09): was `("login", "=ilike",
            # str(slug).lower())`. Odoo's `=ilike` (unlike bare `ilike`)
            # passes its operand to SQL `ILIKE` verbatim -- no wildcard-
            # wrapping AND no escaping of `%`/`_` (confirmed against
            # odoo/orm/fields.py's `condition_to_sql`: `need_wildcard = '='
            # not in operator`). `slug` here is attacker/visitor-controlled
            # (a raw URL path segment) -- a request for `/a%/blog` sets
            # `slug = "a%"`, matching ANY user whose `login` starts with
            # "a" instead of failing to find an exact match. Unlike
            # `website_slug` (DB-constrained to `^[a-z0-9\-]+$`, fixed
            # above by switching to plain `=`), `res.users.login` is NOT
            # charset/case-constrained -- a real login can be a mixed-case
            # email or callsign, so case-insensitivity must stay. Escaping
            # `%`/`_` (and a literal backslash, PostgreSQL's own default
            # LIKE/ILIKE escape character) in the value neutralizes the
            # wildcard injection while preserving exact, case-insensitive
            # matching for every real login.
            escaped_slug = (
                str(slug).lower()
                .replace("\\", "\\\\")
                .replace("%", "\\%")
                .replace("_", "\\_")
            )
            user = (
                target_env["res.users"]
                .with_context(active_test=False)
                .search([("login", "=ilike", escaped_slug)], limit=1)
            )
            return user.id if user else False
        return res
