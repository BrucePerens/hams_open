# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
from unittest.mock import MagicMock

from odoo.exceptions import UserError
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.cloudflare.utils import cloudflare_api

API = "odoo.addons.cloudflare.utils.cloudflare_api._make_request"


def _resp(status, body):
    r = MagicMock()
    r.status_code = status
    r.json.return_value = body
    return r


def _answers(policies):
    return [
        _resp(200, {"result": {"id": "tok1", "status": "active"}}),
        _resp(200, {"result": {"policies": policies}}),
    ]


@tagged("post_install", "-at_install")
class TestTokenScope(HamsTransactionCase):
    """Recorded-shape provider responses; no real key is used."""

    def test_scoped_token_accepted(self):
        # Tests [@ANCHOR: cloudflare:COMM_dns_api_inspect_token]
        pol = [{"effect": "allow", "permission_groups": [{"name": "Zone DNS Write"}, {"name": "Cache Purge"}]}]
        self.safe_patch(API).side_effect = _answers(pol)
        self.assertEqual(cloudflare_api.inspect_token("t"), (True, []))

    def test_broad_token_reported(self):
        # Tests [@ANCHOR: cloudflare:COMM_dns_api_broad_token_permissions]
        pol = [{"effect": "allow", "permission_groups": [{"name": "API Tokens Write"}, {"name": "Zone DNS Write"}]}]
        self.safe_patch(API).side_effect = _answers(pol)
        self.assertEqual(cloudflare_api.inspect_token("t"), (True, ["API Tokens Write"]))

    def test_introspection_down_fails_closed(self):
        self.safe_patch(API).return_value = None
        ok, msg = cloudflare_api.inspect_token("t")
        self.assertFalse(ok)
        self.assertIn("could not verify", msg)

    def test_inactive_token_fails_closed(self):
        self.safe_patch(API).return_value = _resp(200, {"result": {"id": "x", "status": "expired"}})
        self.assertFalse(cloudflare_api.inspect_token("t")[0])

    def test_unreadable_answers_fail_closed(self):
        # [@ANCHOR: test_inspect_token_unreadable_answers]
        # Tests [@ANCHOR: cloudflare:COMM_dns_api_json_result]
        # A 200 whose body is not JSON, has no result object, or has no policy list is not "scoped".
        broken = MagicMock()
        broken.status_code = 200
        broken.json.side_effect = ValueError("not json")
        for first in (broken, _resp(200, {"result": None}), _resp(200, ["x"])):
            self.safe_patch(API).return_value = first
            ok, msg = cloudflare_api.inspect_token("t")
            self.assertFalse(ok)
            self.assertIn("not active", msg)
        for second in (broken, _resp(200, {"result": None}), _resp(200, {"result": {}})):
            self.safe_patch(API).side_effect = [_resp(200, {"result": {"id": "tok1", "status": "active"}}), second]
            ok, msg = cloudflare_api.inspect_token("t")
            self.assertFalse(ok)
            self.assertIn("not readable", msg)

    def _settings(self, **vals):
        website = self.env["website"].get_current_website()
        return self.env["res.config.settings"].create({"website_id": website.id, **vals})

    def test_settings_refuse_then_override(self):
        # Tests [@ANCHOR: cloudflare:COMM_settings_create]
        # Tests [@ANCHOR: cloudflare:COMM_settings_check_token_scope]
        pol = [{"effect": "allow", "permission_groups": [{"name": "Billing Write"}]}]
        m = self.safe_patch("odoo.addons.cloudflare.models.res_config_settings.inspect_token")
        m.return_value = (True, cloudflare_api.broad_token_permissions(pol))
        with self.assertRaisesRegex(UserError, "Billing Write"):
            self._settings(cloudflare_api_token="brand-new-token")
        self._settings(cloudflare_api_token="brand-new-token", cloudflare_allow_broad_token=True)  # accepted, logged

    def test_settings_unchanged_token_not_rechecked(self):
        website = self.env["website"].get_current_website()
        website.write({"cloudflare_api_token": "same"})
        m = self.safe_patch("odoo.addons.cloudflare.models.res_config_settings.inspect_token")
        self._settings(cloudflare_api_token="same")
        m.assert_not_called()

    def test_settings_mask_the_stored_secrets(self):
        # Tests [@ANCHOR: cloudflare:COMM_settings_get_values]
        # Tests [@ANCHOR: cloudflare:COMM_settings_inverse_secrets]
        # Tests [@ANCHOR: cloudflare:COMM_settings_compute_secrets]
        # Tests [@ANCHOR: cloudflare:COMM_settings_masks_secrets]
        website = self.env["website"].get_current_website()
        website.write({"cloudflare_api_token": "real-token", "cloudflare_turnstile_secret": "real-secret"})
        m = self.safe_patch("odoo.addons.cloudflare.models.res_config_settings.inspect_token")
        settings = self._settings()
        self.assertEqual(settings.read(["cloudflare_api_token"])[0]["cloudflare_api_token"], "********")
        self.assertEqual(settings.cloudflare_turnstile_secret, "********")
        self.assertEqual(self.env["res.config.settings"].get_values()["cloudflare_api_token"], "********")
        settings.cloudflare_api_token = "********"  # saved untouched: kept
        self.assertEqual(website.cloudflare_api_token, "real-token")
        self.assertEqual(website.cloudflare_turnstile_secret, "real-secret")
        m.assert_not_called()
        settings.cloudflare_turnstile_secret = "new-secret"  # edited: stored
        self.assertEqual(website.cloudflare_turnstile_secret, "new-secret")
        settings.cloudflare_turnstile_secret = ""  # blank: cleared
        self.assertFalse(website.cloudflare_turnstile_secret)

    def test_json_result_returns_the_result_only_when_it_is_the_kind_asked_for(self):
        # Tests [@ANCHOR: cloudflare:COMM_dns_api_json_result_kind]
        # Tests [@ANCHOR: cloudflare:COMM_dns_api_json_result_kind]
        self.assertEqual(cloudflare_api._json_result(_resp(200, {"result": {"a": 1}})), {"a": 1})
        self.assertIsNone(cloudflare_api._json_result(_resp(200, {"result": [1]})))
        self.assertEqual(cloudflare_api._json_result(_resp(200, {"result": [1]}), list), [1])
        self.assertIsNone(cloudflare_api._json_result(_resp(200, {"result": "x"}), list))
        self.assertIsNone(cloudflare_api._json_result(_resp(200, ["x"])))
