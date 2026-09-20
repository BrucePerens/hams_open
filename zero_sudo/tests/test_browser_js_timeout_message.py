# -*- coding: utf-8 -*-
# This software is distributed under the terms of the Affero General Public License (AGPL-3).
# SPDX-License-Identifier: AGPL-3.0-or-later
"""HamsHttpCase.browser_js must not blame the websocket for a CDP timeout.

Live evidence (see the to-do named in the message) showed that the bare
TimeoutError core raises from _websocket_request() is produced when
_wait_ready()'s busy loop runs out of budget with a falsy ready condition,
with Chrome, the socket and the receiver thread all healthy. The old
"severed/unresponsive Chrome websocket" wording sent three investigations
into Odoo core for days.
"""
from odoo.addons.zero_sudo.tests.common import HamsHttpCase
from odoo.tests.common import HttpCase, tagged


@tagged("post_install", "-at_install")
class TestBrowserJsTimeoutMessage(HamsHttpCase):
    # Tests [@ANCHOR: zero_sudo:hams_http_case_browser_js]
    def test_cdp_timeout_message_points_at_ready_condition_not_websocket(self):
        self.safe_patch_object(
            HttpCase, "browser_js", side_effect=TimeoutError("Runtime.evaluate(x)")
        )
        with self.assertRaises(AssertionError) as caught:
            self.browser_js("/web/login", "console.log('test successful')")
        msg = str(caught.exception)
        self.assertNotIn(
            "severed/unresponsive", msg,
            "[!] DIAGNOSTIC FOR AI: the watchdog message again claims a severed "
            "websocket, which live evidence refuted.",
        )
        self.assertIn("ready condition never became true", msg)
        self.assertIn("Template not found", msg)
        self.assertIn("Runtime.evaluate(x)", msg)
