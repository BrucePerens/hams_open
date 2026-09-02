# SPDX-License-Identifier: AGPL-3.0-or-later
# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import itertools
import os
import redis
import logging
import datetime
from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.pager_duty.models.incident import TREND_WINDOW_MINUTES, TREND_OCCURRENCE_THRESHOLD
from unittest.mock import MagicMock
from odoo import fields, _

_logger = logging.getLogger(__name__)


@tagged("standard", "post_install", "-at_install")
class TestPagerIncidentStandard(HamsTransactionCase):
    def setUp(self):
        super(TestPagerIncidentStandard, self).setUp()
        self.incident_model = self.env["pager.incident"]
        self.service_user = self.env.ref("pager_duty.user_pager_service_internal")
        self.creator_user = self.env.ref("pager_duty.user_pager_incident_creator")

    def test_01_rate_limiting_blocks_spam_standard(self):
        # Tests [@ANCHOR: report_incident_rate_limit]

        # Tests [@ANCHOR: pd_redis_rate_limit]
        vals = {
            "source": "test_daemon",
            "severity": "high",
            "description": "Test breach",
        }

        mock_redis = self.safe_patch("odoo.addons.pager_duty.models.incident.redis")
        self.safe_patch(
            "odoo.addons.pager_duty.models.incident.redis_pool", MagicMock()
        )
        mock_client = MagicMock()
        mock_redis.Redis.return_value = mock_client
        mock_client.set.return_value = False

        result = self.incident_model.report_incident(vals)

        self.assertFalse(
            result, "Incident engine failed to block rate-limited request."
        )
        website = self.env["website"].get_current_website()
        website_suffix = str(website.id) if website else "global"
        mock_client.set.assert_called_with(
            f"pager_rate_limit:test_daemon:{website_suffix}", "1", ex=60, nx=True
        )

    def test_02_zero_sudo_impersonation_and_mail_standard(self):
        # Tests [@ANCHOR: auto_resolve_incidents]

        # Tests [@ANCHOR: test_pager_notification]
        vals = {
            "source": "test_daemon_2",
            "severity": "critical",
            "description": "Zero sudo test",
        }

        mock_redis = self.safe_patch("odoo.addons.pager_duty.models.incident.redis")
        self.safe_patch(
            "odoo.addons.pager_duty.models.incident.redis_pool", MagicMock()
        )
        mock_client = MagicMock()
        mock_redis.Redis.return_value = mock_client
        mock_client.get.return_value = None

        incident_id = self.incident_model.report_incident(vals)
        self.assertTrue(incident_id, "Incident failed to create.")

        incident = self.incident_model.browse(incident_id)
        self.assertEqual(
            incident.create_uid.id,
            self.creator_user.id,
            "Incident not under Zero-Sudo UID.",
        )

        incident.message_post(body=_("Test message"))
        self.incident_model.auto_resolve_incidents("test_daemon_2")
        self.assertEqual(incident.status, "resolved")

    def test_03_bus_notification_on_create_standard(self):
        mock_sendone = self.safe_patch_object(type(self.env["bus.bus"]), "_sendone")
        incident = self.incident_model.create(
            {"source": "manual", "severity": "low", "description": "Bus test"}
        )
        self.assertTrue(incident.id)

        self.assertTrue(
            mock_sendone.called,
            "Bus notification was not dispatched on incident creation.",
        )
        args, kwargs = mock_sendone.call_args
        str_args = [a for a in args if isinstance(a, str)]
        self.assertEqual(
            str_args[0],
            "pager_duty",
            "Bus notification sent to incorrect channel.",
        )
        self.assertEqual(
            str_args[1],
            "update_board",
            "Bus notification used incorrect message type.",
        )

    def test_05_mtta_mttr_calculation(self):
        # Prove MTTA/MTTR computation
        incident = self.incident_model.create(
            {"source": "analytics_test", "severity": "low", "description": "desc"}
        )
        self.assertFalse(incident.mtta)
        self.assertFalse(incident.mttr)

        incident.write({"status": "acknowledged"})
        self.assertTrue(incident.time_acknowledged)
        self.assertIsInstance(incident.mtta, float)

        incident.write({"status": "resolved"})
        self.assertTrue(incident.time_resolved)
        self.assertIsInstance(incident.mttr, float)

    def test_06_escalation(self):
        # Tests [@ANCHOR: test_pager_escalation]
        self.env.user.group_ids = [(4, self.env.ref("pager_duty.group_pager_admin").id)]
        incident = self.incident_model.create(
            {"source": "esc_test", "severity": "high", "description": "desc"}
        )

        self.env.cr.execute(
            "UPDATE pager_incident SET create_date = %s WHERE id = %s",
            (fields.Datetime.now() - datetime.timedelta(minutes=20), incident.id),
        )

        mock_msg = self.safe_patch_object(type(incident), "message_post")
        self.env.ref("pager_duty.cron_escalate_incidents")._trigger()
        self.incident_model.action_escalate_unacknowledged()
        mock_msg.assert_called()

        # Assert that the status flag was successfully changed to break the infinite loop
        incident.invalidate_recordset(["is_escalated"])
        self.assertTrue(incident.is_escalated)

    def test_04_views_render(self):
        # [@ANCHOR: test_pager_view]
        v1 = self.env["pager.incident"].get_view(view_type="form")
        v2 = self.env["pager.incident"].get_view(view_type="list")
        v3 = self.env["calendar.event"].get_view(view_type="form")
        self.assertIn("arch", v1)
        self.assertIn("arch", v2)
        self.assertIn("arch", v3)

    def test_07_board_data_procedure(self):
        # Tests [@ANCHOR: pager_duty_postgres_procedures]

        # Tests [@ANCHOR: test_pager_duty_procedures]
        self.incident_model.report_incident(
            {
                "source": "Dashboard Board Test",
                "severity": "medium",
                "description": "test",
            }
        )
        self.env.flush_all()
        data = self.incident_model.get_board_data()
        self.assertTrue(len(data["active"]) > 0)
        self.assertEqual(data["active"][0]["source"], "Dashboard Board Test")

    def test_08_auto_resolve_multi_tenant(self):
        """Tests that auto-resolve respects website isolation when context is provided."""
        website1 = self.env["website"].create({"name": "Site 1"})
        website2 = self.env["website"].create({"name": "Site 2"})
        
        inc1 = self.incident_model.create({"source": "test_src", "severity": "low", "description": "d", "website_id": website1.id})
        inc2 = self.incident_model.create({"source": "test_src", "severity": "low", "description": "d", "website_id": website2.id})
        
        self.incident_model.auto_resolve_incidents("test_src", website_id=website1.id)
        
        self.assertEqual(inc1.status, "resolved", "Incident 1 should be resolved")
        self.assertEqual(inc2.status, "open", "Incident 2 should remain open because it belongs to a different website")

    def test_09_escalation_security(self):
        """Tests that escalation uses proper service accounts."""
        incident = self.incident_model.create(
            {"source": "esc_sec_test", "severity": "high", "description": "desc"}
        )
        self.env.cr.execute(
            "UPDATE pager_incident SET create_date = %s WHERE id = %s",
            (fields.Datetime.now() - datetime.timedelta(minutes=20), incident.id),
        )
        
        self.env.ref("pager_duty.cron_escalate_incidents")._trigger()
        # This should execute successfully without throwing an AccessError due to missing pd_svc vs mail_svc permissions.
        self.incident_model.action_escalate_unacknowledged()
        
        incident.invalidate_recordset(["is_escalated"])
        self.assertTrue(incident.is_escalated)

    def test_10_occurrence_count_increments_on_dedup_not_a_new_incident(self):
        # Tests [@ANCHOR: pager_incident_occurrence_count]
        # Real coverage for ODOO_DB_LOG_CRASH_MONITORING.md's own gap #1:
        # a repeat report for the same still-open source must not create a
        # duplicate incident (already true, see report_incident_rate_limit
        # above) AND must now increment occurrence_count/update
        # last_occurred on the SAME existing record, rather than silently
        # doing nothing beyond returning its id.
        vals = {
            "source": "occurrence_test_daemon",
            "severity": "high",
            "description": "First occurrence",
        }

        mock_redis = self.safe_patch("odoo.addons.pager_duty.models.incident.redis")
        self.safe_patch(
            "odoo.addons.pager_duty.models.incident.redis_pool", MagicMock()
        )
        mock_client = MagicMock()
        mock_redis.Redis.return_value = mock_client
        # Every rate-limit check passes (True = "the SET succeeded, not
        # already rate-limited") so both calls below reach the real
        # dedup/search logic rather than being blocked at the Redis gate.
        mock_client.set.return_value = True

        # Odoo Datetime fields are second-resolution, so two back-to-back
        # real fields.Datetime.now() calls can genuinely tie -- an
        # assertGreater below on the real clock would be flaky, and the
        # weaker assertGreaterEqual this replaced would pass trivially
        # even if last_occurred never advanced at all. A monotonically
        # increasing fake clock (a fresh, strictly later value every call,
        # regardless of how many times report_incident() itself calls
        # fields.Datetime.now()) makes assertGreater both correct and
        # deterministic.
        fake_clock_base = fields.Datetime.now()
        fake_clock_calls = itertools.count()
        self.safe_patch(
            "odoo.addons.pager_duty.models.incident.fields.Datetime.now",
            side_effect=lambda: fake_clock_base + datetime.timedelta(seconds=next(fake_clock_calls)),
        )

        first_id = self.incident_model.report_incident(vals)
        self.assertTrue(first_id, "First report should create a new incident.")
        incident = self.incident_model.browse(first_id)
        self.assertEqual(incident.occurrence_count, 1, "A brand-new incident starts at 1 occurrence.")
        first_last_occurred = incident.last_occurred

        second_id = self.incident_model.report_incident(
            {**vals, "description": "Second occurrence, same source"}
        )
        self.assertEqual(second_id, first_id, "A repeat for the same open source must reuse the existing incident, not create a duplicate.")
        incident.invalidate_recordset(["occurrence_count", "last_occurred"])
        self.assertEqual(incident.occurrence_count, 2, "occurrence_count must increment on a dedup match.")
        self.assertGreater(incident.last_occurred, first_last_occurred, "last_occurred must actually advance on a repeat report, not just tie.")

    def test_11_low_severity_does_not_page_but_high_severity_still_does(self):
        # Tests [@ANCHOR: pager_trend_severity_gate]
        # Tests [@ANCHOR: pager_trend_detection_params]
        # Tests [@ANCHOR: pager_trend_detection]
        # PAGER_DUTY_MCP_AI_TRIAGE.md's own trend-detection design: low/
        # medium severity incidents get tracked but must NOT page on_duty
        # immediately (that's the whole point of deferring to a trend
        # check instead) -- while high/critical must keep today's
        # unchanged immediate-page behavior.
        mock_redis = self.safe_patch("odoo.addons.pager_duty.models.incident.redis")
        self.safe_patch(
            "odoo.addons.pager_duty.models.incident.redis_pool", MagicMock()
        )
        mock_client = MagicMock()
        mock_redis.Redis.return_value = mock_client
        mock_client.set.return_value = True

        mock_notify = self.safe_patch_object(type(self.incident_model), "_notify_on_duty")

        self.incident_model.report_incident(
            {"source": "low_sev_test", "severity": "low", "description": "sub-critical, tracked only"}
        )
        mock_notify.assert_not_called()

        self.incident_model.report_incident(
            {"source": "high_sev_test", "severity": "high", "description": "must still page"}
        )
        mock_notify.assert_called_once()

    def test_12_trend_window_resets_after_it_lapses(self):
        # Tests [@ANCHOR: pager_trend_window_update]
        # window_occurrence_count must reflect a real burst rate, not a
        # lifetime total -- a repeat occurrence arriving AFTER the
        # rolling window has lapsed must reset the window counter to 1,
        # even though occurrence_count (the lifetime total) keeps
        # incrementing regardless.
        mock_redis = self.safe_patch("odoo.addons.pager_duty.models.incident.redis")
        self.safe_patch(
            "odoo.addons.pager_duty.models.incident.redis_pool", MagicMock()
        )
        mock_client = MagicMock()
        mock_redis.Redis.return_value = mock_client
        mock_client.set.return_value = True
        self.safe_patch_object(type(self.incident_model), "_notify_on_duty")

        fake_now = [fields.Datetime.now()]
        self.safe_patch(
            "odoo.addons.pager_duty.models.incident.fields.Datetime.now",
            side_effect=lambda: fake_now[0],
        )

        vals = {"source": "window_reset_test", "severity": "low", "description": "first"}
        first_id = self.incident_model.report_incident(vals)
        incident = self.incident_model.browse(first_id)
        self.assertEqual(incident.window_occurrence_count, 1)

        # Well inside the window: a second occurrence increments the
        # window counter, not just the lifetime counter.
        fake_now[0] = fake_now[0] + datetime.timedelta(minutes=5)
        self.incident_model.report_incident({**vals, "description": "second, inside window"})
        incident.invalidate_recordset(["window_occurrence_count", "occurrence_count"])
        self.assertEqual(incident.window_occurrence_count, 2, "Still within the window: must accumulate.")
        self.assertEqual(incident.occurrence_count, 2)

        # Past TREND_WINDOW_MINUTES since window_start: the window must
        # reset, even though this is still the same still-open incident.
        fake_now[0] = fake_now[0] + datetime.timedelta(minutes=TREND_WINDOW_MINUTES + 1)
        self.incident_model.report_incident({**vals, "description": "third, window lapsed"})
        incident.invalidate_recordset(["window_occurrence_count", "occurrence_count", "window_start"])
        self.assertEqual(incident.window_occurrence_count, 1, "Window lapsed: must reset to 1, not keep accumulating.")
        self.assertEqual(incident.occurrence_count, 3, "Lifetime total must keep incrementing regardless of window resets.")

    def test_13_trend_threshold_raises_a_real_paging_incident_exactly_once(self):
        # Tests [@ANCHOR: pager_raise_trend_incident]
        # Crossing TREND_OCCURRENCE_THRESHOLD within the window for a
        # low/medium-severity source must raise a real, distinct, paging
        # "Trend:" incident -- and must not raise a second one on further
        # occurrences of the same burst (trend_raised must gate this).
        mock_redis = self.safe_patch("odoo.addons.pager_duty.models.incident.redis")
        self.safe_patch(
            "odoo.addons.pager_duty.models.incident.redis_pool", MagicMock()
        )
        mock_client = MagicMock()
        mock_redis.Redis.return_value = mock_client
        mock_client.set.return_value = True
        mock_notify = self.safe_patch_object(type(self.incident_model), "_notify_on_duty")

        fake_clock_base = fields.Datetime.now()
        fake_clock_calls = itertools.count()
        self.safe_patch(
            "odoo.addons.pager_duty.models.incident.fields.Datetime.now",
            side_effect=lambda: fake_clock_base + datetime.timedelta(seconds=next(fake_clock_calls)),
        )

        vals = {"source": "trend_burst_test", "severity": "medium", "description": "burst"}
        first_id = self.incident_model.report_incident(vals)
        for _i in range(TREND_OCCURRENCE_THRESHOLD - 1):
            self.incident_model.report_incident({**vals, "description": f"burst occurrence {_i}"})

        # _notify_on_duty must have fired exactly once -- for the trend
        # escalation raised by crossing the threshold above, never for the
        # low/medium source's own individual occurrences.
        mock_notify.assert_called_once()

        trend_incident = self.incident_model.search([("source", "=", f"Trend: {vals['source']}")])
        self.assertTrue(trend_incident, "Crossing the trend threshold must raise a real, separate incident.")
        self.assertEqual(trend_incident.severity, "high", "A trend escalation must always page, regardless of the underlying pattern's own severity.")
        mock_notify.assert_called_once()

        source_incident = self.incident_model.browse(first_id)
        source_incident.invalidate_recordset(["trend_raised"])
        self.assertTrue(source_incident.trend_raised)

        # One more occurrence of the same burst must NOT raise a second
        # trend incident.
        self.incident_model.report_incident({**vals, "description": "one more, after trend already raised"})
        mock_notify.assert_called_once()
        all_trend_incidents = self.incident_model.search([("source", "=", f"Trend: {vals['source']}")])
        self.assertEqual(len(all_trend_incidents), 1, "trend_raised must prevent a duplicate trend incident for the same burst.")


