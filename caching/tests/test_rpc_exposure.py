# SPDX-License-Identifier: AGPL-3.0-or-later

"""`/web/dataset/call_kw` reaches every public method. These methods act through a service account (or return what it reads), and
only Python and cron call them, so they are `@api.private`: an RPC call is refused whatever the caller's groups."""
from odoo.exceptions import AccessError
from odoo.service.model import get_public_method
from odoo.tests.common import tagged

from odoo.addons.zero_sudo.tests.common import HamsTransactionCase


@tagged("post_install", "-at_install")
class TestRpcRefusesServiceOnlyMethods(HamsTransactionCase):
    # Tests [@ANCHOR: caching_caching_mixin_caching_mixin]
    def test_service_only_methods_cannot_be_called_over_rpc(self):
        for model, name in (
            ('caching.mixin', 'get_fs_stats'),
            ('caching.mixin', 'get_global_static_info'),
        ):
            with self.subTest(model=model, method=name):
                with self.assertRaises(AccessError):
                    get_public_method(self.env[model], name)
