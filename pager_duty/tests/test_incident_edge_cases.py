# -*- coding: utf-8 -*-
from odoo.tests.common import tagged
from odoo.addons.hams_test.common import HamsTransactionCase
from unittest.mock import MagicMock


@tagged("post_install", "-at_install")
class TestIncidentEdgeCases(HamsTransactionCase):
    def setUp(self):
        super().setUp()
        self.incident_model = self.env["pager.incident"]

    def test_01_redis_fail_open(self):
        mock_redis = self.safe_patch("odoo.addons.pager_duty.models.incident.redis")
        self.safe_patch("odoo.addons.pager_duty.models.incident.redis_pool", MagicMock())
        """Verify that if Redis crashes during report_incident, it safely fails open and logs the incident."""
        mock_redis.Redis.side_effect = Exception("Redis connection timeout")
        mock_redis.exceptions.RedisError = Exception

        self.safe_patch_object(type(self.env["bus.bus"]), "_sendone", create=True)
        incident_id = self.incident_model.report_incident(
                {
                    "source": "fail_open_test",
                    "severity": "high",
                    "description": "Redis failure simulation",
                }
            )

        self.assertTrue(
            incident_id, "The incident MUST be created even if Redis is unreachable."
        )

    def test_02_default_name_assignment(self):
        """Verify that an omitted or 'New' name is automatically assigned 'INC-AUTO'."""
        self.safe_patch_object(type(self.env["bus.bus"]), "_sendone", create=True)
        incident_id = self.incident_model.report_incident(
                {
                    "source": "naming_test",
                    "severity": "low",
                    "description": "Naming simulation",
                }
            )

        incident = self.incident_model.browse(incident_id)
        self.assertEqual(
            incident.name, "INC-AUTO", "Default names MUST be translated to INC-AUTO."
        )

    def test_03_auto_resolve_empty(self):
        """Verify that calling auto_resolve_incidents on a source with no open incidents returns True safely."""
        res = self.incident_model.auto_resolve_incidents("non_existent_source")
        self.assertTrue(
            res, "Auto resolve MUST return True gracefully if no incidents exist."
        )
