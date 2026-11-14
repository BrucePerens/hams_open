# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""One tunnel record per website and Cloudflare tunnel id, and the 1.12 migration removes existing duplicates."""
import importlib.util
import os

import psycopg2

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged
from odoo.tools import mute_logger

_MIGRATION = os.path.join(os.path.dirname(__file__), "..", "migrations", "1.12", "pre-tunnel-dedupe.py")


@tagged("post_install", "-at_install")
class TestTunnelUnique(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.website = self.env["website"].create({"name": "Tunnel Unique Site"})
        self.Tunnel = self.env["cloudflare.tunnel"]

    def _vals(self):
        return {"cf_tunnel_id": "uniq-tunnel-1", "name": "t", "website_id": self.website.id}

    # Tests [@ANCHOR: cloudflare:tunnel_one_record_per_website]
    # [@ANCHOR: test_a_second_record_for_the_same_tunnel_and_website_is_refused]
    def test_a_second_record_for_the_same_tunnel_and_website_is_refused(self):
        self.Tunnel.create(self._vals())
        with mute_logger("odoo.sql_db"), self.assertRaises(psycopg2.IntegrityError):
            with self.env.cr.savepoint():
                self.Tunnel.create(self._vals())

    # Tests [@ANCHOR: cloudflare:migration_tunnel_pair_dedupe]
    def test_migration_keeps_only_the_oldest_record(self):
        spec = importlib.util.spec_from_file_location("tunnel_pair_dedupe", _MIGRATION)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        cr = self.env.cr
        cr.execute("ALTER TABLE cloudflare_tunnel DROP CONSTRAINT cloudflare_tunnel_one_tunnel_per_website")
        first = self.Tunnel.create(self._vals())
        self.Tunnel.flush_model()
        cr.execute(
            "INSERT INTO cloudflare_tunnel (cf_tunnel_id, name, website_id, catch_all_service) "
            "SELECT cf_tunnel_id, name, website_id, catch_all_service FROM cloudflare_tunnel WHERE id = %s",
            (first.id,),
        )
        module.migrate(cr, "1.11")
        cr.execute("SELECT id FROM cloudflare_tunnel WHERE website_id = %s", (self.website.id,))
        self.assertEqual([r[0] for r in cr.fetchall()], [first.id])
