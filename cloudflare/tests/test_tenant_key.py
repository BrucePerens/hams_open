# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
from cryptography.fernet import InvalidToken
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestCloudflareTenantKey(HamsTransactionCase):
    """
    Tests [@ANCHOR: cloudflare:COMM_tenant_key_get_or_create_fernet]

    Cloudflare ships to the open source community, where a single install
    commonly serves several unrelated companies (multi-company Odoo is a
    normal deployment shape). These pin the per-company key isolation
    _get_fernet() relies on: two companies must never share a working key,
    and the per-company key must be stable across repeated reads rather
    than regenerated (which would make previously-encrypted values
    permanently undecryptable).
    """

    def setUp(self):
        super().setUp()
        self.company_a = self.env["res.company"].create({"name": "Tenant A"})
        self.company_b = self.env["res.company"].create({"name": "Tenant B"})

    def test_two_companies_get_independent_keys(self):
        fernet_a = self.env["cloudflare.tenant.key"]._get_or_create_fernet(
            self.company_a
        )
        fernet_b = self.env["cloudflare.tenant.key"]._get_or_create_fernet(
            self.company_b
        )
        self.assertIsNotNone(fernet_a)
        self.assertIsNotNone(fernet_b)

        token_a = self.env["cloudflare.tenant.key"].search(
            [("company_id", "=", self.company_a.id)]
        )
        token_b = self.env["cloudflare.tenant.key"].search(
            [("company_id", "=", self.company_b.id)]
        )
        self.assertEqual(len(token_a), 1)
        self.assertEqual(len(token_b), 1)
        self.assertNotEqual(
            token_a.wrapped_key,
            token_b.wrapped_key,
            "Two different companies must never be issued the same "
            "wrapped per-tenant key.",
        )

        ciphertext = fernet_a.encrypt(b"tenant-a-secret")
        self.assertEqual(fernet_a.decrypt(ciphertext), b"tenant-a-secret")
        with self.assertRaises(InvalidToken):
            fernet_b.decrypt(ciphertext)

    def test_key_is_stable_across_repeated_calls(self):
        first = self.env["cloudflare.tenant.key"]._get_or_create_fernet(
            self.company_a
        )
        second = self.env["cloudflare.tenant.key"]._get_or_create_fernet(
            self.company_a
        )
        self.assertEqual(
            self.env["cloudflare.tenant.key"].search_count(
                [("company_id", "=", self.company_a.id)]
            ),
            1,
            "A second call for the same company must reuse the existing "
            "key, not generate and store a new one.",
        )
        ciphertext = first.encrypt(b"round-trips-across-calls")
        self.assertEqual(second.decrypt(ciphertext), b"round-trips-across-calls")

    def test_website_encryption_is_isolated_per_company(self):
        website_a = self.env["website"].create(
            {"name": "Site A", "company_id": self.company_a.id}
        )
        website_b = self.env["website"].create(
            {"name": "Site B", "company_id": self.company_b.id}
        )
        website_a.write({"cloudflare_turnstile_secret": "shared-plaintext-value"})
        website_b.write({"cloudflare_turnstile_secret": "shared-plaintext-value"})
        website_a.invalidate_recordset(
            ["cloudflare_turnstile_secret", "cloudflare_turnstile_secret_crypt"]
        )
        website_b.invalidate_recordset(
            ["cloudflare_turnstile_secret", "cloudflare_turnstile_secret_crypt"]
        )

        self.assertEqual(website_a.cloudflare_turnstile_secret, "shared-plaintext-value")
        self.assertEqual(website_b.cloudflare_turnstile_secret, "shared-plaintext-value")
        self.assertNotEqual(
            website_a.cloudflare_turnstile_secret_crypt,
            website_b.cloudflare_turnstile_secret_crypt,
            "The same plaintext for two different companies' websites "
            "must not produce ciphertext decryptable with the other "
            "company's key -- confirmed indirectly here since each "
            "round-trips correctly only under its own company's key.",
        )

        fernet_b = self.env["cloudflare.tenant.key"]._get_or_create_fernet(
            self.company_b
        )
        with self.assertRaises(InvalidToken):
            fernet_b.decrypt(website_a.cloudflare_turnstile_secret_crypt.encode("utf-8"))

    def test_get_or_create_fernet_returns_none_without_a_deployment_secret(self):
        mock_secret = self.safe_patch(
            "odoo.addons.zero_sudo.models.security_utils.ZeroSudoSecurityUtils"
            "._get_crypto_secret"
        )
        mock_secret.return_value = ""
        self.assertIsNone(
            self.env["cloudflare.tenant.key"]._get_or_create_fernet(self.company_a)
        )
