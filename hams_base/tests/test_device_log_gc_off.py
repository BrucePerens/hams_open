# -*- coding: utf-8 -*-
# Copyright © Bruce Perens K6BP. License: AGPL-3.0.
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.tests import tagged


@tagged("post_install", "-at_install")
class TestDeviceLogGcOff(HamsTransactionCase):
    # [@ANCHOR: hams_base:test_device_log_gc_off]
    def test_superseded_device_logs_survive_the_gc(self):
        # Tests [@ANCHOR: hams_base_res_device_log_res_device_log]
        # Tests [@ANCHOR: hams_base:device_log_gc_off]
        model = self.env["res.device.log"]
        same = {"session_identifier": "s" * 42, "platform": "linux", "browser": "firefox", "ip_address": "192.0.2.1"}
        older = model.create({**same, "last_activity": "2020-01-01 00:00:00"})
        newer = model.create({**same, "last_activity": "2020-01-02 00:00:00"})
        model._gc_device_log()
        self.assertEqual(len((older | newer).exists()), 2, "[!] DIAGNOSTIC FOR AI: device logs are logs and must never be purged.")
        self.assertNotIn("_autovacuum", type(model)._gc_device_log.__dict__, "the vacuum must not run it")