@tagged("integration", "post_install", "-at_install")
class TestPagerIncidentIntegration(HamsTransactionCase):
    _daemons_started = False

    def setUp(self):
        super(TestPagerIncidentIntegration, self).setUp()
        self.incident_model = self.env["pager.incident"]
        self.service_user = self.env.ref("pager_duty.user_pager_service_internal")
        self.creator_user = self.env.ref("pager_duty.user_pager_incident_creator")

        if not self.__class__._daemons_started:
            base_dir = os.path.join(os.path.dirname(__file__), "..", "daemon")
            daemons = [
                "pager_smart_spooler.py",
                "pager_log_analyzer.py",
                "pager_synthetic_spooler.py",
            ]
            for d in daemons:
                daemon_path = os.path.abspath(os.path.join(base_dir, d))
                if os.path.exists(daemon_path):
                    self.start_daemon(daemon_path)
            self.__class__._daemons_started = True

    def test_01_rate_limiting_blocks_spam_integration(self):
        vals = {
            "source": "test_daemon",
            "severity": "high",
            "description": "Test breach",
        }

        r = redis.Redis(
            host=os.getenv("REDIS_HOST") or "redis",
            port=int(os.getenv("REDIS_PORT") or "6379"),
            db=0,
        )
        r.delete("pager_rate_limit:test_daemon")

        # First request passes the cache check
        res1 = self.incident_model.report_incident(vals)
        self.assertTrue(res1, "First request should pass in integration mode.")

        # Second request is blocked by the TTL key in the real Redis instance
        res2 = self.incident_model.report_incident(vals)
        self.assertFalse(res2, "Second request should be blocked by real Redis.")

    def test_02_zero_sudo_impersonation_and_mail_integration(self):
        vals = {
            "source": "test_daemon_2",
            "severity": "critical",
            "description": "Zero sudo test",
        }

        r = redis.Redis(
            host=os.getenv("REDIS_HOST") or "redis",
            port=int(os.getenv("REDIS_PORT") or "6379"),
            db=0,
        )
        r.delete("pager_rate_limit:test_daemon_2")

        incident_id = self.incident_model.report_incident(vals)
        self.assertTrue(incident_id, "Incident failed to create in integration mode.")

        incident = self.incident_model.browse(incident_id)
        self.assertEqual(
            incident.create_uid.id,
            self.creator_user.id,
            "Incident not under Zero-Sudo UID.",
        )

        incident.message_post(body=_("Test message"))
        self.incident_model.auto_resolve_incidents("test_daemon_2")
        self.assertEqual(incident.status, "resolved")

    def test_03_bus_notification_on_create_integration(self):
        incident = self.incident_model.create(
            {"source": "manual", "severity": "low", "description": "Bus test"}
        )
        self.assertTrue(incident.id)
