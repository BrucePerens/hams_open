# SPDX-License-Identifier: AGPL-3.0-or-later
# Copyright © Bruce Perens K6BP.
"""Refusal branches that no other test reached: a token file outside the keys directory is never removed (the
existing test used a key that never left the directory), an unlisted HTTP method, a zone id that is not a Cloudflare
id, an undecryptable stored secret, TXT content bounds, and the purge wizard's refusals and URL handling."""
import os
import tempfile

from cryptography.fernet import Fernet

from odoo.exceptions import UserError, ValidationError
from odoo.tests.common import tagged

from odoo.addons.cloudflare.utils import cloudflare_api
from odoo.addons.cloudflare.utils import cloudflare_systemd as cf_systemd
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

WIZARD = "odoo.addons.cloudflare.models.purge_wizard."


@tagged("post_install", "-at_install")
class TestCloudflareRefusals(HamsTransactionCase):
    def test_a_key_that_climbs_out_of_the_keys_directory_removes_nothing(self):
        # Tests [@ANCHOR: cloudflare:COMM_systemd_remove_env_file]
        with tempfile.TemporaryDirectory() as tmp:
            keys_dir = os.path.join(tmp, "keys")
            os.makedirs(keys_dir)
            victim = os.path.join(tmp, "victim.env")
            with open(victim, "w") as handle:
                handle.write("TUNNEL_TOKEN=x")
            self.safe_patch_object(cf_systemd, "_KEYS_DIR", keys_dir)
            with self.assertLogs("odoo.addons.cloudflare.utils.cloudflare_systemd", level="ERROR") as logs:
                cf_systemd._remove_env_file("x/../../victim")
            self.assertIn("Refusing to remove", logs.output[0])
            self.assertTrue(os.path.exists(victim))

    def test_a_token_file_that_cannot_be_removed_is_logged_not_raised(self):
        # Tests [@ANCHOR: cloudflare:COMM_systemd_remove_env_file]
        with tempfile.TemporaryDirectory() as tmp:
            self.safe_patch_object(cf_systemd, "_KEYS_DIR", tmp)
            path = cf_systemd._env_file_path("cftun-stuck")
            os.mkdir(path)  # os.remove refuses a directory, the same OSError a permission problem gives
            with self.assertLogs("odoo.addons.cloudflare.utils.cloudflare_systemd", level="WARNING") as logs:
                cf_systemd._remove_env_file("cftun-stuck")
            self.assertIn("Could not remove the token file", logs.output[0])

    def test_an_http_method_the_client_does_not_list_is_refused_before_any_request(self):
        # Tests [@ANCHOR: cloudflare:COMM_make_request]
        session = self.safe_patch_object(cloudflare_api, "session")
        with self.assertRaises(ValueError):
            cloudflare_api._make_request("TRACE", "https://api.cloudflare.com/client/v4/zones", "tok", "err")
        self.assertEqual(session.method_calls, [])

    def test_zone_lookups_without_credentials_or_with_a_path_like_zone_id_send_nothing(self):
        # Tests [@ANCHOR: cloudflare:COMM_dns_api_find_zone_id]
        # Tests [@ANCHOR: cloudflare:COMM_dns_api_get_zone_details]
        request = self.safe_patch_object(cloudflare_api, "_make_request")
        self.assertEqual(cloudflare_api.find_zone_id("hams.com", ""), (False, "Missing credentials or zone name"))
        self.assertEqual(cloudflare_api.find_zone_id("", "tok"), (False, "Missing credentials or zone name"))
        self.assertEqual(cloudflare_api.get_zone_details("../account", "tok"), (False, "Missing credentials or zone"))
        self.assertEqual(cloudflare_api.get_zone_details("0" * 32, ""), (False, "Missing credentials or zone"))
        request.assert_not_called()

    def test_a_stored_secret_that_cannot_be_decrypted_reads_as_the_error_marker(self):
        # Tests [@ANCHOR: cloudflare:COMM_crypt_field]
        website = self.env["website"].search([], limit=1)
        self.safe_patch_object(type(website), "_get_fernet", return_value=Fernet(Fernet.generate_key()))
        foreign = Fernet(Fernet.generate_key()).encrypt(b"token").decode()
        with self.assertLogs("odoo.addons.cloudflare.models.website", level="WARNING"):
            self.assertEqual(website._crypt_field("not-a-fernet-token", decrypt=True), "***ERROR***")
            self.assertEqual(website._crypt_field(foreign, decrypt=True), "***ERROR***")

    def test_txt_content_must_be_one_to_2048_characters(self):
        # Tests [@ANCHOR: cloudflare:COMM_dns_record_constraints]
        Record = self.env["cloudflare.dns.record"]
        vals = {"name": "txt-bounds.hams.com", "type": "TXT", "proxied": False}
        for bad in ("", "   ", "x" * 2049):
            with self.assertRaises(ValidationError, msg=repr(bad[:5])):
                Record.create(dict(vals, content=bad))
                self.env.flush_all()
        self.assertTrue(Record.create(dict(vals, content="x" * 2048)))


@tagged("post_install", "-at_install")
class TestPurgeWizardRefusals(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.website = self.env["website"].search([], limit=1)
        self.website.write({"domain": "https://site.example"})
        self.credentials = self.safe_patch_object(
            type(self.website), "_get_cloudflare_credentials", return_value=("tok", "zone")
        )
        self.purge_urls = self.safe_patch(WIZARD + "purge_urls", return_value=True)
        self.purge_tags = self.safe_patch(WIZARD + "purge_tags", return_value=True)

    def _wizard(self, purge_type, items=""):
        return self.env["cloudflare.purge.wizard"].create(
            {"website_id": self.website.id, "purge_type": purge_type, "items_to_purge": items}
        )

    def test_a_website_without_credentials_is_refused_before_any_purge(self):
        # Tests [@ANCHOR: cloudflare:COMM_purge_wizard_action_purge]
        self.credentials.return_value = (False, False)
        with self.assertRaises(UserError):
            self._wizard("urls", "/a").action_purge()
        self.purge_urls.assert_not_called()

    def test_no_items_is_refused(self):
        # Tests [@ANCHOR: cloudflare:COMM_purge_wizard_action_purge]
        for purge_type in ("urls", "tags"):
            with self.assertRaises(UserError):
                self._wizard(purge_type, " ,\n , ").action_purge()
        self.purge_urls.assert_not_called()
        self.purge_tags.assert_not_called()

    def test_relative_urls_get_the_site_domain_and_commas_split_items(self):
        # Tests [@ANCHOR: cloudflare:COMM_purge_wizard_action_purge]
        result = self._wizard("urls", "/a, https://site.example/b\n/c").action_purge()
        self.assertEqual(result["tag"], "display_notification")
        self.purge_urls.assert_called_once_with(
            ["https://site.example/a", "https://site.example/b", "https://site.example/c"], "tok", "zone"
        )

    def test_tags_are_passed_as_a_list_and_a_refused_purge_is_an_error(self):
        # Tests [@ANCHOR: cloudflare:COMM_purge_wizard_action_purge]
        self.assertEqual(self._wizard("tags", "t1,t2").action_purge()["tag"], "display_notification")
        self.purge_tags.assert_called_once_with(["t1", "t2"], "tok", "zone")
        self.purge_tags.return_value = False
        with self.assertRaises(UserError):
            self._wizard("tags", "t1").action_purge()
        self.purge_urls.return_value = False
        with self.assertRaises(UserError):
            self._wizard("urls", "/a").action_purge()
