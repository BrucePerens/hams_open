# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
# -*- coding: utf-8 -*-
from odoo.tests.common import tagged, BaseCase
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.distributed_redis_cache.redis_cache import invalidate_model_cache
from odoo.addons.cloudflare.utils import zone_settings_plan

MGR = "odoo.addons.cloudflare.models.config_manager."
ALL_OFF = {"ssl": "full", "minify": {"css": "off", "html": "off", "js": "off"}, "bot_fight_mode": False}


@tagged("post_install", "-at_install")
class TestZoneSettingsPlan(BaseCase):
    """The pure comparison: what would change, nothing else."""

    def test_row_to_wanted_settings(self):
        # Tests [@ANCHOR: cloudflare:COMM_zone_settings_plan_desired]
        d = zone_settings_plan.desired_from_row("strict", True, True)
        self.assertEqual(d, {"ssl": "strict", "minify": {"css": "on", "html": "on", "js": "on"}, "bot_fight_mode": True})
        self.assertNotIn("ssl", zone_settings_plan.desired_from_row(False, False, False))

    def test_only_differences_are_changes_and_hash_follows_the_zone(self):
        # Tests [@ANCHOR: cloudflare:COMM_zone_settings_plan_make]
        # Tests [@ANCHOR: cloudflare:COMM_zone_settings_plan_show]
        want = zone_settings_plan.desired_from_row("strict", False, True)
        plan = zone_settings_plan.make(ALL_OFF, want)
        self.assertEqual(plan["changes"], {"ssl": "strict", "bot_fight_mode": True})
        same = zone_settings_plan.make(want, want)
        self.assertEqual(same["changes"], {})
        other = zone_settings_plan.make(dict(ALL_OFF, ssl="flexible"), want)
        self.assertNotEqual(plan["hash"], other["hash"])

    def test_render_lists_entries_and_summary(self):
        # Tests [@ANCHOR: cloudflare:COMM_zone_settings_plan_render]
        plan = zone_settings_plan.make(ALL_OFF, zone_settings_plan.desired_from_row("strict", False, False))
        lines = zone_settings_plan.render(plan).splitlines()
        self.assertTrue(any(ln.startswith("UPDATE") and "ssl" in ln and "was full; now strict" in ln for ln in lines))
        self.assertEqual(lines[-1], "Summary: 1 zone setting(s) would change; nothing else is touched.")


