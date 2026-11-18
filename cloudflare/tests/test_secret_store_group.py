# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""Group `cloudflare` of hams_module_secrets_from_store: the API token and the Turnstile secret come from the
secret store and the encrypted columns are emptied. Test values only. Story:
zero_sudo/docs/stories/secret_store_tier_a.md ("Module secrets")."""

from cryptography.fernet import Fernet

from odoo.exceptions import UserError
from odoo.tests import tagged

from odoo.addons.zero_sudo.daemon import secret_store as ss
from odoo.addons.zero_sudo.models import module_secrets as ms
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

TOKEN = "test-cf-token-0000000000000000000000"
OTHER = "test-cf-token-1111111111111111111111"
TURNSTILE = "test-turnstile-secret-22222222222"


class _DictBackend:
    label = "test"

    def __init__(self, values):
        self.values = values

    def get(self, name):
        return self.values.get(ss.check_name(name))


@tagged("post_install", "-at_install")
class TestCloudflareSecretStoreGroup(HamsTransactionCase):

    def setUp(self):
        super().setUp()
        fernet = Fernet(Fernet.generate_key())
        self.safe_patch("odoo.addons.cloudflare.models.website.WebsiteCloudflare._get_fernet").return_value = fernet
        self.website = self.env["website"].create({"name": "Store group site"})
        self.store_values = {"cloudflare.api_token": TOKEN, "cloudflare.turnstile_secret": TURNSTILE}
        self.safe_patch_object(
            ms, "module_secret_store", side_effect=lambda: ss.SecretStore([_DictBackend(self.store_values)]))

    def _group(self, on):
        self.safe_patch_object(ms, "enabled_groups", return_value=frozenset({"cloudflare"}) if on else frozenset())

    def _columns(self, site):
        site.flush_recordset()
        self.env.cr.execute(
            "SELECT cloudflare_api_token_crypt, cloudflare_turnstile_secret_crypt FROM website WHERE id = %s", [site.id])
        return self.env.cr.fetchone()

    # Tests [@ANCHOR: cloudflare:COMM_compute_encrypted_field]
    def test_group_off_keeps_the_encrypted_columns(self):
        self._group(False)
        self.website.write({"cloudflare_api_token": OTHER})
        self.assertEqual(self.website.cloudflare_api_token, OTHER)
        self.assertTrue(self._columns(self.website)[0])

    # Tests [@ANCHOR: cloudflare:COMM_compute_encrypted_field]
    # Tests [@ANCHOR: cloudflare:COMM_inverse_encrypted_field]
    def test_group_on_serves_the_store_and_refuses_a_typed_secret(self):
        self._group(True)
        self.assertEqual(self.website.cloudflare_api_token, TOKEN)
        self.assertEqual(self.website.cloudflare_turnstile_secret, TURNSTILE)
        token, _zone = self.website._get_cloudflare_credentials()
        self.assertEqual(token, TOKEN)
        with self.assertRaises(UserError):
            self.website.write({"cloudflare_api_token": OTHER})
        self.assertEqual(self._columns(self.website), (None, None))
        self.store_values.clear()
        self.website.invalidate_recordset()
        self.assertFalse(self.website.cloudflare_api_token)

    # Tests [@ANCHOR: cloudflare:store_clear_from_database]
    def test_clear_step_empties_the_columns_and_keeps_the_module_working(self):
        self._group(False)
        self.website.write({"cloudflare_api_token": TOKEN, "cloudflare_turnstile_secret": TURNSTILE})
        self.assertTrue(all(self._columns(self.website)))
        self._group(True)
        cleared = self.env["website"]._hams_cloudflare_clear_secrets_from_database()
        self.assertEqual(sorted(cleared), ["cloudflare_api_token_crypt", "cloudflare_turnstile_secret_crypt"])
        self.assertEqual(self._columns(self.website), (None, None))
        self.website.invalidate_recordset()
        self.assertEqual(self.website._get_cloudflare_credentials()[0], TOKEN)

    # Tests [@ANCHOR: cloudflare:store_clear_from_database]
    def test_clear_step_refuses_a_different_token_or_a_missing_store_value(self):
        self._group(False)
        self.website.write({"cloudflare_api_token": OTHER})
        self._group(True)
        with self.assertRaises(UserError):
            self.env["website"]._hams_cloudflare_clear_secrets_from_database()
        self.store_values.clear()
        with self.assertRaises(ss.SecretNotFound):
            self.env["website"]._hams_cloudflare_clear_secrets_from_database()
        self.assertTrue(self._columns(self.website)[0])
        self._group(False)
        with self.assertRaises(UserError):
            self.env["website"]._hams_cloudflare_clear_secrets_from_database()

    # Tests [@ANCHOR: cloudflare:store_check_secrets]
    # Tests [@ANCHOR: cloudflare:store_register_hook]
    def test_startup_check_fails_when_the_store_lacks_a_secret_the_database_has(self):
        self._group(False)
        self.website.write({"cloudflare_api_token": TOKEN})
        self._group(True)
        self.env["website"]._check_cloudflare_secrets_in_store()
        self.store_values.pop("cloudflare.api_token")
        with self.assertRaises(ss.SecretNotFound):
            self.env["website"]._check_cloudflare_secrets_in_store()
