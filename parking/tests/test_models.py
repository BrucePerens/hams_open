# SPDX-License-Identifier: AGPL-3.0-or-later
from psycopg2 import IntegrityError

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.exceptions import AccessError, ValidationError
from odoo.tests import tagged
from odoo.tools import mute_logger



@tagged("post_install", "-at_install", "parking")
class TestParkingModels(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.Domain = self.env["parking.domain"]

    # [@ANCHOR: parking:COMM_test_domain_constraints]
    def test_names_are_normalized_and_unique(self):
        # Tests [@ANCHOR: parking:COMM_domain_constraints]
        record = self.Domain.create({"name": "Bücher.Example"})
        self.assertEqual(record.name, "xn--bcher-kva.example")
        with self.assertRaises(IntegrityError), mute_logger("odoo.sql_db"):
            with self.cr.savepoint():
                self.Domain.create({"name": "xn--bcher-kva.example"})
                self.env.flush_all()

    def test_invalid_names_are_refused(self):
        for bad in ("intranet", "1.2.3.4", "a b.example", "http://x.example"):
            with self.assertRaises(ValidationError, msg=bad):
                self.Domain.create({"name": bad})
                self.env.flush_all()

    def test_redirect_targets_are_validated(self):
        for url in (False, "javascript:alert(1)", "https://own.example/x", "https://u:p@x.example"):
            with self.assertRaises(ValidationError, msg=str(url)):
                self.Domain.create({"name": "own.example", "behavior": "redirect", "redirect_url": url})
                self.env.flush_all()
        ok = self.Domain.create(
            {"name": "own2.example", "behavior": "redirect", "redirect_url": "https://t.example/"}
        )
        self.assertEqual(ok.redirect_code, "301")

    def test_ttl_range(self):
        with self.assertRaises(ValidationError):
            self.Domain.create({"name": "ttl.example", "cache_ttl": -1})
            self.env.flush_all()

    def test_lookup_exact_and_www(self):
        record = self.Domain.create({"name": "lookup.example"})
        other = self.Domain.create({"name": "nowww.example", "include_www": False})
        self.assertEqual(self.Domain._lookup("lookup.example"), record)
        self.assertEqual(self.Domain._lookup("www.lookup.example"), record)
        self.assertFalse(self.Domain._lookup("www.nowww.example"))
        own_www = self.Domain.create({"name": "www.lookup.example", "behavior": "gone"})
        self.assertEqual(self.Domain._lookup("www.lookup.example"), own_www)
        self.assertFalse(self.Domain._lookup("absent.example"))
        self.assertTrue(other)

    # [@ANCHOR: parking:COMM_test_service_account_acl]
    def test_service_account_can_read_domains_and_write_inquiries_only(self):
        # Tests [@ANCHOR: parking:COMM_service_account_acl]
        domain = self.Domain.create({"name": "svc.example", "notes": "internal"})
        uid = self.env["ir.model.data"]._xmlid_to_res_id("parking.user_parking_service")
        service = self.env(user=uid)
        self.assertEqual(service["parking.domain"].search([("name", "=", "svc.example")]).id, domain.id)
        with self.assertRaises(AccessError):
            service["parking.domain"].browse(domain.id).write({"title": "x"})
        with self.assertRaises(AccessError):
            service["parking.domain"].create({"name": "no.example"})
        with self.assertRaises(AccessError):
            service["ir.config_parameter"].search([])
        with self.assertRaises(AccessError):
            service["res.users"].search([])
        inquiry = service["parking.inquiry"].create(
            {"domain_id": domain.id, "email": "a@b.example", "message": "hi"}
        )
        self.assertEqual(inquiry.read(["message"])[0]["message"], "hi")  # needed for rate limiting
        with self.assertRaises(AccessError):
            inquiry.unlink()
        with self.assertRaises(AccessError):
            inquiry.write({"message": "changed"})

    def test_the_public_user_has_no_access_to_any_parking_model(self):
        public = self.env(user=self.env.ref("base.public_user"))
        with self.assertRaises(AccessError):
            public["parking.domain"].search([])
        with self.assertRaises(AccessError):
            public["parking.inquiry"].search([])

    def test_internal_notes_are_hidden_from_non_managers(self):
        uid = self.env["ir.model.data"]._xmlid_to_res_id("parking.user_parking_service")
        domain = self.Domain.create({"name": "notes.example", "notes": "secret note"})
        with self.assertRaises(AccessError):
            domain.with_user(uid).read(["notes"])

    # [@ANCHOR: parking:COMM_test_views_render]
    def test_backend_views_render(self):
        for view_type in ("list", "form", "search"):
            self.assertTrue(self.Domain.get_view(view_type=view_type)["arch"])
        self.assertTrue(self.env["parking.inquiry"].get_view(view_type="list")["arch"])
