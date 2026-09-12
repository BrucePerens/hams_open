# -*- coding: utf-8 -*-
# Part of Odoo. See LICENSE file for full copyright and licensing details.
#
# This file is part of hams_open, an open source module.
# License: AGPL-3.0

from types import SimpleNamespace
from unittest.mock import MagicMock

from odoo.tests.common import HOST, tagged
from .common import HamsHttpCase, HamsTransactionCase, _patched_handle_request_paused


@tagged("post_install", "-at_install")
class TestZeroSudoViews(HamsHttpCase):

    def setUp(self):
        super().setUp()
        self.env.ref('base.user_admin').lang = "en_US"

    def test_01_res_users_views(self):
        # [@ANCHOR: zero_sudo:COMM_test_res_users_views]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_test_res_users_views]
        # ---
        # Tests [@ANCHOR: zero_sudo:hams_http_case_setup]
        # ---
        # Tests [@ANCHOR: zero_sudo:hams_http_case_start_hams_browser]
        # ---
        # Tests [@ANCHOR: zero_sudo:hams_http_case_teardown]
        # ---
        # Tests [@ANCHOR: zero_sudo:hams_http_case_teardown_class]
        # (all four run automatically via setUp()/tearDown()/
        # tearDownClass() for every test in this HamsHttpCase-based
        # class, this one included)
        """
        Verify that the zero_sudo res.users views compile and render correctly.
        """
        # Execute get_view to satisfy the AST linter for xpath injections
        self.env["res.users"].get_view(view_type="form")
        self.env["res.users"].get_view(view_type="list")
        self.env["res.users"].get_view(view_type="search")

    def test_02_zero_sudo_tour(self):
        # Tests [@ANCHOR: zero_sudo:hams_http_case_start_tour]

        # Tests [@ANCHOR: zero_sudo:hams_http_case_browser_js]
        # (start_tour() delegates to core Odoo's own HttpCase.start_tour,
        # which calls self.browser_js() -- this class's own override --
        # to actually run the tour JS)
        # ---
        # Tests [@ANCHOR: zero_sudo:patched_handle_request_paused]
        # ---
        # Tests [@ANCHOR: zero_sudo:patched_preexec]
        # ---
        # Tests [@ANCHOR: zero_sudo:patched_spawn_chrome]
        # ---
        # Tests [@ANCHOR: zero_sudo:patched_process_request_thread]
        # ---
        # Tests [@ANCHOR: zero_sudo:patched_save_test_file]
        # ---
        # Tests [@ANCHOR: zero_sudo:patched_opener_init]
        # ---
        # Tests [@ANCHOR: zero_sudo:patched_chrome_init]
        # ---
        # Tests [@ANCHOR: zero_sudo:patched_chrome_stop]
        # ---
        # Tests [@ANCHOR: zero_sudo:patched_wait_ready]
        # ---
        # Tests [@ANCHOR: zero_sudo:patched_chrome_start]
        # (this is a real, end-to-end headless-Chrome tour run -- every
        # one of these monkeypatches on ChromeBrowser/werkzeug internals
        # is on the real, live path a tour actually exercises: spawning
        # Chrome under its own process group, waiting for the CDP socket
        # to come up, driving navigation/requests through the DevTools
        # protocol, and (on failure) saving a screenshot)
        # [@ANCHOR: zero_sudo:COMM_test_zero_sudo_tour]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_story_login_blocking]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_journey_service_account_lifecycle]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_zero_sudo_tour]
        """Run the zero_sudo_tour to verify UI functionality."""
        # Enforcing ADR-0081 Section 8: Explicitly set ?debug=1 to prevent Owl dev mode crashes
        self.start_tour("/odoo?debug=1", "zero_sudo_tour", login="admin")

    def test_03_noisy_table_views(self):
        # [@ANCHOR: zero_sudo:COMM_test_noisy_table_views]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_test_noisy_table_views]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_UX_NOISY_TABLE_MANAGEMENT]
        """
        Verify that the noisy_table views compile and render correctly.
        """
        self.env["zero_sudo.noisy_table"].get_view(view_type="form")
        self.env["zero_sudo.noisy_table"].get_view(view_type="list")

    def test_04_security_log_views(self):
        # [@ANCHOR: zero_sudo:COMM_test_security_log_views]
        # ---
        # Tests [@ANCHOR: zero_sudo:COMM_test_security_log_views]
        """Verify that the security.log views compile and render correctly."""
        self.env["zero_sudo.security.log"].get_view(view_type="form")
        self.env["zero_sudo.security.log"].get_view(view_type="list")


