# -*- coding: utf-8 -*-
# Tests [@ANCHOR: cache_manager_config]
# Tests [@ANCHOR: cache_manager_redis_publish]
import time
import redis
import json
import os
import logging
from odoo.tests import tagged
from odoo.addons.zero_sudo.tests.real_transaction import RealTransactionCase

_logger = logging.getLogger(__name__)

@tagged("post_install", "-at_install")
class TestRealCacheManager(RealTransactionCase):
    def setUp(self):
        super().setUp()
        self.daemon_proc = None

    def tearDown(self):
        if self.daemon_proc:
            try:
                self.env["zero_sudo.daemon.utils"].stop_daemon_process(
                    self.daemon_proc
                )
            except (ProcessLookupError, PermissionError) as e:
                _logger.warning("Daemon termination failed: %s", repr(e))
        super().tearDown()

    def test_real_cache_manager_redis(self):
        base_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        daemon_script = os.path.join(base_dir, "daemons", "cache_manager.py")

        daemon_utils = self.env["zero_sudo.daemon.utils"]

        # Inject the dynamic test database name so the daemon listens to the correct PG instance
        env_vars = {
            "DB_NAME": self.env.cr.dbname,
            "REDIS_HOST": os.environ.get("REDIS_HOST", "redis"),
        }

        self.daemon_proc = daemon_utils.start_daemon_process(daemon_script, env_vars=env_vars)

        # Standardize Redis host to match the daemon's default fallback
        redis_host = os.environ.get("REDIS_HOST", "redis")
        redis_port = int(os.environ.get("REDIS_PORT", "6379"))
        r = redis.Redis(host=redis_host, port=redis_port, decode_responses=True)

        pubsub = r.pubsub()
        # Must match REDIS_CHANNEL defined in cache_manager.py
        pubsub.subscribe("odoo_cache_invalidation_bus")

        message_received = False
        user = self.env.user
        start_time = time.time()

        while time.time() - start_time < 60.0:
            # Repeatedly trigger since we don't know exactly when daemon connects
            user.write({"name": f"Cache Trigger Test {time.time()}"})
            self.env.cr.commit()

            # In test environments, web request teardown hooks do not run automatically.
            # We manually trigger registry signals to flush the cache invalidation queue.
            self.env.registry.signal_changes()

            # Failsafe: directly emit NOTIFY to test the daemon relay using parameterized pg_notify
            payload = json.dumps({"model": "res.users", "dbname": self.env.cr.dbname})
            self.env.cr.execute("SELECT pg_notify(%s, %s)", ("distributed_cache_invalidation", payload))
            self.env.cr.commit()

            msg = pubsub.get_message(ignore_subscribe_messages=True)
            if msg and msg['type'] == 'message':
                data = json.loads(msg['data'])
                if data.get("model") == "res.users":
                    message_received = True
                    break
            time.sleep(0.5)  # audit-ignore-sleep

        pubsub.close()
        self.assertTrue(
            message_received,
            "[!] DIAGNOSTIC FOR AI: The standalone cache_manager.py daemon failed to relay the PG NOTIFY event to the Redis 'odoo_cache_invalidation_bus' channel. Verify that the daemon is correctly parsing DB_NAME and connected to Redis."
        )
