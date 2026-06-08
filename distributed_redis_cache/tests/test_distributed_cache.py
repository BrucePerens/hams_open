import os
import logging
import odoo.tests
from odoo import _
from odoo.addons.zero_sudo.tests.common import HamsHttpCase

_logger = logging.getLogger(__name__)

# Tests [@ANCHOR: redis_connection_pool]
# Tests [@ANCHOR: distributed_cache_key_generation]
# Tests [@ANCHOR: distributed_cache_decorator]
# Tests [@ANCHOR: invalidate_model_cache_logic]
# Tests [@ANCHOR: notify_model_invalidation_logic]
# Tests [@ANCHOR: redis_cache_interceptor]
# Tests [@ANCHOR: manual_cache_invalidation]
# Tests [@ANCHOR: check_redis_status_logic]
# Tests [@ANCHOR: distributed_cache_view]

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
                        "title": _("Success"),
                        "message": _("Redis is connected"),
                        "type": "success",
                        "sticky": False
                    },
                }

            def mock_action_invalidate(record):
                return {
                    "type": "ir.actions.client",
                    "tag": "display_notification",
                    "params": {
                        "title": _("Success"),
                        "message": _("Cache invalidated successfully"),
                        "type": "success",
                        "sticky": False
                    },
                }

            self.safe_patch_object(cache_model_cls, 'check_redis_status', mock_check_redis_status)
            self.safe_patch_object(cache_model_cls, 'action_invalidate_model_cache', mock_action_invalidate)

        # Run the tour (Mocked in standard, Real Daemons in integration)
        if is_integration:
            # Signal the server to use real Redis during this tour session.
            # We use a system parameter as a global toggle for the test session.
            self.env['ir.config_parameter'].with_user(1).set_param('distributed_redis_cache.test_integration_active', '1')

        try:
            self.start_tour("/odoo?debug=1", "distributed_cache_admin_tour", login="admin")
        except Exception as e:  # audit-ignore-catch-all
            # [!] DIAGNOSTIC FOR AI: UI Tour 'distributed_cache_admin_tour' failed.
            _logger.error("Tour failed: %s", e)
            raise e
        finally:
            if is_integration:
                self.env['ir.config_parameter'].with_user(1).set_param('distributed_redis_cache.test_integration_active', '0')
