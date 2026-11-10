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
        pol = [{"effect": "allow", "permission_groups": [{"name": "Zone DNS Write"}, {"name": "Cache Purge"}]}]
        self.safe_patch(API).side_effect = _answers(pol)
        self.assertEqual(cloudflare_api.inspect_token("t"), (True, []))

    def test_broad_token_reported(self):
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
        pol = [{"effect": "allow", "permission_groups": [{"name": "Billing Write"}]}]
        settings = self._settings(cloudflare_api_token="brand-new-token")
        m = self.safe_patch("odoo.addons.cloudflare.models.res_config_settings.inspect_token")
        m.return_value = (True, cloudflare_api.broad_token_permissions(pol))
        with self.assertRaisesRegex(UserError, "Billing Write"):
            settings._check_cloudflare_token_scope()
        settings.cloudflare_allow_broad_token = True
        settings._check_cloudflare_token_scope()  # accepted, logged

    def test_settings_unchanged_token_not_rechecked(self):
        website = self.env["website"].get_current_website()
        website.write({"cloudflare_api_token": "same"})
        settings = self._settings()
        m = self.safe_patch("odoo.addons.cloudflare.models.res_config_settings.inspect_token")
        settings._check_cloudflare_token_scope()
        m.assert_not_called()
