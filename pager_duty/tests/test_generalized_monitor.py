# SPDX-License-Identifier: AGPL-3.0-or-later
# This software is distributed under the terms of the Affero General Public License (AGPL-3).

# -*- coding: utf-8 -*-
import datetime
import json
from odoo import fields
import os
import socket
import time
from unittest.mock import MagicMock

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase

# Utilize implicit namespace packages instead of sys.path manipulation
import odoo.addons.pager_duty.daemon.generalized_monitor as generalized_monitor


from odoo.tools import mute_logger


@tagged("post_install", "-at_install")
class TestMonitorExhaustive(HamsTransactionCase):

    @mute_logger("generalized_monitor")
    def test_01_smtp_fallback(self):
        # Tests [@ANCHOR: daemon_report_incident]

        # Tests [@ANCHOR: pager_duty:fallback_notify]
        """Verify that if the Odoo client crashes, the report gracefully triggers the SMTP fallback."""
        mock_smtp = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.smtplib.SMTP"
        )

        orig_env = dict(os.environ)
        os.environ.update(
            {"PAGER_FALLBACK_EMAIL": "admin@test.com", "SMTP_HOST": "smtp.test.com"}
        )

        try:
            mock_client = MagicMock()
            mock_client.execute.side_effect = Exception(
                "Connection Refused (Odoo Down)"
            )

            mock_smtp_instance = MagicMock()
            mock_smtp.return_value.__enter__.return_value = mock_smtp_instance

            generalized_monitor.report(
                mock_client, "Database Check", "Lost connection to Postgres"
            )

            mock_client.execute.assert_called_once()
            mock_smtp.assert_called_with("smtp.test.com", 587, timeout=10)
            mock_smtp_instance.send_message.assert_called_once()

            em_arg = mock_smtp_instance.send_message.call_args[0][0]
            self.assertIn("Lost connection to Postgres", em_arg.get_content())
            self.assertEqual(em_arg["To"], "admin@test.com")
        finally:
            os.environ.clear()
            os.environ.update(orig_env)

    def test_02_parse_env_util(self):
        # Tests [@ANCHOR: pager_duty:parse_env]
        """Verify environment variable injection from YAML configs."""
        orig_env = dict(os.environ)
        os.environ["TEST_DB_NAME"] = "test_db_123"
        try:
            self.assertEqual(
                generalized_monitor.parse_env("ENV:TEST_DB_NAME"), "test_db_123"
            )
            self.assertEqual(
                generalized_monitor.parse_env("HardcodedString"), "HardcodedString"
            )
            self.assertEqual(generalized_monitor.parse_env(8080), 8080)
        finally:
            os.environ.clear()
            os.environ.update(orig_env)

    def test_03_system_checks(self):
        # Tests [@ANCHOR: daemon_execute_check]
        """Exhaustively verify Disk, Memory, CPU, IO Wait, and Steal metrics."""
        mock_psutil = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.psutil"
        )
        # Disk
        mock_psutil.disk_usage.return_value.percent = 95
        success, msg = generalized_monitor.execute_check(
            {
                "type": "system",
                "target": "disk",
                "critical": 90,
                "partition": "/dev/sda1",
            }
        )
        self.assertFalse(success)
        self.assertIn("Disk space", msg)
        self.assertIn("/dev/sda1", msg)

        mock_psutil.disk_usage.return_value.percent = 80
        success, msg = generalized_monitor.execute_check(
            {"type": "system", "target": "disk", "critical": 90}
        )
        self.assertTrue(success)

        # Memory
        mock_psutil.virtual_memory.return_value.percent = 95
        success, msg = generalized_monitor.execute_check(
            {"type": "system", "target": "memory", "critical": 90}
        )
        self.assertFalse(success)

        # CPU
        mock_psutil.cpu_percent.return_value = 95
        success, msg = generalized_monitor.execute_check(
            {"type": "system", "target": "cpu", "critical": 90}
        )
        self.assertFalse(success)

        # IO Wait & Steal
        mock_times = MagicMock()
        mock_times.iowait = 95
        mock_times.steal = 95
        mock_psutil.cpu_times_percent.return_value = mock_times
        success, msg = generalized_monitor.execute_check(
            {"type": "system", "target": "iowait", "critical": 90}
        )
        self.assertFalse(success)
        success, msg = generalized_monitor.execute_check(
            {"type": "system", "target": "steal", "critical": 90}
        )
        self.assertFalse(success)

    def test_04_dns_checks(self):
        """Verify un-cached root DNS lookup via dig with socket fallback."""
        mock_which = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.shutil.which"
        )
        mock_which.return_value = "/bin/mock"
        mock_run = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.subprocess.run"
        )
        mock_gethost = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.socket.gethostbyname"
        )

        # Test Dig Success
        mock_run.return_value.returncode = 0
        mock_run.return_value.stdout = "example.com"
        success, msg = generalized_monitor.execute_check(
            {"type": "dns", "target": "example.com"}
        )
        self.assertTrue(success)

        # Test Fallback Success
        mock_run.return_value.returncode = 1
        mock_gethost.return_value = "1.2.3.4"
        success, msg = generalized_monitor.execute_check(
            {"type": "dns", "target": "example.com"}
        )
        self.assertTrue(success)

        # Test Total Failure
        mock_gethost.side_effect = socket.gaierror("NXDOMAIN")
        success, msg = generalized_monitor.execute_check(
            {"type": "dns", "target": "example.com"}
        )
        self.assertFalse(success)
        self.assertIn("DNS resolution failed", msg)

    def test_05_http_checks(self):
        """Verify payload matching and 500 error detection on HTTP endpoints."""
        mock_urlopen = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.urllib.request.urlopen"
        )
        mock_resp = MagicMock()
        mock_resp.status = 200
        mock_resp.read.return_value = b'{"status": "ok"}'
        mock_urlopen.return_value.__enter__.return_value = mock_resp

        # Success
        success, msg = generalized_monitor.execute_check(
            {"type": "http", "target": "http://test", "expect": "ok"}
        )
        self.assertTrue(success)

        # Mismatch
        success, msg = generalized_monitor.execute_check(
            {"type": "http", "target": "http://test", "expect": "failed"}
        )
        self.assertFalse(success)
        self.assertIn("mismatch", msg)

        # HTTP 500
        mock_resp.status = 500
        success, msg = generalized_monitor.execute_check(
            {"type": "http", "target": "http://test"}
        )
        self.assertFalse(success)
        self.assertIn("HTTP status 500", msg)

    def test_06_tcp_checks(self):
        """Verify raw hex socket handshakes (e.g. RabbitMQ AMQP)."""
        mock_conn = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.socket.create_connection"
        )
        mock_socket = MagicMock()
        mock_conn.return_value.__enter__.return_value = mock_socket
        mock_socket.recv.return_value = b"+PONG"

        # String send/expect
        success, msg = generalized_monitor.execute_check(
            {
                "type": "tcp",
                "target": "redis",
                "port": 6379,
                "send": "PING",
                "expect": "+PONG",
            }
        )
        self.assertTrue(success)

        # Mismatch
        mock_socket.recv.return_value = b"ERR"
        success, msg = generalized_monitor.execute_check(
            {"type": "tcp", "target": "redis", "port": 6379, "expect": "+PONG"}
        )
        self.assertFalse(success)

        # Hex send
        success, msg = generalized_monitor.execute_check(
            {"type": "tcp", "target": "rabbitmq", "port": 5672, "send_hex": "414d5150"}
        )
        self.assertTrue(success)
        mock_socket.sendall.assert_called_with(b"AMQP")

    def test_07_postgres_and_anomaly(self):
        """Verify raw database pinging and custom SQL mathematical anomaly detection."""
        mock_psycopg2 = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.psycopg2"
        )
        mock_conn = MagicMock()
        mock_psycopg2.connect.return_value = mock_conn
        mock_cursor = MagicMock()
        mock_conn.cursor.return_value.__enter__.return_value = mock_cursor
        mock_cursor.fetchone.return_value = [10]

        # Basic Postgres Ping
        success, msg = generalized_monitor.execute_check({"type": "postgres"})
        self.assertTrue(success)

        # Anomaly - Above Threshold
        success, msg = generalized_monitor.execute_check(
            {"type": "anomaly", "critical": 5}
        )
        self.assertTrue(success)

        # Anomaly - Below Threshold
        success, msg = generalized_monitor.execute_check(
            {"type": "anomaly", "critical": 15}
        )
        self.assertFalse(success)
        self.assertIn("Breached", msg)

    def test_08_ssl_checks(self):
        """Verify Let's Encrypt certificate expiration date math."""
        self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.socket.create_connection"
        )
        mock_ssl_ctx = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.ssl.create_default_context"
        )

        mock_ctx = MagicMock()
        mock_ssl_ctx.return_value = mock_ctx
        mock_ssock = MagicMock()
        mock_ctx.wrap_socket.return_value.__enter__.return_value = mock_ssock

        # Future Cert
        future_date = fields.Datetime.now() + datetime.timedelta(days=20)
        mock_ssock.getpeercert.return_value = {
            "notAfter": future_date.strftime("%b %d %H:%M:%S %Y GMT")
        }

        success, msg = generalized_monitor.execute_check(
            {"type": "ssl", "target": "example.com", "critical": 14}
        )
        self.assertTrue(success)

        # Expiring Cert
        expiring_date = fields.Datetime.now() + datetime.timedelta(days=5)
        mock_ssock.getpeercert.return_value = {
            "notAfter": expiring_date.strftime("%b %d %H:%M:%S %Y GMT")
        }

        success, msg = generalized_monitor.execute_check(
            {"type": "ssl", "target": "example.com", "critical": 14}
        )
        self.assertFalse(success)
        self.assertIn("expires in", msg)

    def test_09_process_wrappers(self):
        # Tests [@ANCHOR: pager_duty:ensure_executable]
        """Verify synthetic journey scripts, logrotate, nginx syntax, and cloudflared tunnel checks."""
        mock_which = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.shutil.which"
        )
        mock_which.return_value = "/bin/mock"
        mock_run = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.subprocess.run"
        )
        mock_run.return_value.returncode = 0

        # Synthetic
        success, msg = generalized_monitor.execute_check(
            {"type": "synthetic", "script": "/bin/true"}
        )
        self.assertTrue(success)

        mock_run.return_value.returncode = 1
        mock_run.return_value.stderr = "Execution failed"
        success, msg = generalized_monitor.execute_check(
            {"type": "synthetic", "script": "/bin/false"}
        )
        self.assertFalse(success)
        self.assertIn("Execution failed", msg)

        # pg_dump
        mock_run.return_value.returncode = 0
        success, msg = generalized_monitor.execute_check(
            {"type": "pg_dump", "dbname": "test"}
        )
        self.assertTrue(success)

        # nginx
        success, msg = generalized_monitor.execute_check({"type": "nginx"})
        self.assertTrue(success)

        # logrotate
        success, msg = generalized_monitor.execute_check({"type": "logrotate"})
        self.assertTrue(success)

        # cloudflared
        success, msg = generalized_monitor.execute_check(
            {"type": "cloudflared", "target": "my-tunnel"}
        )
        self.assertTrue(success)

    def test_10_certbot_check(self):
        # Tests [@ANCHOR: daemon_verify_dependencies]
        """Verify the Certbot 3-stage readiness pipeline (API, DNS IP Match, Dry-Run)."""
        mock_which = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.shutil.which"
        )
        mock_which.return_value = "/usr/bin/certbot"
        mock_run = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.subprocess.run"
        )
        mock_urlopen = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.urllib.request.urlopen"
        )

        # Pass API Check
        mock_api_resp = MagicMock()
        mock_api_resp.status = 200
        mock_urlopen.return_value.__enter__.return_value = mock_api_resp

        # Pass Dry Run
        mock_run.return_value.returncode = 0

        success, msg = generalized_monitor.execute_check({"type": "certbot"})
        self.assertTrue(success)

        # Fail Dry Run
        mock_run.return_value.returncode = 1
        mock_run.return_value.stdout = "Challenge failed for domain"
        mock_run.return_value.stderr = ""
        success, msg = generalized_monitor.execute_check({"type": "certbot"})
        self.assertFalse(success)
        self.assertIn("Challenge failed", msg)

    def test_11_smtp_dryrun(self):
        """Verify SMTP pre-flight handshake."""
        mock_smtp = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.smtplib.SMTP"
        )
        mock_server = MagicMock()
        mock_smtp.return_value.__enter__.return_value = mock_server

        success, msg = generalized_monitor.execute_check(
            {"type": "smtp_dryrun", "target": "smtp.gmail.com"}
        )
        self.assertTrue(success)
        mock_server.ehlo.assert_called()

    def test_12_new_protocols_and_rpcs(self):
        """Verify the new natively supported sub-protocols (UDP, HTTP3, XML-RPC, JSON-RPC, native Redis, native RabbitMQ)."""
        mock_which = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.shutil.which"
        )
        mock_which.return_value = "/bin/mock"
        mock_socket = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.socket.socket"
        )
        mock_urlopen = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.urllib.request.urlopen"
        )
        mock_xmlrpc = self.safe_patch("xmlrpc.client.ServerProxy")
        mock_run = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.subprocess.run"
        )

        # 1. UDP Datagram
        mock_sock_inst = MagicMock()
        mock_socket.return_value.__enter__.return_value = mock_sock_inst
        mock_sock_inst.recvfrom.return_value = (b"PONG", ("odoo", 80))
        success, msg = generalized_monitor.execute_check(
            {"type": "udp", "target": "odoo", "send": "PING", "expect": "PONG"}
        )
        self.assertTrue(success)
        mock_sock_inst.sendto.assert_called()

        # 2. HTTP/3 (QUIC)
        mock_run.return_value.returncode = 0
        mock_run.return_value.stdout = '{"status": "ok"}'
        success, msg = generalized_monitor.execute_check(
            {"type": "http3", "target": "https://example.com", "expect": "ok"}
        )
        self.assertTrue(success)
        mock_run.assert_called_with(
            ["/bin/mock", "-s", "--http3", "--", "https://example.com"],
            capture_output=True,
            text=True,
            timeout=10,
        )

        # 3. XML-RPC
        mock_proxy_inst = MagicMock()
        mock_xmlrpc.return_value = mock_proxy_inst
        mock_proxy_inst.test_method.return_value = "RPC_SUCCESS"
        success, msg = generalized_monitor.execute_check(
            {
                "type": "xmlrpc",
                "target": "http://odoo:8069/xmlrpc/2/common",
                "rpc_method": "test_method",
                "rpc_params": "[1, 2]",
                "expect": "RPC_SUCCESS",
            }
        )
        self.assertTrue(success)
        mock_proxy_inst.test_method.assert_called_with(1, 2)

        # 4. JSON-RPC
        mock_resp = MagicMock()
        mock_resp.status = 200
        mock_resp.read.return_value = b'{"result": "JSON_SUCCESS"}'
        mock_urlopen.return_value.__enter__.return_value = mock_resp
        success, msg = generalized_monitor.execute_check(
            {
                "type": "jsonrpc",
                "target": "http://odoo:8069/jsonrpc",
                "rpc_method": "test_json",
                "expect": "JSON_SUCCESS",
            }
        )
        self.assertTrue(success)

        # 5. Native Redis (via library)
        mock_redis_lib = self.safe_patch("redis.Redis")
        mock_redis_inst = MagicMock()
        mock_redis_lib.return_value = mock_redis_inst
        mock_redis_inst.ping.return_value = True
        success, msg = generalized_monitor.execute_check(
            {"type": "redis", "target": "redis"}
        )
        self.assertTrue(success)
        mock_redis_inst.ping.assert_called_once()

        # 6. Native RabbitMQ
        mock_conn = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.socket.create_connection"
        )
        mock_tcp_sock = MagicMock()
        mock_conn.return_value.__enter__.return_value = mock_tcp_sock
        mock_tcp_sock.recv.return_value = b"\x01\x02\x03"
        success, msg = generalized_monitor.execute_check(
            {"type": "rabbitmq", "target": "rabbitmq"}
        )
        self.assertTrue(success)
        mock_tcp_sock.sendall.assert_called_with(b"AMQP\x00\x00\x09\x01")

    def test_13_additional_facilities(self):
        """Verify ICMP, Docker, Memcached, SSH, and Systemd checks."""
        mock_which = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.shutil.which"
        )
        mock_which.return_value = "/bin/mock"
        mock_run = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.subprocess.run"
        )
        mock_conn = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.socket.create_connection"
        )

        # ICMP
        mock_run.return_value.returncode = 0
        success, msg = generalized_monitor.execute_check(
            {"type": "icmp", "target": "8.8.8.8"}
        )
        self.assertTrue(success)

        # Docker
        mock_run.reset_mock()
        mock_run.return_value.returncode = 0
        mock_run.return_value.stdout = "true\n"
        success, msg = generalized_monitor.execute_check(
            {"type": "docker", "target": "odoo_container"}
        )
        self.assertTrue(success)
        # Verify docker inspect call
        docker_call = [
            call
            for call in mock_run.call_args_list
            if "/bin/mock" in call[0][0] and "inspect" in call[0][0]
        ]
        self.assertTrue(docker_call, "Docker call not found")

        # Systemd
        mock_run.return_value.returncode = 0
        mock_run.return_value.stdout = "active\n"
        success, msg = generalized_monitor.execute_check(
            {"type": "systemd", "target": "nginx"}
        )
        self.assertTrue(success)

        # Systemd Wildcard (Failed Services)
        mock_run.return_value.returncode = 0
        mock_run.return_value.stdout = (
            "bad.service loaded failed failed\nignored.service loaded failed failed\n"
        )
        success, msg = generalized_monitor.execute_check(
            {"type": "systemd", "target": "*", "ignored_services": "ignored.service"}
        )
        self.assertFalse(success)
        self.assertIn("bad.service", msg)
        self.assertNotIn("ignored.service", msg)

        # Memcached
        mock_sock = MagicMock()
        mock_conn.return_value.__enter__.return_value = mock_sock
        mock_sock.recv.return_value = b"STAT pid 1234\r\n"
        success, msg = generalized_monitor.execute_check(
            {"type": "memcached", "target": "odoo"}
        )
        self.assertTrue(success)
        mock_sock.sendall.assert_any_call(b"stats\r\n")

        # SSH
        mock_sock.recv.return_value = b"SSH-2.0-OpenSSH_8.4p1 Debian-5+deb11u1\r\n"
        success, msg = generalized_monitor.execute_check(
            {"type": "ssh", "target": "odoo"}
        )
        self.assertTrue(success)

        # Heartbeat
        mock_client = MagicMock()
        mock_client.execute.return_value = True
        success, msg = generalized_monitor.execute_check(
            {"type": "heartbeat", "uuid": "1234", "interval": 60}, client=mock_client
        )
        self.assertTrue(success)
        mock_client.execute.assert_called_with(
            "pager.check", "check_heartbeat_rpc", hb_uuid="1234", interval=60
        )

    def test_14_nagios_checks(self):
        """Verify Nagios-style checks: load, ftp, imap, pop3, mysql, snmp."""
        mock_load = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.os.getloadavg"
        )
        mock_load.return_value = (1.5, 1.0, 0.5)

        # Load
        success, msg = generalized_monitor.execute_check(
            {"type": "load", "critical": 1}
        )
        self.assertFalse(success, msg)
        self.assertIn("exceeds", msg)

        success, msg = generalized_monitor.execute_check(
            {"type": "load", "critical": 5}
        )
        self.assertTrue(success, msg)

        # FTP
        mock_ftp = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.ftplib.FTP"
        )
        mock_inst = MagicMock()
        mock_ftp.return_value.__enter__.return_value = mock_inst
        success, msg = generalized_monitor.execute_check(
            {"type": "ftp", "target": "odoo"}
        )
        self.assertTrue(success, msg)
        mock_inst.login.assert_called()

        # IMAP
        mock_imap = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.imaplib.IMAP4"
        )
        mock_inst = MagicMock()
        mock_imap.return_value = mock_inst
        success, msg = generalized_monitor.execute_check(
            {"type": "imap", "target": "odoo"}
        )
        self.assertTrue(success, msg)
        mock_inst.logout.assert_called()

        # POP3
        mock_pop3 = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.poplib.POP3"
        )
        mock_inst = MagicMock()
        mock_pop3.return_value = mock_inst
        success, msg = generalized_monitor.execute_check(
            {"type": "pop3", "target": "odoo"}
        )
        self.assertTrue(success, msg)
        mock_inst.quit.assert_called()

        # MySQL / MariaDB
        mock_pymysql = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.pymysql"
        )
        mock_conn = MagicMock()
        mock_pymysql.connect.return_value = mock_conn
        mock_cur = MagicMock()
        mock_conn.cursor.return_value.__enter__.return_value = mock_cur
        mock_cur.fetchone.return_value = [1]
        success, msg = generalized_monitor.execute_check(
            {"type": "mysql", "target": "odoo"}
        )
        self.assertTrue(success, msg)
        mock_cur.execute.assert_called_with("SELECT 1;")

        # LDAP (Fallback)
        mock_ldap3 = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.ldap3"
        )
        mock_inst = MagicMock()
        mock_ldap3.Server.return_value = MagicMock()
        mock_ldap3.Connection.return_value = mock_inst
        success, msg = generalized_monitor.execute_check(
            {"type": "ldap", "target": "odoo"}
        )
        self.assertTrue(success, msg)
        mock_inst.unbind.assert_called()

        # NTP (Fallback)
        mock_ntplib = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.ntplib"
        )
        mock_inst = MagicMock()
        mock_ntplib.NTPClient.return_value = mock_inst
        mock_inst.request.return_value.offset = 0.1
        success, msg = generalized_monitor.execute_check(
            {"type": "ntp", "target": "odoo"}
        )
        self.assertTrue(success, msg)

        # SNMP
        mock_which_snmp = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.shutil.which"
        )
        mock_which_snmp.return_value = "/bin/mock"
        mock_run_snmp = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.subprocess.run"
        )

        mock_run_snmp.return_value = MagicMock(
            returncode=0, stdout="Timeout", stderr=""
        )
        success, msg = generalized_monitor.execute_check(
            {
                "type": "snmp",
                "target": "odoo",
                "snmp_oid": "1.3.6",
                "expect": "OK",
            }
        )
        self.assertFalse(success, msg)

        mock_run_snmp.return_value = MagicMock(returncode=0, stdout="OK", stderr="")
        success, msg = generalized_monitor.execute_check(
            {
                "type": "snmp",
                "target": "odoo",
                "snmp_oid": "1.3.6",
                "expect": "OK",
            }
        )
        self.assertTrue(success, msg)

    def test_15_synthetic_spool_reads(self):
        # Tests [@ANCHOR: daemon_main_loop]
        """Verify generalized_monitor correctly reads the airgapped synthetic spool JSON."""
        mock_exists = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.os.path.exists"
        )
        mock_mtime = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.os.path.getmtime"
        )
        mock_open = self.safe_patch("builtins.open")

        mock_exists.return_value = True
        mock_mtime.return_value = time.time() - 10  # Fresh

        mock_open.return_value.__enter__.return_value.read.return_value = json.dumps(
            {
                "My Playwright": {"success": True, "error": ""},
                "My Bash": {"success": False, "error": "Segfault"},
            }
        )

        # Success Match
        success, msg = generalized_monitor.execute_check(
            {"type": "playwright", "name": "My Playwright", "interval": 60}
        )
        self.assertTrue(success)

        # Failure Match
        success, msg = generalized_monitor.execute_check(
            {"type": "bash", "name": "My Bash", "interval": 60}
        )
        self.assertFalse(success)
        self.assertIn("Segfault", msg)

        # Stale Spool File
        mock_mtime.return_value = time.time() - 1000  # Stale
        success, msg = generalized_monitor.execute_check(
            {"type": "executable", "name": "My Bash", "interval": 60}
        )
        self.assertFalse(success)
        self.assertIn("stale", msg)

    def test_16_is_in_maintenance(self):
        # Tests [@ANCHOR: pager_duty:is_in_maintenance]
        now = datetime.datetime.utcnow()
        past = (now - datetime.timedelta(hours=1)).strftime("%Y-%m-%d %H:%M:%S")
        future = (now + datetime.timedelta(hours=1)).strftime("%Y-%m-%d %H:%M:%S")
        self.assertTrue(
            generalized_monitor.is_in_maintenance(
                {"maint_start": past, "maint_end": future}
            ),
            "A check whose maintenance window brackets 'now' must be suppressed.",
        )
        self.assertFalse(
            generalized_monitor.is_in_maintenance({"maint_start": past, "maint_end": past}),
            "An already-lapsed maintenance window must not suppress the check.",
        )
        self.assertFalse(
            generalized_monitor.is_in_maintenance({}),
            "No maintenance window configured at all must not suppress the check.",
        )
        self.assertFalse(
            generalized_monitor.is_in_maintenance({"maint_start": "not-a-date", "maint_end": future}),
            "[!] DIAGNOSTIC FOR AI: an unparseable maintenance timestamp must fail safe (not suppressed), not crash.",
        )

    def test_17_auto_resolve_calls_the_rpc_and_survives_a_failure(self):
        # Tests [@ANCHOR: pager_duty:auto_resolve]
        mock_client = MagicMock()
        generalized_monitor.auto_resolve(mock_client, "My Check", website_id=5)
        mock_client.execute.assert_called_once_with(
            "pager.incident",
            "auto_resolve_incidents",
            source="My Check",
            context={"website_id": 5},
        )

        # Must not raise even when the RPC itself fails.
        mock_client.execute.side_effect = Exception("RPC down")
        generalized_monitor.auto_resolve(mock_client, "My Check")

    def test_18_odoo_client_execute_posts_json2_and_parses_the_response(self):
        # Tests [@ANCHOR: pager_duty:odoo_client_init]

        # Tests [@ANCHOR: pager_duty:odoo_client_execute]
        mock_response = MagicMock()
        mock_response.read.return_value = json.dumps({"result": 42}).encode("utf-8")
        mock_urlopen = self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.urllib.request.urlopen"
        )
        mock_urlopen.return_value.__enter__.return_value = mock_response

        client = generalized_monitor.OdooClient(
            "http://odoo:8069/", "test_db", "pager_service_internal", "secret-pass"
        )
        self.assertEqual(client.url, "http://odoo:8069")
        self.assertEqual(client.headers["Authorization"], "bearer secret-pass")
        self.assertEqual(client.headers["X-Odoo-Database"], "test_db")

        result = client.execute("pager.check", "check_heartbeat_rpc", hb_uuid="x", interval_sec=60)
        self.assertEqual(result, {"result": 42})
        called_req = mock_urlopen.call_args[0][0]
        self.assertEqual(
            called_req.full_url, "http://odoo:8069/json/2/pager.check/check_heartbeat_rpc"
        )

    def test_19_get_odoo_client_reads_env_and_falls_back_to_odoo_db(self):
        # Tests [@ANCHOR: pager_duty:get_odoo_client]
        orig_env = dict(os.environ)
        os.environ.update(
            {
                "ODOO_URL": "http://odoo-test:8069",
                "ODOO_DB": "my_test_db",
                "ODOO_USER": "test_user",
                "ODOO_PASSWORD": "test_pass",
            }
        )
        try:
            client = generalized_monitor.get_odoo_client(generalized_monitor.logger, {})
            self.assertIsNotNone(client)
            self.assertEqual(client.url, "http://odoo-test:8069")
            self.assertEqual(client.db, "my_test_db")
        finally:
            os.environ.clear()
            os.environ.update(orig_env)

    def test_20_polling_thread_runs_one_real_cycle_then_exits(self):
        # Tests [@ANCHOR: pager_duty:polling_thread]

        class _StopLoop(Exception):
            pass

        self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.time.sleep",
            side_effect=_StopLoop,
        )
        mock_client = MagicMock()
        mock_client.execute.return_value = True
        check = {
            "id": 99,
            "name": "Polling Thread Test Check",
            "type": "system",
            "target": "memory",
            "interval": 1,
        }
        with self.assertRaises(_StopLoop):
            generalized_monitor.polling_thread(mock_client, check)
        # The startup jitter sleep is the very first call to time.sleep()
        # this function makes -- reaching it proves the real first-pass
        # execute_check()/status-report logic above it already ran.
        self.assertIn("Polling Thread Test Check", generalized_monitor.THREAD_HEARTBEATS)

    def test_21_log_tail_thread_runs_one_real_cycle_then_exits(self):
        # Tests [@ANCHOR: pager_duty:log_tail_thread]

        class _StopLoop(Exception):
            pass

        self.safe_patch(
            "odoo.addons.pager_duty.daemon.generalized_monitor.time.sleep",
            side_effect=_StopLoop,
        )
        mock_client = MagicMock()
        check = {
            "name": "Log Tail Test Check",
            "type": "log",
            "target": "/var/log/does-not-exist-log-tail-test.log",
            "regex": "ERROR",
            "interval": 1,
        }
        with self.assertRaises(_StopLoop):
            generalized_monitor.log_tail_thread(mock_client, check)
        self.assertIn("Log Tail Test Check", generalized_monitor.THREAD_TIMEOUTS)
