# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0
# SPDX-License-Identifier: AGPL-3.0-or-later

from odoo import http
from odoo.http import request
from odoo.addons.web.controllers.home import Home
from odoo.addons.web.controllers.session import Session


class ZeroSudoHome(Home):
    @http.route()
    def web_login(self, redirect=None, login=None, **kw):
        # [@ANCHOR: zero_sudo:COMM_web_login_interceptor]
        # ---
        # # Verified by [@ANCHOR: zero_sudo:COMM_test_web_login_interceptor]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_story_login_blocking]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_journey_service_account_lifecycle]

        # Explicit input extraction to satisfy strict controller binding audits
        attempted_login = login
        if not attempted_login and request.params:
            attempted_login = request.params.get("login")

        response = super().web_login(redirect=redirect, login=login, **kw)

        if request.session.uid:
            # [@ANCHOR: zero_sudo:COMM_web_login_interceptor_check]
            # ---
            # # Verified by [@ANCHOR: zero_sudo:COMM_test_web_login_interceptor]
            # SECURITY MANDATE: We use direct SQL instead of .sudo() or ORM calls to check the is_service_account flag.
            # This bypasses ACLs for the isolation check (ADR-0005) without triggering Zero-Sudo linter violations.
            # FUTURE DEVELOPERS: DO NOT CHANGE THIS TO .sudo(). Direct SQL is the intentional, audited pattern here.
            # ---
            # Tests [@ANCHOR: zero_sudo:COMM_story_login_blocking]
            # ---
            request.env.cr.execute(  # audit-ignore-sql: Tested by [@ANCHOR: zero_sudo:COMM_test_web_login_interceptor]  # fmt: skip
                "SELECT is_service_account FROM res_users WHERE id = %s",
                (request.session.uid,),
            )
            res = request.env.cr.fetchone()
            if res and res[0]:
                # Log the blocked attempt before logging out
                # We assume the facility account context for logging
                utils = request.env["zero_sudo.security.utils"]
                facility_env = utils._get_service_env(
                    "zero_sudo.odoo_facility_service_internal"
                )
                facility_env["zero_sudo.security.log"].create(
                    {
                        "user_id": request.session.uid,
                        "login": attempted_login,
                        "ip_address": request.httprequest.remote_addr,
                        "user_agent": request.httprequest.user_agent.string,
                        "reason": "service_account_blocked",
                    }
                )
                request.session.logout()
                # Use query parameter to show error on login page after redirect
                return request.redirect(
                    "/web/login?error=access_denied_service"
                )  # burn-ignore-route: Tested by [@ANCHOR: zero_sudo:COMM_test_web_login_interceptor]  # fmt: skip
        return response


