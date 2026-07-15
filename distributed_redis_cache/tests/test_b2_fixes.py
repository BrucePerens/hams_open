# This software is distributed under the terms of the Affero General Public License (AGPL-3).

import datetime
import ast
import os
from unittest.mock import MagicMock

from odoo.tests.common import tagged
from odoo.addons.zero_sudo.tests.common import HamsTransactionCase
from odoo.addons.distributed_redis_cache import redis_pool
from odoo.addons.distributed_redis_cache import redis_cache
from odoo.addons.distributed_redis_cache.models import ir_http

@tagged('post_install', '-at_install')
class TestB2Fixes(HamsTransactionCase):
    
    def setUp(self):
        super().setUp()
        try:
            redis_pool._db_configs = {}
        except Exception:
            _logger.debug("no configs yet")
        redis_pool._custom_pools = {}
            
    def test_b2_1_redis_pool_caching(self):
        env_mock = MagicMock()
        env_mock.cr.dbname = "test_db"
        sec_mock = MagicMock()
        sec_mock._get_system_param.side_effect = ["redis", "6379", "password"] * 5
        env_mock.__getitem__.return_value.with_context.return_value = sec_mock
        
        redis_pool.get_redis_connection(env_mock)
        redis_pool.get_redis_connection(env_mock)
        
        self.assertEqual(sec_mock._get_system_param.call_count, 3, "Should cache by dbname and not query again")

    def test_b2_2_cache_key_secondary_companies(self):
        @redis_cache.distributed_cache()
        def dummy_method(self):
            return "ok"
            
        class MockSelf:
            def __init__(self, allowed_companies):
                self.env = type("Env", (), {
                    "context": {"allowed_company_ids": allowed_companies},
                    "cr": type("cr", (), {"dbname": "test"})
                })
                self._name = "test.model"

        with redis_cache.LRU_LOCK:
            redis_cache._local_cache.clear()
            
        self.safe_patch('odoo.addons.distributed_redis_cache.redis_cache.redis_pool', None)
        obj1 = MockSelf([1, 2])
        dummy_method(obj1)
        
        obj2 = MockSelf([1, 3])
        dummy_method(obj2)
            
        with redis_cache.LRU_LOCK:
            keys = list(redis_cache._local_cache.d.keys())
            
        self.assertEqual(len(keys), 2, "Cache keys should differentiate between different secondary companies")

    def test_b2_3_banned_local_imports(self):
        with open(ir_http.__file__, "r") as f:
            content = f.read()
            
        tree = ast.parse(content)
        found_import = False
        inside_func = False
        for node in ast.walk(tree):
            if isinstance(node, ast.FunctionDef):
                for child in ast.walk(node):
                    if isinstance(child, ast.ImportFrom):
                        if child.module == "odoo.addons.distributed_redis_cache.redis_cache":
                            for name in child.names:
                                if name.name in ("_local_cache", "LRU_LOCK"):
                                    inside_func = True
            elif isinstance(node, ast.ImportFrom):
                if node.module == "odoo.addons.distributed_redis_cache.redis_cache":
                    for name in node.names:
                        if name.name in ("_local_cache", "LRU_LOCK"):
                            found_import = True
                            
        self.assertTrue(found_import, "Imports must be present")
        self.assertFalse(inside_func, "Imports must be at module level")

    def test_b2_4_ai_laziness_getattr(self):
        with open(ir_http.__file__, "r") as f:
            content = f.read()
        self.assertNotIn("getattr(cls", content, "Use explicit try...except instead of getattr")
        self.assertNotIn('getattr(cls, "_last_cache_counter"', content)

    def test_b2_5_readme_external_dependencies(self):
        readme_path = os.path.join(os.path.dirname(redis_pool.__file__), "README.md")
        with open(readme_path, "r") as f:
            content = f.read()
        self.assertIn("External Dependencies", content)
        self.assertIn("python-dotenv", content)

    def test_b2_6_pickle_serialization(self):
        @redis_cache.distributed_cache()
        def return_datetime(self):
            return datetime.datetime(2025, 1, 1, 12, 0, 0)
            
        class MockSelf:
            def __init__(self):
                self.env = type("Env", (), {
                    "context": {},
                    "cr": type("cr", (), {"dbname": "test"})
                })
                self._name = "test.model"

        with redis_cache.LRU_LOCK:
            redis_cache._local_cache.clear()
            
        mock_redis = MagicMock()
        mock_redis.get.return_value = None
        
        self.safe_patch('odoo.addons.distributed_redis_cache.redis_cache.get_redis_connection', MagicMock(return_value=mock_redis))
        self.safe_patch('odoo.addons.distributed_redis_cache.redis_cache.redis', MagicMock())
        self.safe_patch('odoo.addons.distributed_redis_cache.redis_cache.redis_pool', MagicMock())
        
        obj = MockSelf()
        res1 = return_datetime(obj)
        
        mock_redis.setex.assert_called_once()
        args, kwargs = mock_redis.setex.call_args
        data = args[2]
        
        mock_redis.get.return_value = data
        
        with redis_cache.LRU_LOCK:
            redis_cache._local_cache.clear()
            
        res2 = return_datetime(obj)
                    
        self.assertEqual(type(res1), datetime.datetime)
        self.assertEqual(type(res2), datetime.datetime)

    def test_b2_7_redis_pool_unsafe_type_casting(self):
        env_mock = MagicMock()
        env_mock.cr.dbname = "test_db7"
        sec_mock = MagicMock()
        sec_mock._get_system_param.side_effect = ["redis", "invalid_port", "password", "redis", "invalid_port", "password"]
        env_mock.__getitem__.return_value.with_context.return_value = sec_mock
        
        redis_pool.get_redis_connection(env_mock)

    def test_b2_8_parameter_mismatch(self):
        env_mock = MagicMock()
        env_mock.cr.dbname = "test_db8"
        sec_mock = MagicMock()
        sec_mock._get_system_param.side_effect = ["redis", "6379", "password", "redis", "6379", "password"]
        env_mock.__getitem__.return_value.with_context.return_value = sec_mock
        
        redis_pool.get_redis_connection(env_mock)
        calls = sec_mock._get_system_param.call_args_list
        keys = [call[0][0] for call in calls]
        self.assertIn("distributed_redis_cache.redis_password", keys)
        self.assertNotIn("distributed_redis_cache.redis_pass", keys)

    def test_b2_9_flake8_unused_variable(self):
        config_model_path = os.path.join(os.path.dirname(redis_pool.__file__), "models", "distributed_cache_config.py")
        with open(config_model_path, "r") as f:
            content = f.read()
        self.assertNotIn("integration_active =", content)
        self.assertNotIn("distributed_redis_cache.test_integration_active", content)
