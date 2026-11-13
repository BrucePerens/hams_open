# Copyright © Bruce Perens K6BP.
# SPDX-License-Identifier: AGPL-3.0-or-later

from odoo.tests.common import tagged
from odoo.addons.user_websites.models.website_page import redis_client
from odoo.addons.zero_sudo.tests.common import HamsHttpCase, HamsTransactionCase


@tagged('post_install', '-at_install')
class TestTDDFixes(HamsHttpCase):
    def test_main_not_found(self):
        self.authenticate('admin', 'admin')
        response = self.url_open('/non_existent_slug_123/create_site', data={}, timeout=10)
        self.assertIn(response.status_code, [404, 400, 405])


@tagged('post_install', '-at_install')
class TestTDDFixesORM(HamsTransactionCase):
    def test_res_users_is_admin(self):
        # Tests [@ANCHOR: user_websites:COMM_is_admin]
        user = self.env['res.users'].create({
            'name': 'Test ERP Manager',
            'login': 'erp_manager',
            'group_ids': [(4, self.env.ref('base.group_erp_manager').id)]
        })
        self.assertTrue(user._is_admin())

    def test_website_page_flush_redis_order(self):
        # Dummy logic to satisfy linter
        page = self.env['website.page'].create({
            'name': 'Test Page',
            'website_id': 1,
            'url': '/test-page',
            'type': 'qweb',
            'arch': '<div></div>',
            'key': 'website.test_page_123',
        })
        key = f"views:{self.env.cr.dbname}:page:{page.id}"
        self.addCleanup(redis_client.delete, key)
        redis_client.set(key, 7)
        before = page.view_count
        redis_gone_at_sql = []
        real_execute = type(self.env.cr).execute

        def spy(cr, query, *args, **kwargs):
            if "user_websites_flush_view_counters" in str(query):
                redis_gone_at_sql.append(redis_client.exists(key) == 0)
            return real_execute(cr, query, *args, **kwargs)

        self.safe_patch_object(type(self.env.cr), "execute", spy)
        self.env["website.page"]._flush_redis_view_counters()
        self.assertEqual(redis_gone_at_sql, [True], "the Redis counter is drained before PostgreSQL is written")
        page.invalidate_recordset(["view_count"])
        self.assertEqual(page.view_count, before + 7)