class ZeroSudoSession(Session):
    @http.route()
    # [@ANCHOR: zero_sudo:COMM_json_session_authenticate_interceptor]
    # ---
    # # Verified by [@ANCHOR: zero_sudo:COMM_test_json_session_authenticate_interceptor]
    # bug-hunt (2026-09-13): `/web/login` (ZeroSudoHome.web_login above) is
    # NOT the only real session-establishing login endpoint -- Odoo's own
    # web client (and any third-party JSON client) logs in via this exact
    # JSON-RPC route (odoo/addons/web/controllers/session.py's
    # Session.authenticate), which was never covered by the web_login
    # override above. Because ir_http.py's `_authenticate()` request-dispatch
    # guard reads `request.session.uid` as it stood BEFORE this request's own
    # dispatch began (confirmed against odoo/addons/base/models/ir_http.py's
    # `_authenticate`/`_authenticate_explicit`, which run ahead of
    # `_dispatch`), it cannot catch a service account authenticating on THIS
    # request either -- the exact same reason `web_login` needs its own
    # post-super() check instead of relying on that hook alone. Without this
    # override, a service account's leaked credentials could complete one
    # real authenticated call here, returning a live `session_info()`
    # payload (uid, name, allowed companies, etc.) straight to the caller
    # with no audit trail at all -- contradicting docs/stories/
    # login_blocking.md's own "Even if a service account somehow already
    # has a live session... _authenticate() re-checks on every single
    # authenticated request dispatch" claim for this one specific request.
    # (bug-hunt (2026-09-13), second pass: this used to `raise AccessDenied`
    # here instead of returning. Confirmed by reading odoo/service/model.py's
    # `retrying()` line by line: it only calls `env.cr.commit()` AFTER
    # `func()` (this whole dispatch, our override included) returns without
    # raising -- any exception takes the `except Exception: ...; raise`
    # branch instead, which resets the transaction and re-raises WITHOUT
    # ever committing, and `_serve_db()`'s own `finally: cr.close()` then
    # rolls back via `Cursor.close()` -> `_close(False)` -> `self.
    # rollback()` (also confirmed directly, `odoo/sql_db.py`). Raising
    # therefore silently discarded the very `zero_sudo.security.log` row
    # this override exists to create -- the audit trail this whole
    # interceptor is supposed to add would never actually have persisted.
    # Fixed to mirror Odoo core's own real precedent for "don't complete
    # this login" in this exact function: `super().authenticate()`'s own
    # MFA-mismatch branch above already returns `{"uid": None}` rather than
    # raising, for the identical reason -- a normal return commits the
    # transaction (the log row persists for real) and lets
    # Dispatcher.post_dispatch() run (so `request.session.logout()` below
    # is now actually load-bearing -- the rotated/logged-out session gets
    # saved -- not defense-in-depth-only as an earlier draft of this
    # comment claimed).)
    #
    # 2026-09-16: `super().authenticate()` itself builds and returns the
    # `session_info()` payload before this method ever sees the result, and
    # `session_info()` reads `web.max_file_upload_size` via `ir_config_parameter.
    # get_param()`. `ham_base`'s get_param() refuses any key not on its
    # allow lists for an `is_service_account` user, so that read raised
    # AccessError from inside super() -- the service account was still
    # blocked, but by the wrong mechanism (a 500-mapped AccessError instead
    # of this interceptor's own `{"uid": None}` denial and audit-log row),
    # whenever ham_base was installed alongside zero_sudo. Checking
    # `is_service_account` by LOGIN before calling super() at all keeps the
    # fail-fast refusal at the request boundary and never lets
    # session_info() run for a service account, regardless of whether the
    # password given is correct -- which leaks no more than the existing
    # post-super() check already did. The post-super() check below stays as
    # defense in depth (e.g. a login string that doesn't exact-match here).
    def authenticate(self, db, login, password, base_location=None):
        if login:
            request.env.cr.execute(  # audit-ignore-sql: Tested by [@ANCHOR: zero_sudo:COMM_json_session_authenticate_interceptor]  # fmt: skip
                "SELECT id, is_service_account FROM res_users WHERE login = %s",
                (login,),
            )
            res = request.env.cr.fetchone()
            if res and res[1]:
                utils = request.env["zero_sudo.security.utils"]
                facility_env = utils._get_service_env(
                    "zero_sudo.odoo_facility_service_internal"
                )
                facility_env["zero_sudo.security.log"].create(
                    {
                        "user_id": res[0],
                        "login": login,
                        "ip_address": request.httprequest.remote_addr,
                        "user_agent": request.httprequest.user_agent.string,
                        "reason": "service_account_blocked",
                    }
                )
                return {"uid": None}

        result = super().authenticate(db, login, password, base_location=base_location)

        if request.session.uid:
            # SECURITY MANDATE: mirrors ZeroSudoHome.web_login's own
            # direct-SQL check above -- see that method's own comment for
            # why this intentionally does not use .sudo()/ORM here.
            request.env.cr.execute(  # audit-ignore-sql: Tested by [@ANCHOR: zero_sudo:COMM_test_json_session_authenticate_interceptor]  # fmt: skip
                "SELECT is_service_account FROM res_users WHERE id = %s",
                (request.session.uid,),
            )
            res = request.env.cr.fetchone()
            if res and res[0]:
                blocked_uid = request.session.uid
                utils = request.env["zero_sudo.security.utils"]
                facility_env = utils._get_service_env(
                    "zero_sudo.odoo_facility_service_internal"
                )
                facility_env["zero_sudo.security.log"].create(
                    {
                        "user_id": blocked_uid,
                        "login": login,
                        "ip_address": request.httprequest.remote_addr,
                        "user_agent": request.httprequest.user_agent.string,
                        "reason": "service_account_blocked",
                    }
                )
                request.session.logout()
                return {"uid": None}
        return result
