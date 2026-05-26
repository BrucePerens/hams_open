import os
import odoo.tests
from hams_test.tests.common import HamsHttpCase

@odoo.tests.tagged('post_install', '-at_install')
class TestDistributedCacheTour(HamsHttpCase):

    def test_distributed_cache_admin_tour(self):
        """
        Executes the UI tour for the Distributed Redis Cache Manager.
        - Standard Mode: Mocks the backend RPCs to simulate success without network calls.
        - Integration Mode: Hits the real Redis daemon running in the Jules VM.
        """
        is_integration = os.environ.get("HAMS_INTEGRATION_MODE") == "1"

        if not is_integration:
            # Mock the backend RPC methods to simulate a healthy Redis connection
            cache_model_cls = type(self.env['distributed.cache.config'])

            def mock_check_redis_status(record):
                return {
                    "type": "ir.actions.client",
                    "tag": "display_notification",
                    "params": {
                        "title": "Success",
                        "message": "Redis is connected",
                        "type": "success",
                        "sticky": False
                    },
                }

            def mock_action_invalidate(record):
                return {
                    "type": "ir.actions.client",
                    "tag": "display_notification",
                    "params": {
                        "title": "Success",
                        "message": "Cache invalidated successfully",
                        "type": "success",
                        "sticky": False
                    },
                }

            self.safe_patch_object(cache_model_cls, 'check_redis_status', mock_check_redis_status)
            self.safe_patch_object(cache_model_cls, 'action_invalidate_model_cache', mock_action_invalidate)

        # Run the tour (Mocked in standard, Real Daemons in integration)
        self.start_tour("/odoo?debug=1", "distributed_cache_admin_tour", login="admin")
