# -*- coding: utf-8 -*-
import time
import redis
import json
import os
import logging
from odoo.tests import tagged
from odoo.addons.hams_test.tests.real_transaction import RealTransactionCase

_logger = logging.getLogger(__name__)

@tagged("post_install", "-at_install")
class TestRealCacheManager(RealTransactionCase):
    def setUp(self):
        super().setUp()
        self.daemon_proc = None

    def tearDown(self):
        if self.daemon_proc:
            try:
                self.daemon_proc.terminate()
                self.daemon_proc.wait(timeout=2.0)
            except Exception as e: # audit-ignore-catch-all
                _logger.warning("Daemon termination ignored: %s", repr(e))
        super().tearDown()

    def test_real_cache_manager_redis(self):
        base_dir = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        daemon_script = os.path.join(base_dir, "daemons", "cache_manager.py")

        daemon_utils = self.env["zero_sudo.daemon.utils"]
        self.daemon_proc = daemon_utils.start_daemon_process(daemon_script)

        r = redis.Redis(host=os.environ.get("REDIS_HOST", "redis"), decode_responses=True)
        pubsub = r.pubsub()
        pubsub.subscribe("odoo_cache_invalidation")

        message_received = False
        user = self.env.user
        start_time = time.time()

        while time.time() - start_time < 60.0:
            # Repeatedly trigger since we don't know exactly when daemon connects
            user.write({"name": f"Cache Trigger Test {time.time()}"})

            msg = pubsub.get_message(ignore_subscribe_messages=True)
            if msg and msg['type'] == 'message':
                data = json.loads(msg['data'])
                if data.get("model") == "res.users":
                    message_received = True
                    break
            time.sleep(0.5)  # audit-ignore-sleep

        pubsub.close()
        self.assertTrue(message_received, "The actual daemon failed to relay PG NOTIFY to Redis!")
