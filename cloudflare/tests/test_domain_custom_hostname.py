# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
"""
Real coverage for edge.routing.domain's Cloudflare custom-hostname sync
(models/domain.py) -- had zero test coverage before this file (confirmed
by grepping tests/ for the method names). Every function in that file
(create, unlink, _get_website_mapping,
_create_cloudflare_custom_hostname_batch,
_delete_cloudflare_custom_hostname_batch, action_sync_ssl_status) is
exercised here against a real website whose domain matches the routing
domain's own name, with only the outbound Cloudflare API calls mocked.
"""
from cryptography.fernet import Fernet
from odoo.exceptions import UserError
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase


@tagged("post_install", "-at_install")
class TestDomainCustomHostname(RealTransactionCase):
    def setUp(self):
        super().setUp()
        fernet_key = Fernet.generate_key()
        mock_fernet = self.safe_patch(
            "odoo.addons.cloudflare.models.website.WebsiteCloudflare._get_fernet"
        )
        mock_fernet.return_value = Fernet(fernet_key)

        self.website = self.env["website"].create(
            {"name": "Domain Sync Test Site", "domain": "https://domain-sync-test.example"}
        )
        self.website.write(
            {"cloudflare_api_token": "tok", "cloudflare_zone_id": "zone"}
        )

    def test_create_provisions_a_custom_hostname_for_the_matching_website(self):
        # Tests [@ANCHOR: cloudflare:COMM_domain_create]

        # Tests [@ANCHOR: cloudflare:COMM_get_website_mapping]

        # Tests [@ANCHOR: cloudflare:COMM_create_custom_hostname_batch]
        mock_create = self.safe_patch(
            "odoo.addons.cloudflare.models.domain.cf_utils.create_custom_hostname"
        )
        mock_create.return_value = (
            True,
            {"id": "hostname_123", "ssl": {"status": "pending_deployment"}},
        )

        domain = self.env["edge.routing.domain"].create(
            {
                "name": "https://domain-sync-test.example",
                "target_slug": "domain-sync-test",
            }
        )
        mock_create.assert_called_once_with(
            "https://domain-sync-test.example", "tok", "zone"
        )
        self.assertEqual(domain.cloudflare_hostname_id, "hostname_123")
        self.assertEqual(domain.ssl_status, "pending_deployment")

    def test_create_with_no_matching_website_raises_a_user_error(self):
        with self.assertRaises(UserError):
            self.env["edge.routing.domain"].create(
                {
                    "name": "https://no-such-website.example",
                    "target_slug": "no-such-website",
                }
            )
            self.env.flush_all()

    def test_unlink_deletes_the_custom_hostname_when_one_exists(self):
        # Tests [@ANCHOR: cloudflare:COMM_delete_custom_hostname_batch]
        mock_create = self.safe_patch(
            "odoo.addons.cloudflare.models.domain.cf_utils.create_custom_hostname"
        )
        mock_create.return_value = (True, {"id": "hostname_456", "ssl": {"status": "active"}})
        domain = self.env["edge.routing.domain"].create(
            {
                "name": "https://domain-sync-test.example",
                "target_slug": "domain-sync-test-2",
            }
        )

        mock_delete = self.safe_patch(
            "odoo.addons.cloudflare.models.domain.cf_utils.delete_custom_hostname"
        )
        domain.unlink()
        mock_delete.assert_called_once_with("hostname_456", "tok", "zone")

    def test_action_sync_ssl_status_updates_a_changed_status(self):
        # Tests [@ANCHOR: cloudflare:COMM_action_sync_ssl_status]
        mock_create = self.safe_patch(
            "odoo.addons.cloudflare.models.domain.cf_utils.create_custom_hostname"
        )
        mock_create.return_value = (
            True,
            {"id": "hostname_789", "ssl": {"status": "pending_deployment"}},
        )
        domain = self.env["edge.routing.domain"].create(
            {
                "name": "https://domain-sync-test.example",
                "target_slug": "domain-sync-test-3",
            }
        )
        self.assertEqual(domain.ssl_status, "pending_deployment")

        mock_get = self.safe_patch(
            "odoo.addons.cloudflare.models.domain.cf_utils.get_custom_hostname"
        )
        mock_get.return_value = (True, {"ssl": {"status": "active"}})
        domain.action_sync_ssl_status()
        self.assertEqual(domain.ssl_status, "active")