@tagged("post_install", "-at_install")
class TestFetchInterceptExtraAllowedHosts(HamsTransactionCase):
    """Direct unit coverage for `_patched_handle_request_paused`'s
    `extra_allowed_fetch_hosts` opt-in (added 2026-09-12) -- no real
    ChromeBrowser/websocket needed, since the function under test is pure
    Python logic dispatching on a URL string. Real bug this opt-in fixes:
    ham_shack's test_03_shack_relay_confirm_claim_tour needs a real fetch()
    round trip against a fake relay that is deliberately NOT this test
    server's own host (per LOCAL_RELAY_ZERO_TOUCH_ONBOARDING.md's "different
    LAN host" scenario) -- before this opt-in, this safety net's hardcoded
    `HOST`-only allowlist left no way to do that without either a
    self.fetch_proxy() mock (defeating the whole point of a real HTTP round
    trip) or disabling the safety net outright.
    """

    def _fake_browser(self, extra_allowed_fetch_hosts=()):
        # A bare object, not a real ChromeBrowser: only `.test_case` (read by
        # the function under test) and `._websocket_send` (asserted against
        # below) are ever touched.
        test_case = SimpleNamespace(
            fetch_proxy=None, extra_allowed_fetch_hosts=extra_allowed_fetch_hosts
        )
        browser = SimpleNamespace(test_case=test_case, _websocket_send=MagicMock())
        return browser

    def _call(self, browser, url):
        _patched_handle_request_paused(
            browser, {"request": {"url": url}, "requestId": "req-1"}
        )

    # [@ANCHOR: zero_sudo:test_extra_allowed_fetch_hosts_default_is_unchanged]
    # Tests [@ANCHOR: zero_sudo:extra_allowed_fetch_hosts]
    # Tests [@ANCHOR: zero_sudo:patched_handle_request_paused]
    def test_01_default_behavior_unchanged_non_host_url_still_fails(self):
        browser = self._fake_browser()
        self._call(browser, "http://192.168.10.92:38913/api/auth_status")
        cmd, kwargs = browser._websocket_send.call_args[0][0], browser._websocket_send.call_args[1]
        self.assertEqual(cmd, "Fetch.failRequest")
        self.assertEqual(kwargs["params"]["errorReason"], "Failed")

    # [@ANCHOR: zero_sudo:test_extra_allowed_fetch_hosts_default_is_unchanged]
    # Tests [@ANCHOR: zero_sudo:patched_handle_request_paused]
    def test_02_the_standard_host_still_passes_through_with_no_opt_in(self):
        browser = self._fake_browser()
        self._call(browser, f"http://{HOST}:8069/web/session/get_session_info")
        cmd = browser._websocket_send.call_args[0][0]
        self.assertEqual(cmd, "Fetch.continueRequest")

    # [@ANCHOR: zero_sudo:test_extra_allowed_fetch_hosts_opt_in]
    # Tests [@ANCHOR: zero_sudo:extra_allowed_fetch_hosts]
    # Tests [@ANCHOR: zero_sudo:patched_handle_request_paused]
    def test_03_an_opted_in_extra_host_passes_through_for_real(self):
        browser = self._fake_browser(extra_allowed_fetch_hosts=("127.0.0.2",))
        self._call(browser, "http://127.0.0.2:38913/api/auth_status")
        cmd = browser._websocket_send.call_args[0][0]
        self.assertEqual(
            cmd,
            "Fetch.continueRequest",
            "[!] DIAGNOSTIC FOR AI: extra_allowed_fetch_hosts must let a real "
            "fetch to that exact host proceed, matching HOST's own treatment.",
        )

    # [@ANCHOR: zero_sudo:test_extra_allowed_fetch_hosts_opt_in]
    # Tests [@ANCHOR: zero_sudo:extra_allowed_fetch_hosts]
    # Tests [@ANCHOR: zero_sudo:patched_handle_request_paused]
    def test_04_opting_in_one_host_does_not_allow_a_different_one(self):
        browser = self._fake_browser(extra_allowed_fetch_hosts=("127.0.0.2",))
        self._call(browser, "http://192.168.10.92:38913/api/auth_status")
        cmd = browser._websocket_send.call_args[0][0]
        self.assertEqual(
            cmd,
            "Fetch.failRequest",
            "[!] DIAGNOSTIC FOR AI: the opt-in must not become a blanket "
            "allow-everything switch -- only the specific listed host(s) "
            "should ever bypass the safety net.",
        )
