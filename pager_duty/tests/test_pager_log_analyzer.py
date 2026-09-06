# SPDX-License-Identifier: AGPL-3.0-or-later
# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
"""
pager_log_analyzer.py had zero test coverage anywhere -- confirmed via
grep. Real, previously-undiscovered hazard found while closing this gap:
config loading, the real Redis connection, and the real chroot/privilege-
drop sequence used to run unconditionally at *module import time*, so
merely `import`ing this file for any reason (a linter, a REPL, a future
test) connected to a live Redis server and, if run as root, chrooted and
de-privileged the calling process -- none of it reversible within the
same process. Moved into main() (guarded by __name__ == "__main__",
matching generalized_monitor.py's own convention), and tail_file()/
redis_search_listener() now take r_client as an explicit parameter
instead of a module-global connection -- what actually makes them
testable against a real double here.
"""
import json
import os
import re
import tempfile
import threading
import time
from unittest.mock import MagicMock

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.pager_duty.daemon import pager_log_analyzer


@tagged("post_install", "-at_install")
class TestPagerLogAnalyzer(HamsTransactionCase):
    def test_01_translate_path_strips_the_chroot_prefix(self):
        # Tests [@ANCHOR: pager_duty:translate_path]
        self.assertEqual(pager_log_analyzer.translate_path("/var/log/syslog"), "/syslog")
        self.assertEqual(pager_log_analyzer.translate_path("/other/path"), "/other/path")

    def test_02_tail_file_reports_a_matching_line_and_ignores_others(self):
        # Tests [@ANCHOR: pager_duty:tail_file]
        with tempfile.NamedTemporaryFile(
            mode="w", suffix=".log", delete=False
        ) as tmp:
            tmp_path = tmp.name
        self.addCleanup(lambda: os.path.exists(tmp_path) and os.remove(tmp_path))

        mock_r_client = MagicMock()
        compiled = [
            {
                "name": "OOM",
                "severity": "critical",
                "website_id": None,
                "c_reg": re.compile("(?i)out of memory"),
            }
        ]

        stop_event = threading.Event()

        def _stop_after_first_push(*args, **kwargs):
            stop_event.set()

        mock_r_client.lpush.side_effect = _stop_after_first_push

        # tail_file() is a genuine infinite loop (a real daemon thread) --
        # run it in a background thread against a real temp file, write a
        # matching line, and join with a timeout rather than trying to
        # bound the loop from the inside. daemon=True below is the actual
        # bound: the test process reaps it on exit rather than hanging.
        thread = threading.Thread(  # burn-ignore-test-daemon-thread: daemon=True below is the real bound, see comment above
            target=pager_log_analyzer.tail_file,
            args=(mock_r_client, tmp_path, compiled),
            daemon=True,
        )
        thread.start()
        try:
            deadline = time.time() + 5
            while time.time() < deadline and not thread.is_alive():
                time.sleep(0.05)
            with open(tmp_path, "a") as f:
                f.write("System ran Out Of Memory during allocation\n")
                f.flush()
            self.assertTrue(
                stop_event.wait(timeout=5),
                "tail_file() must detect a real appended line matching the pattern.",
            )
        finally:
            # Daemon thread; the process/test runner reaps it. No clean
            # shutdown hook exists in tail_file() itself (a real, infinite
            # daemon loop by design), so nothing further to join here.
            pass

        mock_r_client.lpush.assert_called()
        args = mock_r_client.lpush.call_args[0]
        self.assertEqual(args[0], "pager_log_anomalies")
        payload = json.loads(args[1])
        self.assertEqual(payload["source"], "Log Analyzer: OOM")
        self.assertIn("Out Of Memory", payload["description"])

    def test_03_redis_search_listener_answers_a_real_query_against_a_real_file(self):
        # Tests [@ANCHOR: pager_duty:redis_search_listener]
        with tempfile.NamedTemporaryFile(
            mode="w", suffix=".log", delete=False
        ) as tmp:
            tmp.write("line one\nERROR something broke\nline three\n")
            tmp_path = tmp.name
        self.addCleanup(lambda: os.path.exists(tmp_path) and os.remove(tmp_path))

        mock_r_client = MagicMock()
        req = {"uuid": "search-1", "file": tmp_path, "regex": "ERROR"}
        mock_pubsub = MagicMock()
        mock_pubsub.listen.return_value = [
            {"type": "message", "data": json.dumps(req)},
            {"type": "subscribe", "data": None},  # non-"message" events must be ignored
        ]
        mock_r_client.pubsub.return_value = mock_pubsub

        pager_log_analyzer.redis_search_listener(mock_r_client)

        mock_r_client.lpush.assert_called_once()
        args = mock_r_client.lpush.call_args[0]
        self.assertEqual(args[0], "pager_log_search_res_queue")
        result = json.loads(args[1])
        self.assertEqual(result["uuid"], "search-1")
        self.assertEqual(result["payload"]["matches"], ["ERROR something broke"])

    def test_04_main_exits_cleanly_when_no_files_are_configured(self):
        # Tests [@ANCHOR: pager_duty:log_analyzer_main]
        with tempfile.TemporaryDirectory() as tmpdir:
            config_path = os.path.join(tmpdir, "pager_config.json")
            with open(config_path, "w", encoding="utf-8") as f:
                json.dump({"log_analyzer": {"files": [], "patterns": []}}, f)

            # main() computes its config path from this module's own
            # __file__ -- patch that one module-scoped attribute (not
            # os.path.dirname globally, which every other piece of code
            # running in this process also calls) to redirect it at a
            # throwaway temp config instead of the real, already-populated
            # daemon/pager_config.json on this dev box.
            self.safe_patch(
                "odoo.addons.pager_duty.daemon.pager_log_analyzer.__file__",
                os.path.join(tmpdir, "pager_log_analyzer.py"),
            )
            mock_redis_cls = self.safe_patch(
                "odoo.addons.pager_duty.daemon.pager_log_analyzer.redis.Redis"
            )
            mock_redis_cls.return_value.ping.return_value = True

            with self.assertRaises(SystemExit) as ctx:
                pager_log_analyzer.main()
            self.assertEqual(ctx.exception.code, 0)
