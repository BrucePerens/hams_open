# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.exceptions import ValidationError
from odoo.tests.common import tagged


@tagged("post_install", "-at_install")
class TestDnsRecordCnameAndLength(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.Record = self.env["cloudflare.dns.record"]
        self.Record.search([]).unlink()

    def _make(self, **vals):
        return self.Record.create(
            dict({"name": "www.cname-rule.example", "type": "A", "content": "66.135.10.56", "proxied": False}, **vals)
        )

    def _refused(self, **vals):
        with self.assertRaises(ValidationError), self.env.cr.savepoint():
            self._make(**vals)
            self.env.flush_all()

    def test_a_cname_cannot_share_a_name_with_another_row(self):
        # Tests [@ANCHOR: COMM_test_dns_record_constraints]
        # Cloudflare refuses the push of such a row, at push time and per row.
        self._make()
        self._refused(type="CNAME", content="target.example.org")
        self.Record.search([]).unlink()
        self._make(type="CNAME", content="target.example.org")
        self._refused(type="TXT", content="v=spf1 -all")
        self._refused(content="66.135.10.57")
        self._make(name="other.cname-rule.example", content="66.135.10.58")

    def test_an_archived_row_does_not_clash_with_a_cname(self):
        # Tests [@ANCHOR: COMM_test_dns_record_constraints]
        self._make(active=False)
        self._make(type="CNAME", content="target.example.org")

    def test_the_whole_name_is_at_most_253_characters(self):
        # Tests [@ANCHOR: COMM_test_dns_record_constraints]
        label = "a" * 60
        fits = ".".join([label] * 4 + ["example"])  # 60 * 4 + 3 dots + 8 = 251
        self._make(name=fits)
        self._refused(name=".".join([label] * 4 + ["example", "com"]))
