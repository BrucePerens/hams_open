# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# SPDX-License-Identifier: AGPL-3.0-or-later

from odoo.addons.base.models import res_users as core_res_users
from odoo.addons.zero_sudo.models import res_users as zero_sudo_res_users
from odoo.addons.zero_sudo.models import security_utils
from odoo.exceptions import AccessDenied
from odoo.tests.common import tagged

from . import common


class _FakeHttpRequest:
    """Only what core's login cooldown and `_get_trusted_client_ip` read: a WSGI environ and the headers."""

    def __init__(self, remote_addr, headers):
        self.environ = {"REMOTE_ADDR": remote_addr}
        self.headers = headers

    @property
    def remote_addr(self):
        return self.environ["REMOTE_ADDR"]


class _FakeRequest:
    def __init__(self, remote_addr, headers):
        self.httprequest = _FakeHttpRequest(remote_addr, headers)

    def _get_current_object(self):
        return self

    def __bool__(self):
        return True


@tagged("post_install", "-at_install")
class TestLoginCooldownClientIp(common.HamsTransactionCase):
    def _fail_once(self, request_obj):
        users = self.env["res.users"]
        self.safe_patch_object(core_res_users, "request", request_obj)
        self.safe_patch_object(zero_sudo_res_users, "request", request_obj)
        self.safe_patch_object(security_utils, "request", request_obj)
        with self.assertRaises(AccessDenied):
            with users._assert_can_auth(user="nobody@example.invalid"):
                raise AccessDenied()
        return request_obj.httprequest.remote_addr

    # [@ANCHOR: test_cooldown_is_counted_per_visitor_not_per_tunnel]
    def test_cooldown_is_counted_per_visitor_not_per_tunnel(self):
        # Tests [@ANCHOR: zero_sudo:login_cooldown_per_client_ip]
        """Behind the Tunnel every request arrives from loopback, so core's per-source-address login cooldown
        counted every visitor in one bucket: five bad passwords from one address locked everybody out. The
        failure must be recorded against the visitor's own address (CF-Connecting-IP) and REMOTE_ADDR restored."""
        self.env["ir.config_parameter"].set_param("base.login_cooldown_after", "1")
        registry = self.env.registry
        registry._login_failures = None
        request_obj = _FakeRequest("::1", {"CF-Connecting-IP": "203.0.113.7"})
        after = self._fail_once(request_obj)
        self.assertEqual(after, "::1", "REMOTE_ADDR must be restored after the login attempt")
        self.assertIn("203.0.113.7", registry._login_failures)
        self.assertNotIn("::1", registry._login_failures)

    def test_garbage_forwarded_address_falls_back_to_the_peer(self):
        # Tests [@ANCHOR: zero_sudo:login_cooldown_per_client_ip]
        self.env["ir.config_parameter"].set_param("base.login_cooldown_after", "1")
        registry = self.env.registry
        registry._login_failures = None
        request_obj = _FakeRequest("::1", {"CF-Connecting-IP": "not-an-ip"})
        self._fail_once(request_obj)
        self.assertIn("::1", registry._login_failures)
