# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
"""
`action_ensure_tunnel_running` on a server that fronts SEVERAL websites.

It used to do `self.env["cloudflare.tunnel"].search([], limit=1)`: on a server
with more than one tunnel record it operated on the lowest-id one and silently
ignored every other website's tunnel, with no error and no log. Bruce settled
the scope on 2026-09-19 (`hams_com/night_shift_questions/answered/
cloudflare-tunnel-ensure-running-multi-tunnel-scope-ec6882e6.md`): "It ends up
that one server fronting multiple web sites is the common case. I will probably
front hams.com, perens.com, and postopen.org on the same server ... So, make
that work correctly."

Every test here drives the real method with a STUB daemon starter, so what is
asserted is which tunnels the method decided to act on -- never a real
`cloudflared`, a real thread, or a real Cloudflare call.
"""
from concurrent.futures import Future

from cryptography.fernet import Fernet
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.cloudflare.utils import cloudflare_daemon as cf_daemon
from odoo.addons.cloudflare.models.tunnel import (
    LEGACY_PROVISIONED_MIGRATED,
    LEGACY_PROVISIONED_PARAM,
)


@tagged("post_install", "-at_install")
class TestTunnelMultiWebsite(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        # Every test here asserts on counts across ALL tunnels, which is
        # exactly what the method now iterates, so start from a known-empty
        # table rather than from whatever a combined test run left behind.
        # One search, outside any loop (an in-loop one is a real burn-list
        # N+1 finding, not a style preference).
        self.env["cloudflare.tunnel"].search([], limit=10000).unlink()
        self.Tunnel = self.env["cloudflare.tunnel"]
        self.utils = self.env["zero_sudo.security.utils"]
        self.svc_uid = self.utils._get_service_uid(
            "cloudflare.user_cloudflare_tunnel"
        )
        mock_fernet = self.safe_patch(
            "odoo.addons.cloudflare.models.website.WebsiteCloudflare._get_fernet"
        )
        mock_fernet.return_value = Fernet(Fernet.generate_key())

    def _website(self, slug, with_credentials=True):
        website = self.env["website"].create(
            {
                "name": "Multi Tunnel %s" % slug,
                "domain": "https://%s.example.com" % slug,
            }
        )
        if with_credentials:
            website.write(
                {
                    "cloudflare_api_token": "tok-%s" % slug,
                    "cloudflare_zone_id": "zone-%s" % slug,
                    "cloudflare_account_id": "acct-%s" % slug,
                }
            )
        return website

    def _tunnel(self, slug, with_credentials=True, provisioned=False):
        tunnel = self.Tunnel.create(
            {
                "cf_tunnel_id": "cftun-%s" % slug,
                "name": "Tunnel %s" % slug,
                "website_id": self._website(slug, with_credentials).id,
            }
        )
        if provisioned:
            tunnel.routes_provisioned = True
        return tunnel

    def _stub_daemon(self, running=False):
        """Stub out both halves of the daemon layer; return the start stub."""
        self.safe_patch(
            "odoo.addons.cloudflare.models.tunnel.is_tunnel_daemon_running",
            return_value=running,
        )
        return self.safe_patch(
            "odoo.addons.cloudflare.models.tunnel.start_tunnel_daemon"
        )

    def _stub_token(self, return_value=(True, "run-token")):
        return self.safe_patch(
            "odoo.addons.cloudflare.models.tunnel.get_cfd_tunnel_token",
            return_value=return_value,
        )

    def _stub_push(self):
        return self.safe_patch(
            "odoo.addons.cloudflare.models.tunnel.CloudflareTunnel.action_push_configuration",
            return_value=True,
        )

    def test_01_every_website_s_tunnel_is_started_not_just_the_first(self):
        # Tests [@ANCHOR: COMM_ensure_tunnel_running]
        """Three websites, three tunnels: all three daemons are started.

        This is the whole point of the change. Under the old
        `search([], limit=1)` this test would have seen exactly one call, for
        the lowest-id tunnel, and no error about the other two.
        """
        tunnels = [self._tunnel(slug) for slug in ("hams", "perens", "postopen")]
        self._stub_token()
        self._stub_push()
        start = self._stub_daemon()

        summary = self.Tunnel.action_ensure_tunnel_running()

        self.assertEqual(summary["started"], 3)
        self.assertEqual(summary["failed"], 0)
        self.assertEqual(summary["skipped"], 0)
        started_keys = sorted(
            call.kwargs["tunnel_key"] for call in start.call_args_list
        )
        self.assertEqual(
            started_keys,
            sorted(t.cf_tunnel_id for t in tunnels),
            "Each tunnel MUST be started under its OWN key, so the daemons "
            "are tracked independently rather than sharing one slot.",
        )

    def test_02_a_tunnel_already_running_is_not_restarted(self):
        # Tests [@ANCHOR: COMM_ensure_tunnel_running]
        """An already-provisioned, already-running tunnel costs nothing.

        Not merely "is not started twice": it must not even reach Cloudflare
        for a run token, or a five-minute cron would make one API call per
        tunnel per tick forever on a server fronting a dozen sites.
        """
        self._tunnel("running", provisioned=True)
        token = self._stub_token()
        start = self._stub_daemon(running=True)

        summary = self.Tunnel.action_ensure_tunnel_running()

        self.assertEqual(summary["already_running"], 1)
        self.assertEqual(summary["started"], 0)
        start.assert_not_called()
        token.assert_not_called()

    def test_03_one_failing_tunnel_does_not_stop_the_others(self):
        # Tests [@ANCHOR: COMM_ensure_one_tunnel_running]
        """A Cloudflare failure on one tunnel is logged and stepped over.

        The failing tunnel is deliberately the MIDDLE one by id, so a method
        that aborted on the first failure would still have started the first
        tunnel and this test would still catch it by the missing third.
        """
        good_a = self._tunnel("good-a")
        bad = self._tunnel("bad")
        good_b = self._tunnel("good-b")
        self._stub_push()
        start = self._stub_daemon()

        def fake_token(_account_id, _token, cf_tunnel_id):
            if cf_tunnel_id == bad.cf_tunnel_id:
                return (False, "simulated Cloudflare 502")
            return (True, "run-token")

        self.safe_patch(
            "odoo.addons.cloudflare.models.tunnel.get_cfd_tunnel_token",
            side_effect=fake_token,
        )

        summary = self.Tunnel.action_ensure_tunnel_running()

        self.assertEqual(summary["failed"], 1)
        self.assertEqual(summary["started"], 2)
        self.assertEqual(
            sorted(call.kwargs["tunnel_key"] for call in start.call_args_list),
            sorted([good_a.cf_tunnel_id, good_b.cf_tunnel_id]),
            "The two healthy tunnels MUST both still be started; the failure "
            "belongs to its own tunnel only.",
        )

    def test_04_the_provisioned_marker_is_per_tunnel(self):
        # Tests [@ANCHOR: COMM_ensure_one_tunnel_running]
        """Routes are pushed for the un-provisioned tunnel only.

        Under the old single global `cloudflare.tunnel.provisioned` parameter,
        one tunnel being provisioned suppressed the FIRST route push of every
        other tunnel on the server -- permanently, since nothing ever cleared
        the flag. This is the regression test for that.
        """
        already = self._tunnel("already", provisioned=True)
        fresh = self._tunnel("fresh")
        self._stub_token()
        push = self._stub_push()
        self._stub_daemon()

        self.Tunnel.action_ensure_tunnel_running()

        self.assertEqual(
            push.call_count,
            1,
            "Exactly one route push: the already-provisioned tunnel must not "
            "be pushed again, and the fresh one must not be skipped.",
        )
        self.assertTrue(fresh.routes_provisioned)
        self.assertTrue(already.routes_provisioned)

    def test_05_legacy_global_flag_migrates_onto_the_lowest_id_tunnel(self):
        # Tests [@ANCHOR: COMM_migrate_global_provisioned_flag]
        """An already-provisioned single-tunnel install is not re-provisioned.

        The old flag could only ever mean "the tunnel `search([], limit=1)`
        picked has been provisioned", i.e. the lowest id. Folding it onto ALL
        existing tunnels would wrongly suppress the second tunnel's first
        route push forever, which is the failure this asserts against.
        """
        first = self._tunnel("legacy-first")
        second = self._tunnel("legacy-second")
        self.assertLess(
            first.id, second.id, "test premise: 'first' must be the lower id"
        )
        self.utils.with_user(self.svc_uid)._set_system_param(
            LEGACY_PROVISIONED_PARAM, "True"
        )
        self.env.registry.clear_cache()
        self._stub_token()
        push = self._stub_push()
        self._stub_daemon()

        self.Tunnel.action_ensure_tunnel_running()

        self.assertTrue(
            first.routes_provisioned,
            "The tunnel the old global flag was actually about MUST inherit "
            "it, so an already-provisioned install is not re-provisioned.",
        )
        self.assertEqual(
            push.call_count,
            1,
            "Only the SECOND tunnel -- which the old flag never described -- "
            "may be pushed.",
        )
        self.assertTrue(second.routes_provisioned)
        self.assertEqual(
            self.utils.with_user(self.svc_uid)
            .with_context(redis_bypass_cache=True)
            ._get_system_param(LEGACY_PROVISIONED_PARAM),
            LEGACY_PROVISIONED_MIGRATED,
            "The parameter MUST be rewritten to the sentinel, not deleted: a "
            "rollback to the previous code then still finds a truthy value.",
        )

    def test_06_the_migration_runs_once_and_is_a_no_op_afterwards(self):
        # Tests [@ANCHOR: COMM_migrate_global_provisioned_flag]
        """Called on every cron tick, so it must be idempotent and cheap."""
        tunnel = self._tunnel("idempotent")
        self.utils.with_user(self.svc_uid)._set_system_param(
            LEGACY_PROVISIONED_PARAM, "True"
        )
        self.env.registry.clear_cache()

        self.assertTrue(self.Tunnel._migrate_global_provisioned_flag())
        self.assertTrue(tunnel.routes_provisioned)

        # Clearing the field would be how a second migration run would show
        # itself: an idempotent one leaves it alone.
        tunnel.routes_provisioned = False
        self.assertFalse(self.Tunnel._migrate_global_provisioned_flag())
        self.assertFalse(
            tunnel.routes_provisioned,
            "A second migration run MUST NOT re-apply the legacy flag.",
        )

    def test_07_an_install_that_never_had_the_flag_migrates_nothing(self):
        # Tests [@ANCHOR: COMM_migrate_global_provisioned_flag]
        tunnel = self._tunnel("never-flagged")
        self.utils.with_user(self.svc_uid)._set_system_param(
            LEGACY_PROVISIONED_PARAM, False
        )
        self.env.registry.clear_cache()

        self.assertFalse(self.Tunnel._migrate_global_provisioned_flag())
        self.assertFalse(tunnel.routes_provisioned)

    def test_08_a_tunnel_whose_website_has_no_credentials_is_skipped(self):
        # Tests [@ANCHOR: COMM_ensure_one_tunnel_running]
        """Skipped, not failed, and without stopping the tunnel next to it."""
        self._tunnel("uncredentialed", with_credentials=False)
        good = self._tunnel("credentialed")
        self._stub_token()
        self._stub_push()
        start = self._stub_daemon()

        summary = self.Tunnel.action_ensure_tunnel_running()

        self.assertEqual(summary["skipped"], 1)
        self.assertEqual(summary["started"], 1)
        self.assertEqual(summary["failed"], 0)
        start.assert_called_once_with("run-token", tunnel_key=good.cf_tunnel_id)

    def test_09_no_tunnels_at_all_is_a_clean_no_op(self):
        # Tests [@ANCHOR: COMM_ensure_tunnel_running]
        """Zero tunnels: an all-zero summary, and NOTHING else touched.

        The migration assertion is the load-bearing half. `search([])` on a
        model the caller has no ACL row for raises only once there are records
        to check, so on an empty install the search alone does not gate an
        unauthorized caller -- the method must return before reaching the
        migration's whitelisted ir.config_parameter read, which is what keeps
        "the ACL-gated tunnel search is the only thing an unauthorized caller
        can reach" true for every install and not just populated ones.
        """
        start = self._stub_daemon()
        token = self._stub_token()
        migrate = self.safe_patch(
            "odoo.addons.cloudflare.models.tunnel.CloudflareTunnel."
            "_migrate_global_provisioned_flag"
        )

        summary = self.Tunnel.action_ensure_tunnel_running()

        self.assertEqual(
            summary,
            {"started": 0, "already_running": 0, "skipped": 0, "failed": 0},
        )
        start.assert_not_called()
        token.assert_not_called()
        migrate.assert_not_called()

    def test_10_each_tunnel_key_is_tracked_independently_in_the_daemon_layer(self):
        # Tests [@ANCHOR: COMM_is_tunnel_daemon_running]
        """The daemon layer answers "is this one running?" per key.

        Exercised against the real module state rather than a mock, with no
        daemon ever started: an unknown key must read as not-running, and a
        key whose recorded future has finished must read as not-running too,
        which is what makes a DIED daemon get restarted instead of being
        mistaken for a healthy one.
        """
        self.assertFalse(cf_daemon.is_tunnel_daemon_running("cftun-never-seen"))

        finished = Future()
        finished.set_result(None)
        live = Future()
        cf_daemon._tunnel_futures["cftun-test-dead"] = finished
        cf_daemon._tunnel_futures["cftun-test-live"] = live
        try:
            self.assertFalse(
                cf_daemon.is_tunnel_daemon_running("cftun-test-dead"),
                "A finished daemon loop MUST read as not running, so the next "
                "'ensure' restarts it.",
            )
            self.assertTrue(
                cf_daemon.is_tunnel_daemon_running("cftun-test-live"),
                "A live daemon loop MUST read as running, so the next "
                "'ensure' leaves it alone.",
            )
            self.assertFalse(
                cf_daemon.is_tunnel_daemon_running("cftun-test-live-typo"),
                "Keys MUST NOT bleed into one another.",
            )
        finally:
            live.set_result(None)
            cf_daemon._tunnel_futures.pop("cftun-test-dead", None)
            cf_daemon._tunnel_futures.pop("cftun-test-live", None)
