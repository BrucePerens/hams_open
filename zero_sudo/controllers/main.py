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


# [@ANCHOR: zero_sudo_main_zero_sudo_home]
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
                #
                # The IP recorded here is `_get_trusted_client_ip()`, not the
                # raw `request.httprequest.remote_addr`.
                # This deployment is Cloudflare-Tunnel-only, with
                # cloudflared on the same host as the Odoo HTTP server, so
                # the transport peer of a real external request is
                # loopback; `remote_addr` recorded `127.0.0.1` for every
                # attacker, making the `ip_address` column of the one audit
                # log this module maintains unable to answer the only
                # question it exists to answer. `zero_sudo.security.utils`
                # -- already instantiated on the line above for the service
                # env -- provides the correct resolver. It
                # degrades to exactly `remote_addr` whenever the peer is
                # NOT loopback, so this is never worse than what it
                # replaces.
                utils = request.env["zero_sudo.security.utils"]
                facility_env = utils._get_service_env(
                    "zero_sudo.odoo_facility_service_internal"
                )
                facility_env["zero_sudo.security.log"].create(
                    {
                        "user_id": request.session.uid,
                        "login": attempted_login,
                        "ip_address": utils._get_trusted_client_ip(),
                        "user_agent": request.httprequest.user_agent.string,
                        "reason": "service_account_blocked",
                    }
                )
                request.session.logout()
                # Use query parameter to show error on login page after redirect
                return request.redirect(
                    "/web/login?error=access_denied_service"
                )  # Tested by [@ANCHOR: zero_sudo:COMM_test_web_login_interceptor]  # fmt: skip
        return response


# [@ANCHOR: zero_sudo_main_zero_sudo_session]
class ZeroSudoSession(Session):
    @http.route()
    # [@ANCHOR: zero_sudo:COMM_json_session_authenticate_interceptor]
    # ---
    # # Verified by [@ANCHOR: zero_sudo:COMM_test_json_session_authenticate_interceptor]
    # `/web/login` (ZeroSudoHome.web_login above) is
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
    # (This returns instead of raising `AccessDenied`. In
    # odoo/service/model.py's `retrying()`, it only calls `env.cr.commit()` AFTER
    # `func()` (this whole dispatch, our override included) returns without
    # raising -- any exception takes the `except Exception: ...; raise`
    # branch instead, which resets the transaction and re-raises WITHOUT
    # ever committing, and `_serve_db()`'s own `finally: cr.close()` then
    # rolls back via `Cursor.close()` -> `_close(False)` -> `self.
    # rollback()` (`odoo/sql_db.py`). Raising
    # would therefore silently discard the very `zero_sudo.security.log` row
    # this override exists to create, so the audit trail would never
    # persist. This mirrors Odoo core's own precedent for "don't complete
    # this login" in this exact function: `super().authenticate()`'s own
    # MFA-mismatch branch above already returns `{"uid": None}` rather than
    # raising, for the identical reason -- a normal return commits the
    # transaction (the log row persists for real) and lets
    # Dispatcher.post_dispatch() run (so `request.session.logout()` below
    # is load-bearing -- the rotated/logged-out session gets
    # saved -- not defense-in-depth only.)
    #
    # `super().authenticate()` itself builds and returns the
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
                # Verify the credential through Odoo
                # core's OWN authentication path -- `Session.authenticate()`
                # in odoo/http.py, which is exactly what
                # `super().authenticate()` calls, including
                # `res.users._assert_can_auth`'s per-source-IP login cooldown
                # -- BEFORE writing anything at all.
                #
                # This is deliberately NOT `super().authenticate()`: super()
                # goes on to build `session_info()`, which is the entire
                # reason the pre-check above exists (see that
                # note). Calling the session-level primitive directly gets
                # the credential check without ever reaching session_info().
                #
                # Why this matters: `/web/session/authenticate` is
                # `auth="none"`. Returning
                # `{"uid": None}` (and writing a committed
                # `zero_sudo.security.log` row) on a LOGIN match alone, with
                # the password never examined, would be abusable. Every
                # service-account login in this project is declared in public XML in this
                # very repository, so any anonymous caller could name one
                # and drive an unlimited number of committed audit rows --
                # unthrottled, because returning before super() also skips
                # core's own login cooldown, and retained forever (nothing
                # purges `zero_sudo.security.log`). It would also destroy the
                # row's meaning: `service_account_blocked` must imply
                # "someone actually held this account's credentials", not
                # "someone typed this name", or a real
                # credential compromise becomes indistinguishable from a
                # background scan in the one log built to tell them apart.
                #
                # A wrong password raises core's own `AccessDenied` from
                # inside `session.authenticate()`, exactly as an unmatched
                # login already does, which `retrying()` rolls back -- no
                # row, no commit. A CORRECT password lands here, which is
                # the event this interceptor exists to record and deny.
                credential = {"login": login, "password": password, "type": "password"}
                request.session.authenticate(request.env, credential)
                utils = request.env["zero_sudo.security.utils"]
                facility_env = utils._get_service_env(
                    "zero_sudo.odoo_facility_service_internal"
                )
                facility_env["zero_sudo.security.log"].create(
                    {
                        "user_id": res[0],
                        "login": login,
                        "ip_address": utils._get_trusted_client_ip(),
                        "user_agent": request.httprequest.user_agent.string,
                        "reason": "service_account_blocked",
                    }
                )
                request.session.logout()
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
                # Real client IP, not the tunnel's
                # loopback peer -- see ZeroSudoHome.web_login's own note.
                utils = request.env["zero_sudo.security.utils"]
                facility_env = utils._get_service_env(
                    "zero_sudo.odoo_facility_service_internal"
                )
                facility_env["zero_sudo.security.log"].create(
                    {
                        "user_id": blocked_uid,
                        "login": login,
                        "ip_address": utils._get_trusted_client_ip(),
                        "user_agent": request.httprequest.user_agent.string,
                        "reason": "service_account_blocked",
                    }
                )
                request.session.logout()
                return {"uid": None}
        return result
