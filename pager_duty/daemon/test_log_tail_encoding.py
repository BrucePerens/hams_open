# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later
"""A log line that is not valid UTF-8 must not kill a log check's tail thread. Plain unittest, no Odoo:
    python3 pager_duty/daemon/test_log_tail_encoding.py
"""
import os
import sys
import tempfile
import threading
import time
import unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import generalized_monitor as gm  # noqa: E402


class LogTailEncoding(unittest.TestCase):
    def test_a_non_utf8_line_does_not_kill_the_tail_thread(self):
        # Tests [@ANCHOR: pager_duty:log_tail_thread]
        handle, path = tempfile.mkstemp(suffix=".log")
        os.close(handle)
        self.addCleanup(os.remove, path)
        check = {"name": "Encoding test log", "target": path, "regex": "boom", "grace": 0}
        with mock.patch.object(gm, "report") as report, \
                mock.patch.object(gm.pagerduty_maintenance, "read_flag", return_value={"state": "off"}):
            thread = threading.Thread(target=gm.log_tail_thread, args=("client", check), daemon=True)
            thread.start()
            # The tailer starts at the end of the file, whenever the thread gets to run: keep appending the pair of lines
            # until it has seen one. The deadline is only a safety net.
            deadline = time.time() + 120
            while not report.called and time.time() < deadline:
                with open(path, "ab") as log:
                    log.write(b"garbage \xff\xfe bytes\nboom happened\n")
                time.sleep(0.05)
            self.assertTrue(report.called, "the tail thread died on the non-UTF-8 line and never saw the next one")
            self.assertTrue(thread.is_alive())

    def test_a_file_truncated_in_place_is_read_again_from_the_start(self):
        # Tests [@ANCHOR: pager_duty:log_tail_thread]
        handle, path = tempfile.mkstemp(suffix=".log")
        os.close(handle)
        self.addCleanup(os.remove, path)
        check = {"name": "Truncate test log", "target": path, "regex": "boom", "grace": 0}
        with mock.patch.object(gm, "report") as report, \
                mock.patch.object(gm.pagerduty_maintenance, "read_flag", return_value={"state": "off"}):
            thread = threading.Thread(target=gm.log_tail_thread, args=("client", check), daemon=True)
            thread.start()
            deadline = time.time() + 120
            # First get past the start-at-end seek: keep appending until the tailer has read a line.
            while not report.called and time.time() < deadline:
                with open(path, "a") as log:
                    log.write("boom one\n")
                time.sleep(0.05)
            self.assertTrue(report.called, "the tailer never read the file")
            # Truncate in place (same inode) and write a shorter, distinct line than the tailer's read position.
            report.reset_mock()
            while not report.called and time.time() < deadline:
                with open(path, "w") as log:
                    log.write("boom\n")
                time.sleep(0.05)
            self.assertTrue(report.called, "alerts stopped after the log was truncated in place")


if __name__ == "__main__":
    unittest.main()
