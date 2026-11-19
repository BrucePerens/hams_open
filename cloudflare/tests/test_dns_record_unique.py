# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""One active DNS row per (type, name, content) in the database, and the 1.13 migration archives existing duplicates."""
import importlib.util
import os

import psycopg2

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests.common import tagged
from odoo.tools import SQL, mute_logger

_MIGRATION = os.path.join(os.path.dirname(__file__), "..", "migrations", "1.13", "pre-dns-record-dedupe.py")
_VALS = {"name": "uniq.hams.com", "type": "A", "content": "66.135.10.99", "proxied": False}


@tagged("post_install", "-at_install")
class TestDnsRecordUnique(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.Record = self.env["cloudflare.dns.record"]
        self.Record.search([]).unlink()

    # [@ANCHOR: cloudflare:test_two_active_rows_for_one_record_are_impossible_in_the_database]
    def test_two_active_rows_for_one_record_are_impossible_in_the_database(self):
        """The Python check searches first, so two simultaneous creates both pass it. Straight SQL, which the
        check never sees, is refused too; an archived twin does not count."""
        first = self.Record.create(_VALS)
        self.Record.flush_model()
        insert = ("INSERT INTO cloudflare_dns_record (name, type, content, proxied, manage, active) "
                  "VALUES (%s, %s, %s, FALSE, TRUE, %s)")
        with mute_logger("odoo.sql_db"), self.assertRaises(psycopg2.IntegrityError):
            with self.env.cr.savepoint():
                self.env.cr.execute(insert, (first.name, first.type, first.content, True))
        with self.env.cr.savepoint():
            self.env.cr.execute(insert, (first.name, first.type, first.content, False))

    # Tests [@ANCHOR: cloudflare:migration_dns_record_dedupe]
    def test_migration_archives_all_but_the_linked_or_oldest_row(self):
        spec = importlib.util.spec_from_file_location("dns_record_dedupe", _MIGRATION)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        cr = self.env.cr
        cr.execute("SELECT indexname FROM pg_indexes WHERE tablename = 'cloudflare_dns_record' "
                   "AND indexname LIKE '%one_active_row_per_record%'")
        (index_name,) = cr.fetchone()
        cr.execute(SQL("DROP INDEX %s", SQL.identifier(index_name)))
        first = self.Record.create(_VALS)
        self.Record.flush_model()
        cr.execute(
            "INSERT INTO cloudflare_dns_record (name, type, content, proxied, manage, active, cf_record_id) "
            "VALUES (%s, %s, %s, FALSE, TRUE, TRUE, %s) RETURNING id",
            (first.name, first.type, first.content, "a" * 32))
        linked_id = cr.fetchone()[0]
        cr.execute(
            "INSERT INTO cloudflare_dns_record (name, type, content, proxied, manage, active) "
            "VALUES (%s, %s, %s, FALSE, TRUE, TRUE) RETURNING id", (first.name, first.type, first.content))
        newest_id = cr.fetchone()[0]
        module.migrate(cr, "1.12")
        cr.execute("SELECT id FROM cloudflare_dns_record WHERE name = %s AND active", (first.name,))
        self.assertEqual([r[0] for r in cr.fetchall()], [linked_id])
        cr.execute("SELECT count(*) FROM cloudflare_dns_record WHERE name = %s AND NOT active", (first.name,))
        self.assertEqual(cr.fetchone()[0], 2)
        self.assertNotEqual(first.id, newest_id)
