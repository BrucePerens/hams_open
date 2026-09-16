# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import collections
import logging
import http.server
import threading
import requests
from unittest.mock import MagicMock
from odoo.exceptions import AccessError
from odoo.tools import mute_logger
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.cloudflare.utils.cloudflare_api import (
    purge_urls,
    purge_tags,
    retry_strategy,
    session,
)

_logger = logging.getLogger(__name__)


@tagged("post_install", "-at_install")
class TestCloudflareAPIs(HamsTransactionCase):

    def test_01_waf_ban_ip(self):
        # [@ANCHOR: test_cf_ban_ip_api]

        # Tests [@ANCHOR: COMM_cf_ban_ip_api]

        mock_ban_ip = self.safe_patch("odoo.addons.cloudflare.models.ip_ban.ban_ip")
        mock_ban_ip.return_value = (True, "fake_rule_123")

        website = self.env["website"].get_current_website()
        website.write(
            {"cloudflare_api_token": "fake_token", "cloudflare_zone_id": "fake_zone"}
        )
        
        mock_creds = self.safe_patch("odoo.addons.cloudflare.models.website.WebsiteCloudflare._get_cloudflare_credentials")
        mock_creds.return_value = ("fake_token", "fake_zone")

        res = self.env["cloudflare.waf"].ban_ip("192.168.1.100", website_id=website.id)
        self.assertTrue(res)

    def test_02_turnstile_secret_fetch(self):
        # Tests [@ANCHOR: cloudflare:COMM_compute_cf_turnstile_secret]

        # Tests [@ANCHOR: cloudflare:COMM_inverse_cf_turnstile_secret]

        # [@ANCHOR: COMM_test_cf_turnstile_verify]

        # Tests [@ANCHOR: COMM_cf_turnstile_verify]

        website = self.env["website"].get_current_website()
        website.write({"cloudflare_turnstile_secret": "my_super_secret_key"})

        mock_post = self.safe_patch(
            "odoo.addons.cloudflare.utils.cloudflare_api.session.post"
        )
        mock_response = MagicMock()
        mock_response.json.return_value = {"success": True}
        mock_post.return_value = mock_response

        res = self.env["cloudflare.turnstile"].verify_token(
            "fake_token_123", "odoo", website_id=website.id
        )
        self.assertTrue(res)

        called_data = mock_post.call_args[1]["data"]
        self.assertEqual(called_data["secret"], "my_super_secret_key")
        self.assertEqual(called_data["response"], "fake_token_123")

    def test_02b_verify_token_works_for_an_unprivileged_caller(self):
        # Tests [@ANCHOR: COMM_cf_turnstile_verify]
        # Bug-hunt, review_tier 1, 2026-09-09: verify_token exists
        # specifically to be called by an anonymous/ordinary site visitor
        # submitting a Turnstile CAPTCHA response -- `cloudflare_turnstile_
        # secret` carries a `groups=` restriction, and reading it via the
        # caller's own unelevated env used to raise AccessError for
        # exactly that population. test_02_turnstile_secret_fetch above
        # only proves this works for the admin-level default test user,
        # which already has base.group_system -- it can't catch this bug,
        # since the fix (elevating via with_user on the WAF service
        # account) and the bug it fixes both look identical to an
        # already-privileged caller. This test uses a bare portal user
        # instead, the same population the real feature exists to serve.
        website = self.env["website"].get_current_website()
        website.write({"cloudflare_turnstile_secret": "my_super_secret_key"})

        mock_post = self.safe_patch(
            "odoo.addons.cloudflare.utils.cloudflare_api.session.post"
        )
        mock_response = MagicMock()
        mock_response.json.return_value = {"success": True}
        mock_post.return_value = mock_response

        unprivileged = self.env["res.users"].create(
            {
                "name": "Unprivileged Turnstile Caller",
                "login": "turnstile_unprivileged",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )
        res = (
            self.env["cloudflare.turnstile"]
            .with_user(unprivileged)
            .verify_token("fake_token_123", "odoo", website_id=website.id)
        )
        self.assertTrue(
            res,
            "[!] DIAGNOSTIC FOR AI: an unprivileged/anonymous caller must "
            "still get a real verification verdict, not an AccessError, "
            "from verify_token -- that's the entire point of CAPTCHA "
            "verification.",
        )

    def test_03_tunnel_setup(self):
        # [@ANCHOR: COMM_test_cf_tunnel_setup]

        # Tests [@ANCHOR: COMM_cf_tunnel_setup]
        mock_create = self.safe_patch(
            "odoo.addons.cloudflare.models.res_config_settings.create_cfd_tunnel"
        )
        mock_get_token = self.safe_patch(
            "odoo.addons.cloudflare.models.res_config_settings.get_cfd_tunnel_token"
        )

        mock_create.return_value = (True, "tunnel_id_123")
        mock_get_token.return_value = (True, "mock_token_xyz")

        website = self.env["website"].get_current_website()
        website.write(
            {"cloudflare_account_id": "acc123", "cloudflare_api_token": "tok123"}
        )
        settings = self.env["res.config.settings"].create({"website_id": website.id})

        action = settings.action_generate_tunnel_command()
        self.assertEqual(action["res_model"], "cloudflare.tunnel.wizard")

        wizard = self.env["cloudflare.tunnel.wizard"].browse(action["res_id"])
        self.assertIn("mock_token_xyz", wizard.command)

    def test_04_sync_tunnels(self):
        # Tests [@ANCHOR: cloudflare:COMM_sync_tunnels_for_website]

        # [@ANCHOR: COMM_test_cf_sync_tunnels]

        # Tests [@ANCHOR: COMM_cf_sync_tunnels]
        mock_list = self.safe_patch(
            "odoo.addons.cloudflare.models.tunnel.list_cfd_tunnels"
        )
        mock_list.return_value = [
            {
                "id": "t1",
                "name": "Tunnel 1",
                "status": "healthy",
                "created_at": "2021-01-01T00:00:00Z",
            }
        ]
        website = self.env["website"].get_current_website()
        website.write(
            {"cloudflare_account_id": "acc123", "cloudflare_api_token": "tok123"}
        )

        self.env["cloudflare.tunnel"].action_sync_tunnels()
        tunnel = self.env["cloudflare.tunnel"].search(
            [("cf_tunnel_id", "=", "t1")], limit=1
        )
        self.assertTrue(tunnel)
        self.assertEqual(tunnel.name, "Tunnel 1")

    def test_04a_sync_tunnels_isolates_one_websites_failure_from_the_rest(self):
        # Tests [@ANCHOR: COMM_cf_sync_tunnels]
        # Bug-hunt fix, 2026-09-11: action_sync_tunnels looped over every website calling
        # _sync_tunnels_for_website with no isolation at all -- an uncaught exception from
        # ANY one website's sync (a Cloudflare API failure, a malformed response, a DB
        # constraint violation) aborted the whole loop, silently starving every OTHER
        # website's sync for the rest of that cron/manual run. A savepoint around each
        # website's own sync (not just a bare try/except) matters specifically because a
        # DB-level failure marks the whole Postgres transaction aborted, refusing every
        # further query on that cursor until rolled back -- exercising a real ORM write
        # failure here (not just a network-layer exception) so a fix that only isolated
        # Python-level exceptions, without a savepoint, would still fail this test.
        website_broken = self.env["website"].create({"name": "Broken Cloudflare Tenant"})
        website_ok = self.env["website"].create({"name": "Healthy Cloudflare Tenant"})

        # Patching _sync_tunnels_for_website itself (rather than list_cfd_tunnels/
        # credentials several layers down) isolates this test to exactly the loop logic
        # action_sync_tunnels owns -- unittest.mock.patch on a class method doesn't bind
        # `self`, so the side_effect below receives only the plain `website_id` int
        # actually passed at each call site, with no dependency on real credentials, the
        # encrypted cloudflare_api_token field, or its @distributed_cache().
        calls = []

        def _sync_side_effect(website_id):
            calls.append(website_id)
            if website_id == website_broken.id:
                # Simulate a real ORM-level failure (not just a Python-level exception) to
                # prove the fix uses a savepoint, not merely a try/except: a bare
                # try/except would silence this too, but a DB-level failure marks the
                # whole Postgres transaction aborted -- the FOLLOWING website's own ORM
                # calls would then also fail with "current transaction is aborted" unless
                # isolated by a real savepoint, which is exactly what this proves.
                self.env.cr.execute("SELECT 1/0")

        mock_sync = self.safe_patch(
            "odoo.addons.cloudflare.models.tunnel.CloudflareTunnel._sync_tunnels_for_website"
        )
        mock_sync.side_effect = _sync_side_effect

        with mute_logger("odoo.addons.cloudflare.models.tunnel"):
            self.env["cloudflare.tunnel"].action_sync_tunnels()

        self.assertIn(
            website_ok.id,
            calls,
            "A failure syncing one website's tunnels must not prevent a later website in "
            "the same run from being synced too.",
        )

    def test_04b_sync_tunnels_rejects_an_unprivileged_caller(self):
        # Tests [@ANCHOR: cloudflare:COMM_check_tunnel_caller_authorized]
        # Bug-hunt, review_tier 1, 2026-09-09: action_sync_tunnels is a
        # public @api.model method on cloudflare.tunnel whose own
        # ir.model.access.csv grants access only to
        # cloudflare.group_cloudflare_tunnel/base.group_system -- but its
        # own first ORM touch is `self.env["website"].search(...)`, and
        # core Odoo's website module grants base.group_public read on
        # `website`. So a caller with no real cloudflare.tunnel access at
        # all (here, a plain portal user) could previously still trigger a
        # real Cloudflare API call and local cloudflare.tunnel writes for
        # every website, because _sync_tunnels_for_website elevates to the
        # tunnel service account before ever touching cloudflare.tunnel's
        # own ACL. Must now be refused before any of that runs.
        unprivileged = self.env["res.users"].create(
            {
                "name": "Unprivileged Tunnel Caller",
                "login": "tunnel_unprivileged",
                "group_ids": [(6, 0, [self.env.ref("base.group_portal").id])],
            }
        )
        with self.assertRaises(
            AccessError,
            msg="[!] DIAGNOSTIC FOR AI: an unprivileged caller must not be "
            "able to trigger a real Cloudflare tunnel sync.",
        ):
            self.env["cloudflare.tunnel"].with_user(unprivileged).action_sync_tunnels()

    def test_05_delete_tunnel(self):
        # [@ANCHOR: COMM_test_cf_delete_tunnel]

        # Tests [@ANCHOR: COMM_cf_delete_tunnel]
        mock_delete = self.safe_patch(
            "odoo.addons.cloudflare.models.tunnel.delete_cfd_tunnel"
        )
        mock_delete.return_value = (True, "Success")
        website = self.env["website"].get_current_website()
        website.write(
            {"cloudflare_account_id": "acc123", "cloudflare_api_token": "tok123"}
        )
        tunnel = self.env["cloudflare.tunnel"].create(
            {"cf_tunnel_id": "t1", "name": "Tunnel 1", "website_id": website.id}
        )
        tunnel.action_delete_tunnel()
        self.assertFalse(tunnel.exists())

    def test_06_purge_urls(self):
        # [@ANCHOR: COMM_test_purge_urls_api]

        # Tests [@ANCHOR: COMM_cf_purge_urls_api]

        mock_post = self.safe_patch(
            "odoo.addons.cloudflare.utils.cloudflare_api.session.post"
        )

        # Case 1: Missing credentials
        self.assertFalse(purge_urls(["https://a.com"], None, "zone1"))
        self.assertFalse(purge_urls(["https://a.com"], "tok1", None))

        # Case 2: Empty URLs
        self.assertTrue(purge_urls([], "tok1", "zone1"))
        mock_post.assert_not_called()

        # Case 3: Success path
        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_response.raise_for_status = MagicMock()
        mock_post.return_value = mock_response

        urls = ["https://a.com/1", "https://a.com/2"]
        res = purge_urls(urls, "fake_token", "fake_zone")
        self.assertTrue(res)

        mock_post.assert_called_once()
        _, kwargs = mock_post.call_args
        self.assertEqual(kwargs["json"]["files"], urls)
        self.assertEqual(kwargs["headers"]["Authorization"], "Bearer fake_token")

        # Case 4: Batching (Chunking to max 30)
        mock_post.reset_mock()
        many_urls = [f"https://a.com/{i}" for i in range(40)]
        purge_urls(many_urls, "fake_token", "fake_zone")
        self.assertEqual(mock_post.call_count, 2)
        self.assertEqual(len(mock_post.call_args_list[0][1]["json"]["files"]), 30)
        self.assertEqual(len(mock_post.call_args_list[1][1]["json"]["files"]), 10)

        # Case 5: API failure
        mock_post.reset_mock()
        mock_post.side_effect = requests.exceptions.RequestException("API fail")

        with mute_logger("odoo.addons.cloudflare.utils.cloudflare_api"):
            self.assertFalse(purge_urls(["https://a.com"], "tok1", "zone1"))

    def test_07_purge_tags(self):
        # Tests [@ANCHOR: cloudflare:COMM_purge_tags]

        # Tests [@ANCHOR: cloudflare:COMM_make_request]

        # Tests [@ANCHOR: cloudflare:COMM_handle_api_error]

        # [@ANCHOR: COMM_test_purge_tags_api]

        # Tests [@ANCHOR: COMM_cf_purge_tags_api]

        mock_post = self.safe_patch(
            "odoo.addons.cloudflare.utils.cloudflare_api.session.post"
        )

        # Case 1: Missing credentials
        self.assertFalse(purge_tags(["tag1"], None, "zone1"))
        self.assertFalse(purge_tags(["tag1"], "tok1", None))

        # Case 2: Empty tags
        self.assertTrue(purge_tags([], "tok1", "zone1"))
        mock_post.assert_not_called()

        # Case 3: Success path
        mock_response = MagicMock()
        mock_response.status_code = 200
        mock_response.raise_for_status = MagicMock()
        mock_post.return_value = mock_response

        tags = ["tag-a", "tag-b"]
        res = purge_tags(tags, "fake_token", "fake_zone")
        self.assertTrue(res)

        mock_post.assert_called_once()
        _, kwargs = mock_post.call_args
        self.assertEqual(kwargs["json"]["tags"], tags)
        self.assertEqual(kwargs["headers"]["Authorization"], "Bearer fake_token")

        # Case 4: Batching (Chunking to max 30)
        mock_post.reset_mock()
        many_tags = [f"tag-{i}" for i in range(40)]
        purge_tags(many_tags, "fake_token", "fake_zone")
        self.assertEqual(mock_post.call_count, 2)
        self.assertEqual(len(mock_post.call_args_list[0][1]["json"]["tags"]), 30)
        self.assertEqual(len(mock_post.call_args_list[1][1]["json"]["tags"]), 10)

        # Case 5: API failure
        mock_post.reset_mock()
        mock_post.side_effect = requests.exceptions.RequestException("API fail")

        with mute_logger("odoo.addons.cloudflare.utils.cloudflare_api"):
            self.assertFalse(purge_tags(["tag1"], "tok1", "zone1"))


@tagged("post_install", "-at_install")
class TestCloudflareRetryIdempotency(HamsTransactionCase):
    """The shared `requests` session must not replay a request Cloudflare may
    already have applied.

    The session used to retry every method on 429 and on 500/502/503/504 alike,
    so a 502 arriving after Cloudflare had already created a DNS record, a
    firewall access rule or a tunnel route sent the same create again -- up to
    three more times. These tests pin the split that fixes it.

    They drive a real HTTP server on loopback rather than a hand-built fixture,
    because the property under test lives in urllib3's retry machinery and not
    in our own code: a fixture written from how that machinery is documented to
    behave could only ever confirm the mental model the fix was written from.
    Odoo's test-mode request handler permits loopback requests, which is what
    makes this possible inside the suite.
    """

    def _serve(self, status, retry_after=None):
        """Run a loopback server answering `status`, and count requests by method.

        Follows the pattern `zero_sudo/tests/test_ssrf_safe_fetch.py` already
        established for an in-suite local server: bind port 0, serve on a thread
        that is explicitly shut down and joined rather than merely left daemonic,
        and clean up through `addCleanup` so a failing assertion still tears the
        server down.
        """
        counts = collections.Counter()

        class Handler(http.server.BaseHTTPRequestHandler):
            def _respond(self):
                counts[self.command] += 1
                self.send_response(status)
                if retry_after is not None:
                    self.send_header("Retry-After", str(retry_after))
                self.send_header("Content-Length", "0")
                self.end_headers()

            do_GET = do_PUT = do_DELETE = do_POST = do_PATCH = _respond

            def log_message(self, *args):
                """Keep the suite's output clean; the counter is the record we read."""

        server = http.server.HTTPServer(("127.0.0.1", 0), Handler)  # burn-ignore-self-hosted-server: this test spawns its own local server and connects to it in the same process
        thread = threading.Thread(target=server.serve_forever, daemon=True)  # burn-ignore-test-daemon-thread: explicitly shut down and joined by the cleanups registered below
        thread.start()
        self.addCleanup(thread.join, 5.0)
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        port = server.server_address[1]
        return f"http://127.0.0.1:{port}/probe", counts  # burn-ignore-self-hosted-server: the local server started immediately above, in this same process

    def _attempts(self, method, status, retry_after=None):
        """Send one request and report how many attempts actually reached the server.

        Both outcomes are deliberately accepted, because which one happens is
        the very thing under test: a request retried to exhaustion surfaces as a
        `RetryError`, while one that is not retried at all simply returns its
        error response. Asserting on the exception would assert the answer
        instead of measuring it -- which is what the first draft of this helper
        did, making every non-retried case fail for the wrong reason.
        """
        url, counts = self._serve(status, retry_after)
        try:
            getattr(session, method.lower())(url, timeout=10)
        except requests.exceptions.RequestException as exc:
            _logger.info("Probe %s expectedly ended in %s: %s", method, type(exc).__name__, exc)
        return counts[method]

    def test_08_retry_policy_excludes_non_idempotent_methods(self):
        # [@ANCHOR: test_cf_retry_policy_excludes_non_idempotent_methods]

        # Tests [@ANCHOR: cloudflare:COMM_idempotency_aware_retry]
        """POST and PATCH are retryable on 429 only; the rest keep the 5xx list."""
        self.assertNotIn("POST", retry_strategy.allowed_methods)
        self.assertNotIn("PATCH", retry_strategy.allowed_methods)
        self.assertTrue(retry_strategy.respect_retry_after_header)

        for method in ("GET", "HEAD", "OPTIONS", "PUT", "DELETE"):
            for status in (429, 500, 502, 503, 504):
                self.assertTrue(
                    retry_strategy.is_retry(method, status),
                    f"{method} {status} should be retried: it cannot be applied twice",
                )

        for method in ("POST", "PATCH"):
            self.assertTrue(
                retry_strategy.is_retry(method, 429),
                f"{method} 429 should be retried: a rate-limit refusal is never applied",
            )
            for status in (500, 502, 503, 504):
                self.assertFalse(
                    retry_strategy.is_retry(method, status),
                    f"{method} {status} must NOT be retried: Cloudflare may have applied it",
                )

    def test_09_a_post_answered_with_502_reaches_the_server_once(self):
        # [@ANCHOR: test_cf_post_502_is_not_resent]
        """A 502 must not replay a POST, while an idempotent method still retries."""
        self.assertEqual(
            self._attempts("POST", 502),
            1,
            "a POST answered 502 was re-sent; Cloudflare may already have applied it",
        )
        self.assertEqual(
            self._attempts("GET", 502),
            4,
            "the idempotent path lost its retries; this fix must not disable them",
        )

    def test_10_a_rate_limited_post_is_still_retried(self):
        # [@ANCHOR: test_cf_post_429_is_retried]
        """429 is refused before it is applied, so replaying a POST is safe."""
        self.assertEqual(
            self._attempts("POST", 429, retry_after=0),
            4,
            "a rate-limited POST should be retried rather than dropped",
        )
