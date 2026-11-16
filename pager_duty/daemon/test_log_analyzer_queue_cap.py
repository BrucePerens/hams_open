# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""The Redis list the log analyzer feeds is capped. Plain unittest, no Odoo:
    python3 pager_duty/daemon/test_log_analyzer_queue_cap.py
"""
import os
import sys
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import pager_log_analyzer  # noqa: E402


class AnomalyQueueCap(unittest.TestCase):
    def test_push_anomaly_trims_the_list_to_the_cap(self):
        # Tests [@ANCHOR: pager_duty:log_analyzer_push_anomaly]
        r_client = mock.MagicMock()
        pager_log_analyzer.push_anomaly(r_client, {"source": "x"})
        r_client.lpush.assert_called_once()
        r_client.ltrim.assert_called_once_with("pager_log_anomalies", 0, pager_log_analyzer.MAX_QUEUED_ANOMALIES - 1)


if __name__ == "__main__":
    unittest.main()
