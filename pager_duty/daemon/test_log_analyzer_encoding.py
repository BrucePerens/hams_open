# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""A log line that is not valid UTF-8 must not kill the analyzer's tailer thread. Plain unittest, no Odoo:
    python3 pager_duty/daemon/test_log_analyzer_encoding.py
"""
import os
import re
import sys
import tempfile
import threading
import time
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import pager_log_analyzer  # noqa: E402


class LogAnalyzerEncoding(unittest.TestCase):
    def setUp(self):
        handle, self.path = tempfile.mkstemp(suffix=".log")
        os.close(handle)
        self.addCleanup(os.remove, self.path)

    def test_a_non_utf8_line_does_not_kill_the_tailer(self):
        # Tests [@ANCHOR: pager_duty:tail_file]
        r_client = mock.MagicMock()
        compiled = [{"name": "OOM", "severity": "critical", "website_id": None, "c_reg": re.compile("oom", re.I)}]
        path = self.path
        thread = threading.Thread(target=pager_log_analyzer.tail_file, args=(r_client, path, compiled), daemon=True)
        thread.start()
        # The tailer starts at the end of the file, whenever the thread gets to run: keep appending the pair of lines until it
        # has seen one. The deadline is only a safety net.
        deadline = time.time() + 120
        while not r_client.lpush.called and time.time() < deadline:
            with open(path, "ab") as log:
                log.write(b"kernel: garbage \xff\xfe bytes\nOOM killer ran\n")
            time.sleep(0.05)
        self.assertTrue(r_client.lpush.called, "the tailer died on the non-UTF-8 line and never saw the next one")
        self.assertTrue(thread.is_alive())

    def test_a_search_over_a_file_with_non_utf8_bytes_answers(self):
        # Tests [@ANCHOR: pager_duty:redis_search_listener]
        with open(self.path, "wb") as log:
            log.write(b"bad \xff line\nneedle here\n")
        pubsub = mock.MagicMock()
        pubsub.listen.return_value = [{"type": "message", "data": '{"uuid": "u1", "file": "%s", "regex": "needle"}' % self.path}]
        r_client = mock.MagicMock()
        r_client.pubsub.return_value = pubsub
        pager_log_analyzer.redis_search_listener(r_client)
        self.assertEqual(r_client.lpush.call_count, 1)
        self.assertIn("needle here", r_client.lpush.call_args.args[1])


if __name__ == "__main__":
    unittest.main()