@tagged("post_install", "-at_install")
class TestZoneSettingsPush(HamsTransactionCase):
    """Odoo's cloudflare.zone.settings row reaches the zone (mocked API, never the real one)."""

    def setUp(self):
        super().setUp()
        self.svc_uid = self.env["zero_sudo.security.utils"]._get_service_uid("cloudflare.user_cloudflare_waf")
        self.website = self.env["website"].get_current_website()
        self.website.write({"cloudflare_api_token": "fake_token", "cloudflare_zone_id": "fake_zone"})
        invalidate_model_cache(self.env, "website")
        self.env["cloudflare.zone.settings"].search([]).unlink()
        self.get_setting = self.safe_patch(MGR + "get_zone_setting_checked")
        self.get_bot = self.safe_patch(MGR + "get_bot_fight_mode_checked")
        self.set_setting = self.safe_patch(MGR + "update_zone_setting")
        self.set_bot = self.safe_patch(MGR + "update_bot_fight_mode")
        self.zone = {"ssl": "full", "minify": {"css": "off", "html": "off", "js": "off"}}
        self.get_setting.side_effect = lambda key, token, zone: ("ok", self.zone[key])
        self.get_bot.return_value = ("ok", False)
        self.set_setting.return_value = (True, "ok")
        self.set_bot.return_value = (True, "ok")
        self.row = self.env["cloudflare.zone.settings"].create(
            {"name": "Main", "ssl_mode": "strict", "auto_minify": False, "bot_fight_mode": True, "website_id": self.website.id}
        )

    def _mgr(self):
        return self.env["cloudflare.config.manager"].with_user(self.svc_uid)

    def _push(self, **kw):
        return self._mgr().action_push_zone_settings(website_id=self.website.id, **kw)

    def _apply_writes_to_zone(self):

        def set_setting(key, value, token, zone):
            self.zone[key] = value
            return True, "ok"
        self.set_setting.side_effect = set_setting

        def set_bot(value, token, zone):
            self.get_bot.return_value = ("ok", value)
            return True, "ok"
        self.set_bot.side_effect = set_bot

    def test_push_writes_only_what_differs(self):
        # Tests [@ANCHOR: cloudflare:COMM_zone_settings_push]
        self._apply_writes_to_zone()
        ok, msg = self._push()
        self.assertTrue(ok, msg)
        self.set_setting.assert_called_once_with("ssl", "strict", "fake_token", "fake_zone")
        self.set_bot.assert_called_once_with(True, "fake_token", "fake_zone")
        self.assertIn("OK ssl", msg)
        self.assertNotIn("fake_token", msg)

    def test_second_push_has_nothing_to_do(self):
        # Tests [@ANCHOR: cloudflare:COMM_zone_settings_push]
        self._apply_writes_to_zone()
        self._push()
        self.set_setting.reset_mock()
        self.set_bot.reset_mock()
        ok, msg = self._push()
        self.assertTrue(ok)
        self.assertIn("Nothing to do", msg)
        self.set_setting.assert_not_called()
        self.set_bot.assert_not_called()

    def test_unreadable_setting_is_refused_not_treated_as_unset(self):
        # Tests [@ANCHOR: cloudflare:COMM_zone_settings_plan]
        self.get_setting.side_effect = lambda key, token, zone: ("error", "HTTP 403")
        ok, msg = self._push()
        self.assertFalse(ok)
        self.assertIn("REFUSED", msg)
        self.set_setting.assert_not_called()
        self.set_bot.assert_not_called()
        self.get_setting.side_effect = lambda key, token, zone: ("ok", self.zone[key])
        self.get_bot.return_value = ("error", "API Error")
        ok, msg = self._push()
        self.assertFalse(ok)
        self.assertIn("bot_fight_mode", msg)
        self.set_setting.assert_not_called()

    def test_plan_writes_nothing_and_stale_hash_is_refused(self):
        # Tests [@ANCHOR: cloudflare:COMM_zone_settings_plan]
        # Tests [@ANCHOR: cloudflare:COMM_zone_settings_push]
        plan = self._mgr().plan_zone_settings(self.website.id)
        self.assertTrue(plan["ok"])
        self.assertEqual(plan["writes"], 2)
        self.set_setting.assert_not_called()
        self.set_bot.assert_not_called()
        ok, msg = self._push(plan_hash="0" * 64)
        self.assertFalse(ok)
        self.set_setting.assert_not_called()
        self._apply_writes_to_zone()
        self.assertTrue(self._push(plan_hash=plan["hash"])[0])
        self.set_setting.assert_called_once()

    def test_each_failed_write_is_reported_and_the_rest_still_tried(self):
        # Tests [@ANCHOR: cloudflare:COMM_zone_settings_push]
        self.set_setting.return_value = (False, "API Error")
        ok, msg = self._push()
        self.assertFalse(ok)
        self.assertIn("FAILED ssl: API Error", msg)
        self.assertIn("OK bot_fight_mode", msg)

    def test_missing_credentials_and_row_count_are_refused(self):
        # Tests [@ANCHOR: cloudflare:COMM_zone_settings_plan]
        self.env["cloudflare.zone.settings"].create({"name": "Second", "website_id": self.website.id})
        ok, msg = self._push()
        self.assertFalse(ok)
        self.assertIn("exactly one", msg)
        self.row.unlink()
        self.env["cloudflare.zone.settings"].search([]).unlink()
        ok, msg = self._push()
        self.assertFalse(ok)
        self.website.write({"cloudflare_api_token": False})
        invalidate_model_cache(self.env, "website")
        ok, msg = self._push()
        self.assertFalse(ok)
        self.assertIn("Missing API credentials", msg)
        self.set_setting.assert_not_called()

    def test_form_buttons_plan_then_push_via_service_user(self):
        # Tests [@ANCHOR: cloudflare:COMM_zone_settings_row_plan_push]
        # Tests [@ANCHOR: cloudflare:COMM_zone_settings_row_push_to_cloudflare]
        # Tests [@ANCHOR: cloudflare:COMM_features_notification]
        res = self.row.action_plan_zone_settings_push()
        self.assertIn("UPDATE", res["params"]["message"])
        self.set_setting.assert_not_called()
        self.get_bot.return_value = ("error", "API Error")
        self.assertIn("REFUSED", self.row.action_plan_zone_settings_push()["params"]["message"])
        self.get_bot.return_value = ("ok", False)
        res = self.row.action_push_zone_settings_to_cloudflare()
        self.set_setting.assert_called_once()
        self.assertEqual(res["tag"], "display_notification")

    def test_api_helpers_tell_unreadable_from_a_value(self):
        # Tests [@ANCHOR: cloudflare:COMM_cf_api_get_zone_setting_checked]
        # Tests [@ANCHOR: cloudflare:COMM_cf_api_get_bot_fight_mode_checked]
        # Tests [@ANCHOR: cloudflare:COMM_cf_api_update_bot_fight_mode]
        from odoo.addons.cloudflare.utils import cloudflare_api as api

        class Resp:
            def __init__(self, status, body):
                self.status_code, self._body = status, body

            def json(self):
                return self._body

        req = self.safe_patch("odoo.addons.cloudflare.utils.cloudflare_api._make_request")
        req.return_value = Resp(200, {"result": {"id": "ssl", "value": "strict"}})
        self.assertEqual(api.get_zone_setting_checked("ssl", "t", "z"), ("ok", "strict"))
        req.return_value = Resp(403, {})
        self.assertEqual(api.get_zone_setting_checked("ssl", "t", "z"), ("error", "HTTP 403"))
        req.return_value = None
        self.assertEqual(api.get_zone_setting_checked("ssl", "t", "z")[0], "error")
        self.assertEqual(api.get_zone_setting_checked("ssl", "", "z")[0], "error")
        req.return_value = Resp(200, {"result": {"fight_mode": True}})
        self.assertEqual(api.get_bot_fight_mode_checked("t", "z"), ("ok", True))
        req.return_value = Resp(200, {"result": {}})
        self.assertEqual(api.get_bot_fight_mode_checked("t", "z")[0], "error")
        req.return_value = Resp(200, {})
        self.assertTrue(api.update_bot_fight_mode(True, "t", "z")[0])
        req.return_value = None
        self.assertFalse(api.update_bot_fight_mode(True, "t", "z")[0])
        self.assertEqual(req.call_args[1]["json"], {"fight_mode": True})
