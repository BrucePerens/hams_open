# SPDX-License-Identifier: AGPL-3.0-or-later

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
import concurrent.futures.thread
import json
import os
import re
import tempfile
import threading
import time
from unittest.mock import MagicMock, patch

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.pager_duty.daemon import generalized_monitor, pager_log_analyzer


@tagged("post_install", "-at_install")
class TestPagerLogAnalyzer(HamsTransactionCase):
    def test_01_translate_path_strips_the_chroot_prefix(self):
        # Tests [@ANCHOR: pager_duty:translate_path]
        self.assertEqual(pager_log_analyzer.translate_path("/var/log/syslog"), "/syslog")
        self.assertEqual(pager_log_analyzer.translate_path("/other/path"), "/other/path")

    def test_01b_translate_path_only_strips_a_leading_prefix_not_every_occurrence(self):
        # A plain fp.replace("/var/log", "")
        # would strip the substring anywhere in fp, so a path with "/var/log"
        # somewhere other than a leading prefix silently produced a wrong
        # chroot-relative path instead of being left alone.
        self.assertEqual(
            pager_log_analyzer.translate_path("/srv/var/log/x"), "/srv/var/log/x"
        )

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
            # thread.is_alive() is true long before tail_file() has opened the file and
            # seeked to its end, which is where a real tail starts reading. A line appended
            # before that seek lands behind the read position and is never seen, so a single
            # append made this test fail intermittently. Keep appending until the line is
            # detected: every append after the seek is a genuine "line written after the
            # tail started", which is exactly the behaviour under test.
            deadline = time.time() + 10
            while time.time() < deadline and not stop_event.is_set():
                with open(tmp_path, "a") as f:
                    f.write("System ran Out Of Memory during allocation\n")
                    f.flush()
                stop_event.wait(timeout=0.2)
            self.assertTrue(
                stop_event.is_set(),
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

    def test_02b_config_path_follows_the_same_rule_as_the_monitor(self):
        # Tests [@ANCHOR: pager_duty:log_analyzer_resolve_config_path]
        with tempfile.TemporaryDirectory() as d:
            explicit = os.path.join(d, "explicit.json")
            open(os.path.join(d, "pager_config.json"), "w").close()
            for env in ({"PAGER_CONFIG_PATH": explicit}, {"PAGER_CONFIG_DIR": d},
                        {"PAGER_CONFIG_DIR": os.path.join(d, "none")}):
                self.assertEqual(
                    pager_log_analyzer.resolve_config_path(env),
                    generalized_monitor.resolve_config_path(env), env)
            self.assertEqual(pager_log_analyzer.resolve_config_path({"PAGER_CONFIG_PATH": explicit}), explicit)
            self.assertEqual(pager_log_analyzer.resolve_config_path({"PAGER_CONFIG_DIR": d}),
                             os.path.join(d, "pager_config.json"))
            self.assertEqual(
                pager_log_analyzer.resolve_config_path({"PAGER_CONFIG_DIR": os.path.join(d, "none")}),
                os.path.join(os.path.dirname(pager_log_analyzer.__file__), "pager_config.json"))

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

            # main() resolves its config file with PAGER_CONFIG_PATH first (an explicit file), so point it at a
            # throwaway config instead of the real, already-populated one on this dev box (/opt/hams/etc).
            env_patch = patch.dict(os.environ, {"PAGER_CONFIG_PATH": config_path})
            env_patch.start()
            self.addCleanup(env_patch.stop)
            mock_redis_cls = self.safe_patch(
                "odoo.addons.pager_duty.daemon.pager_log_analyzer.redis.Redis"
            )
            mock_redis_cls.return_value.ping.return_value = True

            with self.assertRaises(SystemExit) as ctx:
                pager_log_analyzer.main()
            self.assertEqual(ctx.exception.code, 0)

    def test_05_resolve_redis_host_returns_an_address_and_leaves_an_unresolvable_name_alone(self):
        # Tests [@ANCHOR: pager_duty:log_analyzer_resolve_redis_host]
        self.assertEqual(pager_log_analyzer.resolve_redis_host("192.0.2.7"), "192.0.2.7", "an address stays as it is")
        def fake_getaddrinfo(host, port, family=0, type=0):
            if host != "redis":
                raise pager_log_analyzer.socket.gaierror("unknown host")
            return [(family, type, 0, "", ("2001:db8::1" if family != pager_log_analyzer.socket.AF_INET else "192.0.2.9", 0))]

        self.safe_patch_object(pager_log_analyzer.socket, "getaddrinfo", side_effect=fake_getaddrinfo)
        self.assertEqual(pager_log_analyzer.resolve_redis_host("redis"), "192.0.2.9", "IPv4 is preferred")
        self.assertEqual(
            pager_log_analyzer.resolve_redis_host("no-such-host.invalid"), "no-such-host.invalid",
            "a name that does not resolve is returned as given so the connection attempt reports the real error",
        )

    def test_06_push_anomaly_waits_out_a_redis_outage_instead_of_losing_the_match(self):
        # Tests [@ANCHOR: pager_duty:log_analyzer_push_anomaly]
        mock_r_client = MagicMock()
        mock_r_client.lpush.side_effect = [pager_log_analyzer.redis.ConnectionError("down"), pager_log_analyzer.redis.ConnectionError("down"), 1]
        payload = {"source": "Log Analyzer: PostgreSQL PANIC", "severity": "critical", "description": "PANIC: x", "website_id": None}
        pager_log_analyzer.push_anomaly(mock_r_client, payload, retry_seconds=0)
        self.assertEqual(mock_r_client.lpush.call_count, 3)
        queue, body = mock_r_client.lpush.call_args[0]
        self.assertEqual(queue, "pager_log_anomalies")
        self.assertEqual(json.loads(body), payload)

    def test_07_tail_file_reads_a_log_again_after_copytruncate_rotation(self):
        # Tests [@ANCHOR: pager_duty:tail_file]
        with tempfile.NamedTemporaryFile(mode="w", suffix=".log", delete=False) as tmp:
            tmp_path = tmp.name
        self.addCleanup(lambda: os.path.exists(tmp_path) and os.remove(tmp_path))
        seen = []
        mock_r_client = MagicMock()
        mock_r_client.lpush.side_effect = lambda queue, body: seen.append(json.loads(body)["description"])
        compiled = [{"name": "T", "severity": "critical", "website_id": None, "c_reg": re.compile("MATCH")}]
        thread = threading.Thread(  # burn-ignore-test-daemon-thread: daemon=True is the bound, as in test_02
            target=pager_log_analyzer.tail_file, args=(mock_r_client, tmp_path, compiled), daemon=True
        )
        thread.start()

        def wait_for(text):
            deadline = time.time() + 15
            while time.time() < deadline and text not in seen:
                with open(tmp_path, "a") as f:
                    f.write("padding line that makes the offset large enough to pass the truncated size\n" * 5)
                    f.write("MATCH before\n")
                time.sleep(0.2)
            return text in seen

        self.assertTrue(wait_for("MATCH before"))
        # copytruncate: same inode, the file becomes shorter than the reader's offset
        with open(tmp_path, "w") as f:
            f.write("MATCH after\n")
        deadline = time.time() + 15
        while time.time() < deadline and "MATCH after" not in seen:
            time.sleep(0.2)
        self.assertIn("MATCH after", seen, "a truncated log must be read again from the start")

    def test_08_main_exits_the_process_when_the_search_listener_fails(self):
        # Tests [@ANCHOR: pager_duty:log_analyzer_main]
        with tempfile.TemporaryDirectory() as tmpdir:
            config_path = os.path.join(tmpdir, "pager_config.json")
            with open(config_path, "w", encoding="utf-8") as f:
                json.dump({"log_analyzer": {"files": ["/var/log/x.log"], "patterns": []}}, f)
            env_patch = patch.dict(os.environ, {"PAGER_CONFIG_PATH": config_path, "REDIS_HOST": "192.0.2.7"})
            env_patch.start()
            self.addCleanup(env_patch.stop)
            module = "odoo.addons.pager_duty.daemon.pager_log_analyzer"
            mock_redis_cls = self.safe_patch(module + ".redis.Redis")
            mock_redis_cls.return_value.ping.return_value = True
            self.safe_patch(module + ".os.geteuid", return_value=1000)
            self.safe_patch(module + ".ThreadPoolExecutor")
            self.safe_patch(module + ".redis_search_listener", side_effect=RuntimeError("listener died"))
            mock_exit = self.safe_patch(module + ".os._exit")
            pager_log_analyzer.main()
            mock_exit.assert_called_once_with(1)
            self.assertEqual(mock_redis_cls.call_args.kwargs["host"], "192.0.2.7", "Redis is connected to by the resolved address")

    def test_09_main_refuses_to_run_as_root_when_the_privilege_drop_fails(self):
        # Tests [@ANCHOR: pager_duty:log_analyzer_main]
        with tempfile.TemporaryDirectory() as tmpdir:
            config_path = os.path.join(tmpdir, "pager_config.json")
            with open(config_path, "w", encoding="utf-8") as f:
                json.dump({"log_analyzer": {"files": ["/var/log/x.log"], "patterns": []}}, f)
            env_patch = patch.dict(os.environ, {"PAGER_CONFIG_PATH": config_path, "REDIS_HOST": "192.0.2.7"})
            env_patch.start()
            self.addCleanup(env_patch.stop)
            module = "odoo.addons.pager_duty.daemon.pager_log_analyzer"
            mock_redis_cls = self.safe_patch(module + ".redis.Redis")
            mock_redis_cls.return_value.ping.return_value = True
            self.safe_patch(module + ".os.geteuid", return_value=0)
            self.safe_patch(module + ".pwd.getpwnam", return_value=MagicMock(pw_uid=65534))
            self.safe_patch(module + ".grp.getgrnam", return_value=MagicMock(gr_gid=4))
            self.safe_patch(module + ".os.chdir")
            self.safe_patch(module + ".os.chroot")
            self.safe_patch(module + ".ctypes.CDLL")
            self.safe_patch(module + ".os.setgroups")
            self.safe_patch(module + ".os.setresgid")
            self.safe_patch(module + ".os.setresuid", side_effect=PermissionError("not allowed"))
            mock_executor = self.safe_patch(module + ".ThreadPoolExecutor")
            with self.assertRaises(SystemExit) as ctx:
                pager_log_analyzer.main()
            self.assertEqual(ctx.exception.code, 1)
            mock_executor.assert_not_called()

    def test_10_the_thread_pool_class_is_bound_at_import_before_the_chroot(self):
        # main() builds its ThreadPoolExecutor after chrooting to /var/log, where no Python library can be loaded;
        # concurrent.futures imports its .thread submodule lazily, so the class must already be bound at import.
        self.assertIs(pager_log_analyzer.ThreadPoolExecutor, concurrent.futures.thread.ThreadPoolExecutor)
        with open(pager_log_analyzer.__file__) as f:
            source = f.read()
        self.assertNotIn("concurrent.futures.ThreadPoolExecutor(", source)
